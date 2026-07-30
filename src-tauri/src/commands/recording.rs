//! Recording lifecycle commands: start/pause/resume/stop, in-recording
//! stars and live speaker renames, and the detection prompt responses.

#[cfg(feature = "cloud")]
use embral_types::AppConfig;
use embral_types::AppError;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Mutex;

use crate::audio::recorder::Recorder;
use crate::transcription::{self, TranscriptionEvent, TranscriptionSession};
use crate::AppState;

use super::finalize::{finalize_meeting, AudioSource};
use super::support::*;

/// Whether the configured local model is on disk — the gate for falling
/// back from cloud transcription mid-recording.
#[cfg(feature = "cloud")]
fn local_model_present(config: &AppConfig) -> bool {
    embral_engine::catalog::find(&config.meeting_asr_model()).is_some_and(|m| m.present())
}

#[cfg(feature = "cloud")]
use crate::transcription::TranscriptionProvider;

/// Distinct speakers past which live diarization is treated as having
/// failed rather than having found a crowd. Meetings with more real voices
/// than this exist, but a clusterer that keeps opening speakers is far
/// commoner — and confidently wrong labels cost the reader more than no
/// labels do ([speakers.md]).
const MAX_LIVE_SPEAKERS: usize = 6;

/// Whether the live labels have stopped being believable.
fn diarization_has_run_away(distinct_speakers: usize) -> bool {
    distinct_speakers > MAX_LIVE_SPEAKERS
}

/// Drop every speaker label from the accumulated segments. Turning
/// diarization off part-way through must not leave a half-labelled
/// transcript — that reads as "the app lost track", which is worse than
/// a transcript that never claimed to know.
async fn strip_speakers(segments: &crate::SharedSegments) {
    for seg in segments.lock().await.iter_mut() {
        seg.speaker = None;
        seg.speaker_id = None;
    }
}

/// One choke point for every start path (button, hotkey, detection accept,
/// auto-start): a refused start counts once, whatever refused it.
#[tauri::command]
pub async fn start_recording(app: AppHandle, state: State<'_, AppState>) -> Result<(), AppError> {
    let result = start_recording_inner(app, &state).await;
    if result.is_err() {
        crate::telemetry::track(
            &state,
            "error",
            serde_json::json!({ "category": "recording_start_failed" }),
        );
    }
    result
}

async fn start_recording_inner(app: AppHandle, state: &State<'_, AppState>) -> Result<(), AppError> {
    let config = state.config.lock().await.clone();

    // Nothing downstream is idempotent: a second start overwrites the
    // recorder, the session, and `in_progress.txt` while the first
    // recording's capture threads and transcription session keep running.
    if state.recorder.lock().await.is_some() {
        tracing::warn!("start requested while already recording — ignoring");
        return Err(AppError::AlreadyRecording);
    }
    // Who transcribes this meeting: the standing choice, bent by the power
    // policy. Read once, here — the lane is fixed for the meeting, and the
    // record gate has to ask about the lane the meeting is really taking.
    let power = crate::platform::power_source();
    let provider_choice = crate::config::provider_for_power(&config, power);
    if provider_choice != config.transcription_provider {
        tracing::info!(
            "power source is {power:?} — this meeting transcribes with {provider_choice:?}"
        );
    }
    if let Some(gap) = crate::config::missing_prerequisite(&config, &provider_choice) {
        tracing::warn!("refusing to record — {gap}");
        return Err(AppError::NotConfigured);
    }
    if state.dictating.load(std::sync::atomic::Ordering::Acquire) {
        return Err(AppError::BusyDictating);
    }

    let base = crate::storage::storage_base(&config.storage_dir);
    crate::storage::init_storage_dirs(&base).map_err(|e| e.to_string())?;

    let meeting_id = crate::storage::generate_meeting_id();
    let wav_path = base.join("audio").join(format!("{}.wav", meeting_id));

    // Reset the backend-side segment accumulator. The event forwarder below
    // populates it; stop_recording reads from it as source of truth.
    state.current_segments.lock().await.clear();
    // This recording's own diarization standing, from the setting. It can
    // only go off from here — by the toggle or the runaway guard.
    state.live_diarization.store(
        config.diarization_enabled,
        std::sync::atomic::Ordering::Release,
    );
    state
        .live_speaker_labels
        .lock()
        .expect("live speaker labels poisoned")
        .clear();
    state.live_label_renames.lock().await.clear();
    state.stars.lock().await.clear();
    state.star_anchors.lock().await.clear();
    let segments_acc = state.current_segments.clone();

    // Build transcription provider and open session. Signed out with
    // cloud selected, the relay handshake cannot succeed — and waiting out
    // its timeout would cost the first seconds of every meeting, so the
    // configured fallback applies immediately instead.
    #[cfg(feature = "cloud")]
    let signed_out_cloud = provider_choice == embral_types::TranscriptionProvider::Cloud
        && config.cloud_session_token.is_empty();
    #[cfg(not(feature = "cloud"))]
    let signed_out_cloud = false;

    let provider = if signed_out_cloud {
        tracing::info!("cloud selected but signed out — transcribing on this device");
        Arc::new(transcription::local::LocalProvider::new(
            state.engine.clone(),
            config.meeting_asr_model(),
            config.vocabulary.clone(),
            config.diarization_enabled,
        )) as Arc<dyn transcription::TranscriptionProvider>
    } else {
        transcription::build_provider(&provider_choice, &config, state.engine.clone())
    };
    let capabilities = provider.capabilities();
    // Snapshot for stop_recording: whether this session's labels are final
    // (cloud live diarization) or a provisional preview (local live labels).
    state
        .labels_authoritative
        .store(capabilities.labels_authoritative, std::sync::atomic::Ordering::Release);

    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<TranscriptionEvent>();

    let session_result = provider.start_session(event_tx.clone()).await;
    // A cloud refusal at start (out of hours, unreachable) degrades per the
    // out-of-hours setting — a local session, or no transcription at all —
    // never a dead record button.
    #[cfg(feature = "cloud")]
    let session_result: Result<Option<Box<dyn TranscriptionSession>>, _> = match session_result {
        Err(e) if provider_choice == embral_types::TranscriptionProvider::Cloud => {
            match crate::config::on_cloud_failure(
                config.cloud_out_of_hours,
                local_model_present(&config),
            ) {
                crate::config::CloudFailureAction::DisableTranscription => {
                    tracing::warn!(
                        "cloud transcription unavailable ({e}); recording without a transcript"
                    );
                    let _ = app.emit(
                        "transcription-disabled",
                        &AppError::Internal { detail: e.to_string() },
                    );
                    Ok(None)
                }
                crate::config::CloudFailureAction::SwitchToLocal => {
                    tracing::warn!("cloud connect failed ({e}); starting a local session instead");
                    state
                        .labels_authoritative
                        .store(false, std::sync::atomic::Ordering::Release);
                    let local = transcription::local::LocalProvider::new(
                        state.engine.clone(),
                        config.meeting_asr_model(),
                        config.vocabulary.clone(),
                        config.diarization_enabled,
                    );
                    let result = local.start_session(event_tx.clone()).await;
                    if result.is_ok() {
                        let _ = app.emit(
                            "transcription-fallback",
                            &AppError::CloudUnreachable,
                        );
                    }
                    result.map(Some)
                }
                crate::config::CloudFailureAction::Fail => Err(e),
            }
        }
        other => other.map(Some),
    };
    #[cfg(not(feature = "cloud"))]
    let session_result = session_result.map(Some);
    let session = session_result.map_err(|e| e.to_string())?;

    // Wrap session in Arc<Mutex<Option<...>>> so both the audio-bridge task
    // and the main recording state can access it. `None` is a live recording
    // with transcription disabled.
    let session_arc: Arc<Mutex<Option<Box<dyn TranscriptionSession>>>> =
        Arc::new(Mutex::new(session));
    let session_for_audio = session_arc.clone();

    // Audio bridge: drain audio chunks and call send_audio on the session
    let (audio_tx, mut audio_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<f32>>();

    // Samples fed to the session so far = the recording's stream clock.
    // The mid-recording fallback uses it to shift the replacement local
    // session's timestamps (which restart at zero) onto this clock.
    let samples_sent = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let samples_for_bridge = samples_sent.clone();

    tokio::spawn(async move {
        tracing::info!("Audio bridge task started");
        let mut chunk_n: usize = 0;
        let mut total_samples: usize = 0;
        let mut warned_no_session = false;
        while let Some(chunk) = audio_rx.recv().await {
            samples_for_bridge
                .fetch_add(chunk.len() as u64, std::sync::atomic::Ordering::Relaxed);
            if chunk_n == 0 {
                tracing::info!(
                    "Audio bridge: first chunk received ({} samples)",
                    chunk.len()
                );
            }
            total_samples += chunk.len();
            let guard = session_for_audio.lock().await;
            if let Some(s) = guard.as_ref() {
                warned_no_session = false;
                match s.send_audio(&chunk).await {
                    Ok(()) => {
                        if chunk_n == 0 {
                            tracing::info!("Audio bridge: first send_audio call succeeded");
                        }
                    }
                    Err(e) => {
                        tracing::error!(
                            "Audio bridge: send_audio failed at chunk {}: {}",
                            chunk_n,
                            e
                        );
                    }
                }
            } else if !warned_no_session {
                // Expected steady state when transcription is disabled — the
                // recorder keeps writing audio, nothing consumes the stream.
                // Say it once, not ten times a second.
                warned_no_session = true;
                tracing::info!(
                    "Audio bridge: no transcription session from chunk {} — audio continues to disk only",
                    chunk_n
                );
            }
            if (chunk_n + 1) % 50 == 0 {
                tracing::debug!(
                    "Audio bridge stats: {} chunks forwarded, {} total samples",
                    chunk_n + 1,
                    total_samples
                );
            }
            chunk_n += 1;
        }
        tracing::info!(
            "Audio bridge task exiting: {} chunks forwarded, {} total samples",
            chunk_n,
            total_samples
        );
    });

    // Event forwarder: emit transcription-{interim,segment} Tauri events AND
    // accumulate Segments into the AppState-owned Vec (source of truth).
    let app_clone = app.clone();
    let segments_acc_for_forwarder = segments_acc.clone();
    let recovery_base = base.clone();
    #[cfg(feature = "cloud")]
    let session_slot_for_fallback = session_arc.clone();
    #[cfg(feature = "cloud")]
    let event_tx_for_fallback = event_tx.clone();
    #[cfg(feature = "cloud")]
    let samples_for_fallback = samples_sent.clone();
    tokio::spawn(async move {
        #[cfg(feature = "cloud")]
        let mut fallen_back = false;
        while let Some(event) = event_rx.recv().await {
            match event {
                TranscriptionEvent::Interim { segment, tentative } => {
                    // Flat wire shape so the frontend interim payload reads as a
                    // TranscriptionSegment with one extra optional field.
                    #[derive(serde::Serialize)]
                    struct InterimPayload<'a> {
                        speaker: Option<&'a str>,
                        text: &'a str,
                        start: f64,
                        end: f64,
                        tentative_text: Option<&'a str>,
                    }
                    let payload = InterimPayload {
                        speaker: segment.speaker.as_deref(),
                        text: &segment.text,
                        start: segment.start,
                        end: segment.end,
                        tentative_text: tentative.as_deref(),
                    };
                    let _ = app_clone.emit("transcription-interim", &payload);
                }
                TranscriptionEvent::Segment(mut seg) => {
                    // Apply any live speaker renames so later utterances of a
                    // renamed cluster carry the user-given name everywhere.
                    if let Some(label) = seg.speaker.as_ref() {
                        let state = app_clone.state::<AppState>();
                        let renames = state.live_label_renames.lock().await;
                        if let Some(new_name) = renames.get(label) {
                            seg.speaker = Some(new_name.clone());
                        }
                    }
                    // The live label layer. Off — by the header toggle, or
                    // because the guard below tripped — means no label
                    // reaches the transcript at all, for local and cloud
                    // alike ([speakers.md]).
                    {
                        use std::sync::atomic::Ordering;
                        let state = app_clone.state::<AppState>();
                        if !state.live_diarization.load(Ordering::Acquire) {
                            seg.speaker = None;
                            seg.speaker_id = None;
                        } else if let Some(label) = seg.speaker.clone() {
                            let distinct = {
                                let mut seen = state
                                    .live_speaker_labels
                                    .lock()
                                    .expect("live speaker labels poisoned");
                                seen.insert(label);
                                seen.len()
                            };
                            if diarization_has_run_away(distinct) {
                                // Not a crowd — a clusterer inventing people.
                                // Stand down exactly as the button does,
                                // including for what is already on screen.
                                state.live_diarization.store(false, Ordering::Release);
                                seg.speaker = None;
                                seg.speaker_id = None;
                                strip_speakers(&segments_acc_for_forwarder).await;
                                tracing::info!(
                                    distinct,
                                    "too many speakers — diarization off for this recording"
                                );
                                let _ = app_clone.emit(
                                    "diarization-disabled",
                                    serde_json::json!({ "speakers": distinct }),
                                );
                            }
                        }
                    }
                    segments_acc_for_forwarder.lock().await.push(seg.clone());
                    // Straight to the recovery scratch too: until finalize
                    // runs, this Vec is the only copy of the meeting.
                    crate::recovery::append_segment(&recovery_base, &seg);
                    // The silence check-in's clock: a transcribed word is
                    // the proof the meeting is still going.
                    app_clone
                        .state::<AppState>()
                        .last_speech_at
                        .store(epoch_ms(), std::sync::atomic::Ordering::Release);
                    let _ = app_clone.emit("transcription-segment", &seg);
                }
                TranscriptionEvent::Failed { message } => {
                    // Mid-recording session death. Cloud builds swap in a
                    // local session on the same event channel; the offline
                    // core just ends the stream (local sessions don't emit
                    // this today).
                    #[cfg(feature = "cloud")]
                    {
                        if fallen_back {
                            tracing::error!("transcription failed after fallback: {message}");
                            break;
                        }
                        fallen_back = true;
                        let state = app_clone.state::<AppState>();
                        let config = state.config.lock().await.clone();
                        match crate::config::on_cloud_failure(
                            config.cloud_out_of_hours,
                            local_model_present(&config),
                        ) {
                            crate::config::CloudFailureAction::DisableTranscription => {
                                // The recording and the notes go on; the
                                // transcript ends here, as configured.
                                tracing::warn!(
                                    "cloud transcription ended ({message}); recording continues without a transcript"
                                );
                                *session_slot_for_fallback.lock().await = None;
                                let _ = app_clone.emit(
                                    "transcription-disabled",
                                    &AppError::Internal { detail: message.clone() },
                                );
                                break;
                            }
                            crate::config::CloudFailureAction::Fail => {
                                tracing::error!(
                                    "cloud transcription failed with no local model to fall back to: {message}"
                                );
                                let _ = app_clone.emit(
                                    "transcription-failed",
                                    &AppError::Internal { detail: message.clone() },
                                );
                                crate::telemetry::track(
                                    &app_clone.state::<AppState>(),
                                    "error",
                                    serde_json::json!({ "category": "transcription_failed" }),
                                );
                                break;
                            }
                            crate::config::CloudFailureAction::SwitchToLocal => {}
                        }
                        let offset = samples_for_fallback
                            .load(std::sync::atomic::Ordering::Relaxed)
                            as f64
                            / 16000.0;
                        let provider = transcription::local::LocalProvider::new(
                            state.engine.clone(),
                            config.meeting_asr_model(),
                            config.vocabulary.clone(),
                            config.diarization_enabled,
                        );
                        let (local_tx, mut local_rx) =
                            tokio::sync::mpsc::unbounded_channel::<TranscriptionEvent>();
                        match provider.start_session(local_tx).await {
                            Ok(new_session) => {
                                *session_slot_for_fallback.lock().await = Some(new_session);
                                // Local live labels are provisional again.
                                state.labels_authoritative.store(
                                    false,
                                    std::sync::atomic::Ordering::Release,
                                );
                                // The local session stamps from zero; shift
                                // onto the recording's stream clock so the
                                // transcript stays ordered across the swap.
                                let out_tx = event_tx_for_fallback.clone();
                                tokio::spawn(async move {
                                    while let Some(ev) = local_rx.recv().await {
                                        let shifted = match ev {
                                            TranscriptionEvent::Interim {
                                                mut segment,
                                                tentative,
                                            } => {
                                                segment.start += offset;
                                                segment.end += offset;
                                                TranscriptionEvent::Interim { segment, tentative }
                                            }
                                            TranscriptionEvent::Segment(mut seg) => {
                                                seg.start += offset;
                                                seg.end += offset;
                                                TranscriptionEvent::Segment(seg)
                                            }
                                            other => other,
                                        };
                                        if out_tx.send(shifted).is_err() {
                                            break;
                                        }
                                    }
                                });
                                tracing::warn!(
                                    "cloud transcription failed; switched to local: {message}"
                                );
                                let _ = app_clone.emit(
                                    "transcription-fallback",
                                    &AppError::Internal { detail: message.clone() },
                                );
                            }
                            Err(e) => {
                                tracing::error!("local fallback failed to start: {e}");
                                let _ = app_clone.emit(
                                    "transcription-failed",
                                    &AppError::Internal { detail: message.clone() },
                                );
                                crate::telemetry::track(
                                    &app_clone.state::<AppState>(),
                                    "error",
                                    serde_json::json!({ "category": "transcription_failed" }),
                                );
                                break;
                            }
                        }
                    }
                    #[cfg(not(feature = "cloud"))]
                    {
                        tracing::error!("transcription session failed: {message}");
                        let _ = app_clone.emit(
                            "transcription-failed",
                            &AppError::Internal { detail: message.clone() },
                        );
                        crate::telemetry::track(
                            &app_clone.state::<AppState>(),
                            "error",
                            serde_json::json!({ "category": "transcription_failed" }),
                        );
                        break;
                    }
                }
                TranscriptionEvent::Done => break,
            }
        }
    });

    // Start recorder (this also writes WAV to disk)
    let mic = Some(config.mic_device.as_str()).filter(|s| !s.trim().is_empty());
    let output = Some(config.output_device.as_str()).filter(|s| !s.trim().is_empty());
    // ~10 Hz pre-mix band spectra for the recording view's live meter.
    // Paused callbacks discard samples before the tap, so pausing silences
    // it.
    let app_level = app.clone();
    let level_cb: Box<dyn Fn(&[f32], &[f32]) + Send> = Box::new(move |mic, system| {
        let _ = app_level.emit(
            "audio-level",
            serde_json::json!({ "mic": mic, "system": system }),
        );
    });
    // Fresh recording, default selection: everything the machine plays.
    // The source picker narrows it live, and the lane re-reads this on
    // every supervision tick.
    *state
        .system_audio_wanted
        .lock()
        .expect("system audio selection poisoned") =
        crate::platform::types::SystemAudioWanted::Everything;
    state
        .extra_mics
        .lock()
        .expect("extra mics poisoned")
        .clear();
    let wanted_handle = app.clone();
    let wanted: Box<dyn Fn() -> crate::platform::types::SystemAudioWanted + Send> =
        Box::new(move || {
            wanted_handle
                .state::<AppState>()
                .system_audio_wanted
                .lock()
                .expect("system audio selection poisoned")
                .clone()
        });
    let mics_handle = app.clone();
    let extra_mics: Box<dyn Fn() -> Vec<String> + Send> = Box::new(move || {
        mics_handle
            .state::<AppState>()
            .extra_mics
            .lock()
            .expect("extra mics poisoned")
            .clone()
    });
    let recorder = Recorder::start(
        wav_path,
        Some(audio_tx),
        mic,
        output,
        Some(level_cb),
        wanted,
        extra_mics,
    )
    .map_err(|e| e.to_string())?;

    // Store meeting ID in session's Arc so stop_recording can derive the path
    // (we store it as a thread-local state via the meeting_id field below)
    *state.recorder.lock().await = Some(recorder);
    // Share the session Arc with AppState so stop_recording can take ownership later.
    // We must NOT .take() the inner Box here â€” that would leave the audio-bridge clone
    // pointing at a None and silently drop every audio chunk.
    *state.session.lock().await = Some(session_arc);

    // Fresh recording, fresh draft mirror.
    *state.recording_drafts.lock().expect("drafts poisoned") = None;
    // Arm the silence check-in for this recording.
    state
        .last_speech_at
        .store(epoch_ms(), std::sync::atomic::Ordering::Release);
    state
        .silence_notice_at
        .store(0, std::sync::atomic::Ordering::Release);
    spawn_silence_watcher(app.clone());

    // Open the recovery scratch: which meeting is in flight (the stop path
    // reads it back), and from here on its segments, notes, and stars as
    // they arrive ([recording.md] §Crash recovery).
    crate::recovery::begin(&base, &meeting_id);

    // Emit recording-started with provider capabilities and the start
    // instant — the frontend derives elapsed time from it, so the timer
    // survives view remounts instead of restarting from a local counter.
    let started_at = epoch_ms();
    state
        .recording_started_at_ms
        .store(started_at, std::sync::atomic::Ordering::Release);
    app.emit(
        "recording-started",
        serde_json::json!({ "capabilities": capabilities, "started_at": started_at }),
    )
    .map_err(|e| e.to_string())?;

    if let Err(e) = crate::tray::update_tray_recording_state(&app, true) {
        tracing::warn!("failed to update tray icon: {e}");
    }

    // Say so: cloud was chosen, this recording is on-device. The same
    // banner a mid-recording fallback raises — silently downgrading would
    // leave the user believing they got cloud quality.
    if signed_out_cloud {
        let _ = app.emit("transcription-fallback", &AppError::CloudSignedOut);
    }

    Ok(())
}

#[tauri::command]
pub async fn pause_recording(app: AppHandle, state: State<'_, AppState>) -> Result<(), AppError> {
    if let Some(recorder) = state.recorder.lock().await.as_ref() {
        recorder.pause();
    }
    // Pausing is an answer: take any silence check-in down (the watcher
    // itself skips paused ticks).
    state
        .silence_notice_at
        .store(0, std::sync::atomic::Ordering::Release);
    let _ = app.emit("silence-cleared", ());
    if let Err(e) = crate::tray::update_tray_recording_state(&app, false) {
        tracing::warn!("failed to update tray icon: {e}");
    }
    Ok(())
}

#[tauri::command]
pub async fn resume_recording(app: AppHandle, state: State<'_, AppState>) -> Result<(), AppError> {
    if let Some(recorder) = state.recorder.lock().await.as_ref() {
        recorder.resume();
    }
    // A paused span is intentional quiet, not silence — the check-in's
    // clock restarts here.
    state
        .last_speech_at
        .store(epoch_ms(), std::sync::atomic::Ordering::Release);
    state
        .silence_notice_at
        .store(0, std::sync::atomic::Ordering::Release);
    if let Err(e) = crate::tray::update_tray_recording_state(&app, true) {
        tracing::warn!("failed to update tray icon: {e}");
    }
    Ok(())
}

/// Star the current moment of the running recording. Stars live in an
/// AppState accumulator (like segments) so every stop path — button,
/// hotkey, tray, auto-stop — persists them.
/// Star the current moment. Splits the in-flight utterance so words spoken
/// after the star start a new segment, and returns the star's timestamp on
/// the **segment timeline** (the session's stream clock — the wall clock
/// runs ahead of it by the processing backlog, and a wall-clock star would
/// sort after the very words spoken before it). Falls back to the caller's
/// wall-clock `seconds` when the session can't report a split point.
#[tauri::command]
pub async fn star_moment(state: State<'_, AppState>, seconds: f64) -> Result<f64, AppError> {
    if state.recorder.lock().await.is_none() {
        return Err(AppError::NoActiveRecording);
    }

    // Take the reply handle without holding the session locks across the
    // await (the audio bridge needs them).
    let reply = {
        let outer = state.session.lock().await;
        match outer.as_ref() {
            Some(shared) => shared
                .lock()
                .await
                .as_ref()
                .and_then(|session| session.split_utterance()),
            None => None,
        }
    };

    let mut star_secs = seconds.max(0.0);
    if let Some(rx) = reply {
        if let Ok(Ok(boundary)) =
            tokio::time::timeout(std::time::Duration::from_millis(800), rx).await
        {
            star_secs = boundary;
        }
    }

    let stars = {
        let mut stars = state.stars.lock().await;
        stars.push(star_secs);
        stars.clone()
    };
    // A star is a deliberate mark on the meeting; it survives the process
    // dying alongside the notes it belongs with.
    {
        let config = state.config.lock().await;
        let base = crate::storage::storage_base(&config.storage_dir);
        let drafts = state.recording_drafts.lock().expect("drafts poisoned").clone();
        let (notes, title) = drafts.unwrap_or_default();
        crate::recovery::write_drafts(&base, &notes, &title, &stars);
    }
    crate::telemetry::track(&state, "star_used", serde_json::json!({}));
    Ok(star_secs)
}

/// Turn speaker labeling on or off for the running recording — the
/// transcript header's toggle ([speakers.md] §Live labels).
///
/// Off strips the labels already accumulated as well as every later one,
/// so the transcript never reads as half-labelled, and it is what
/// `finalize_meeting` honors: a recording stopped with labeling off gets
/// no post-meeting speaker pass either. Turning it back on resumes
/// labeling for new segments only — the discarded ones are not restored,
/// because the post-meeting pass re-derives the whole meeting from its
/// audio anyway.
#[tauri::command]
pub async fn set_live_diarization(
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<(), AppError> {
    use std::sync::atomic::Ordering;
    if state.recorder.lock().await.is_none() {
        return Err(AppError::NoActiveRecording);
    }
    state.live_diarization.store(enabled, Ordering::Release);
    if !enabled {
        strip_speakers(&state.current_segments).await;
    }
    tracing::info!(enabled, "live diarization toggled");
    Ok(())
}

/// Remove one starred moment (a gutter-star click during the recording).
#[tauri::command]
pub async fn unstar_moment(state: State<'_, AppState>, seconds: f64) -> Result<(), AppError> {
    let mut stars = state.stars.lock().await;
    if let Some(idx) = stars.iter().position(|s| *s == seconds) {
        stars.remove(idx);
    }
    Ok(())
}

/// Record where each star sits in the user's notes — sent by the frontend
/// on `recording-stopped`, before finalize persists the stars.
#[tauri::command]
pub async fn set_star_anchors(
    state: State<'_, AppState>,
    anchors: Vec<Star>,
) -> Result<(), AppError> {
    *state.star_anchors.lock().await = anchors;
    Ok(())
}

/// Rename a live speaker label during a recording (a pill edit): rewrites
/// the accumulated segments and registers the rename for every later
/// segment of that cluster. The post-meeting pipeline keeps user-given
/// names for the clusters their segments cover (`speakers.rs`).
#[tauri::command]
pub async fn rename_live_speaker(
    state: State<'_, AppState>,
    from: String,
    to: String,
) -> Result<(), AppError> {
    let to = to.trim().to_string();
    if from.is_empty() || to.is_empty() || from == to {
        return Ok(());
    }

    {
        let mut renames = state.live_label_renames.lock().await;
        // Keep the map one-hop: renaming A→B then B→C must send future
        // A-labeled segments straight to C.
        for target in renames.values_mut() {
            if *target == from {
                *target = to.clone();
            }
        }
        renames.insert(from.clone(), to.clone());
    }

    let mut segments = state.current_segments.lock().await;
    for seg in segments.iter_mut() {
        if seg.speaker.as_deref() == Some(from.as_str()) {
            seg.speaker = Some(to.clone());
        }
    }
    Ok(())
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The silence check-in ([detection.md] §Auto-stop on silence): watches
/// the current recording for a configured stretch with no transcribed
/// word, raises "Still recording?", and acts on the setting when the
/// fixed grace runs out unanswered. One task per recording; exits with
/// the recorder. Paused spans and transcription-less recordings never
/// count as silence.
fn spawn_silence_watcher(app: AppHandle) {
    use crate::autodetect::silence::{self, Notice, Verdict};
    use std::sync::atomic::Ordering;

    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(15)).await;
            let state = app.state::<AppState>();
            let Some(paused) = state.recorder.lock().await.as_ref().map(|r| r.is_paused()) else {
                break; // the recording ended
            };
            let config = state.config.lock().await.clone();
            let threshold_secs = u64::from(config.silence_stop_minutes) * 60;
            if threshold_secs == 0 || paused || state.session.lock().await.is_none() {
                continue;
            }
            let now = epoch_ms();
            let silence_secs =
                now.saturating_sub(state.last_speech_at.load(Ordering::Acquire)) / 1000;
            let notice = match state.silence_notice_at.load(Ordering::Acquire) {
                0 => Notice::None,
                u64::MAX => Notice::StoodDown,
                at => Notice::Pending { age_secs: now.saturating_sub(at) / 1000 },
            };
            match silence::check(silence_secs, threshold_secs, notice) {
                Verdict::Quiet | Verdict::Waiting => {}
                Verdict::Notify => {
                    state.silence_notice_at.store(now, Ordering::Release);
                    tracing::info!(
                        minutes = config.silence_stop_minutes,
                        "silence check-in raised"
                    );
                    let _ = app.emit(
                        "silence-notice",
                        serde_json::json!({ "minutes": config.silence_stop_minutes }),
                    );
                }
                Verdict::Cleared => {
                    state.silence_notice_at.store(0, Ordering::Release);
                    let _ = app.emit("silence-cleared", ());
                }
                Verdict::Unanswered => {
                    let _ = app.emit("silence-cleared", ());
                    match config.silence_stop_unanswered {
                        embral_types::SilenceUnanswered::Stop => {
                            state.silence_notice_at.store(0, Ordering::Release);
                            tracing::info!("silence check-in unanswered — stopping the recording");
                            request_stop(&app);
                        }
                        embral_types::SilenceUnanswered::Keep => {
                            state.silence_notice_at.store(u64::MAX, Ordering::Release);
                            tracing::info!("silence check-in unanswered — recording continues");
                        }
                    }
                }
            }
        }
    });
}

/// The frontend's notes/title drafts, mirrored into the backend while
/// recording (debounced). A stop that arrives without them — the handshake
/// fallback when the frontend never answers — substitutes this mirror, so
/// the human's words survive every stop path.
#[tauri::command]
pub async fn sync_recording_drafts(
    notes: String,
    meeting_title: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    // Onto disk as well: the mirror below survives a stop the frontend
    // never answers, but only the scratch survives the process dying.
    // Debounced by the caller, so this is not a per-keystroke write.
    let base = crate::storage::storage_base(&state.config.lock().await.storage_dir);
    let stars = state.stars.lock().await.clone();
    crate::recovery::write_drafts(&base, &notes, &meeting_title, &stars);
    *state.recording_drafts.lock().expect("drafts poisoned") = Some((notes, meeting_title));
    Ok(())
}

/// The backend's answer to "what is actually happening right now" — the
/// frontend reconciles against it on mount and on window focus, because a
/// hidden webview gets throttled and can drop the events it would
/// otherwise have built this state from (the auto-start-while-hidden bug).
#[derive(serde::Serialize)]
pub struct RecordingStatus {
    pub recording: bool,
    pub paused: bool,
    pub started_at_ms: u64,
    pub labels_authoritative: bool,
    /// This recording's live diarization standing, so a window that missed
    /// the toggle (or the runaway guard) shows the right button state.
    pub diarization: bool,
    pub segments: Vec<embral_types::TranscriptionSegment>,
    /// The picker's current choices, so a reopened window shows them
    /// checked rather than snapping back to the defaults.
    pub selected_apps: Option<Vec<u32>>,
    pub extra_mics: Vec<String>,
}

#[tauri::command]
pub async fn recording_status(state: State<'_, AppState>) -> Result<RecordingStatus, AppError> {
    use std::sync::atomic::Ordering;
    let (recording, paused) = match state.recorder.lock().await.as_ref() {
        Some(r) => (true, r.is_paused()),
        None => (false, false),
    };
    Ok(RecordingStatus {
        recording,
        paused,
        started_at_ms: state.recording_started_at_ms.load(Ordering::Acquire),
        labels_authoritative: state.labels_authoritative.load(Ordering::Acquire),
        diarization: state.live_diarization.load(Ordering::Acquire),
        segments: if recording {
            state.current_segments.lock().await.clone()
        } else {
            Vec::new()
        },
        selected_apps: match &*state
            .system_audio_wanted
            .lock()
            .expect("system audio selection poisoned")
        {
            crate::platform::types::SystemAudioWanted::Everything => None,
            crate::platform::types::SystemAudioWanted::Apps(pids) => Some(pids.clone()),
        },
        extra_mics: state.extra_mics.lock().expect("extra mics poisoned").clone(),
    })
}

/// The source picker's system-audio choice. `None` (nothing unchecked) is
/// everything the machine plays — the default, and the lane that needs no
/// per-app capture. A list narrows the recording to those apps' own audio.
#[tauri::command]
pub async fn set_system_audio_sources(
    apps: Option<Vec<u32>>,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let wanted = match apps {
        None => crate::platform::types::SystemAudioWanted::Everything,
        Some(pids) => crate::platform::types::SystemAudioWanted::Apps(pids),
    };
    tracing::info!(?wanted, "system audio selection changed");
    *state
        .system_audio_wanted
        .lock()
        .expect("system audio selection poisoned") = wanted;
    // Apply now rather than on the lane's next tick.
    if let Some(recorder) = state.recorder.lock().await.as_ref() {
        recorder.reconfigure_sources();
    }
    Ok(())
}

/// The source picker's extra microphones (beyond the recording's primary
/// mic, which owns the master clock and cannot be removed mid-recording).
#[tauri::command]
pub async fn set_extra_mics(
    devices: Vec<String>,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    tracing::info!(?devices, "extra microphones changed");
    *state.extra_mics.lock().expect("extra mics poisoned") = devices;
    if let Some(recorder) = state.recorder.lock().await.as_ref() {
        recorder.reconfigure_mics();
    }
    Ok(())
}

/// A stop for surfaces that hold no drafts (the notice window): route
/// through the handshake like every backend-initiated stop.
#[tauri::command]
pub async fn request_stop_recording(app: AppHandle) -> Result<(), AppError> {
    request_stop(&app);
    Ok(())
}

/// The check-in's "Keep recording" answer: a fresh full silence window.
#[tauri::command]
pub async fn silence_keep_recording(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    use std::sync::atomic::Ordering;
    state.last_speech_at.store(epoch_ms(), Ordering::Release);
    state.silence_notice_at.store(0, Ordering::Release);
    app.emit("silence-cleared", ()).map_err(|e| e.to_string())?;
    Ok(())
}

/// Finish a recording the last run never stopped ([recording.md] §Crash
/// recovery). Called once from `setup()`; it takes the scratch (so a
/// second launch cannot try again), then runs the ordinary finalize
/// pipeline in the background — the recovered meeting is just a meeting.
///
/// Silent by design. Approving your own recording is a chore, and after a
/// crash the user may not remember there was one; the threshold in
/// `recovery::worth_recovering` is what keeps two-second orphans out of
/// the list instead of a prompt.
pub fn recover_interrupted_recording(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let config = state.config.lock().await.clone();
        let base = crate::storage::storage_base(&config.storage_dir);
        let audio_dir = base.join("audio");
        let Some(found) = crate::recovery::take(&base, |id| audio_dir.join(format!("{id}.wav")))
        else {
            return;
        };
        let db = match state.db().await {
            Ok(db) => db,
            Err(e) => {
                tracing::warn!("cannot recover the interrupted recording: {e}");
                return;
            }
        };
        let wav_path = audio_dir.join(format!("{}.wav", found.meeting_id));
        let started_at = meeting_start_time(&found.meeting_id);
        // Labels are never authoritative here: a cloud session that died
        // mid-recording left partial diarization at best, so the finalize
        // pipeline re-derives speakers from the audio it has.
        finalize_meeting(
            app.clone(),
            db,
            base,
            config,
            found.meeting_id,
            started_at,
            found.segments,
            AudioSource::Wav(wav_path),
            false,
            found.stars,
            found.user_notes,
            found.user_title,
        )
        .await;
    });
}

/// Ask the frontend to perform the stop, so the notes draft and title
/// travel with it exactly like a stop from the button — a direct backend
/// stop would finalize the meeting without them. The webview runs even
/// while the window is hidden; the timed fallback covers a wedged
/// frontend, not a normal path.
pub fn request_stop(app: &AppHandle) {
    let _ = app.emit("stop-requested", ());
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        let still_recording = app.state::<AppState>().recorder.lock().await.is_some();
        if still_recording {
            tracing::warn!("frontend did not answer stop-requested; stopping without the drafts");
            if let Err(e) = stop_recording(app.clone(), app.state(), None, None).await {
                tracing::warn!("fallback stop failed: {e}");
            }
        }
    });
}

/// Time we're willing to block in the post-stop pipeline waiting for the
/// transcription session to finalize tail audio. A streaming provider can
/// keep its socket open for ~60s post-stop with empty heartbeats while
/// internally processing a backlog — but no NEW tokens arrive during that
/// window. Waiting past ~5–10s buys nothing.
const SESSION_FINISH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

#[tauri::command]
pub async fn stop_recording(
    app: AppHandle,
    state: State<'_, AppState>,
    user_notes: Option<String>,
    meeting_title: Option<String>,
) -> Result<(), AppError> {
    let mut config = state.config.lock().await.clone();
    // This recording's standing wins over the setting: labeling turned off
    // mid-meeting (by the toggle or the runaway guard) must not come back
    // as a post-meeting speaker pass over the same audio.
    config.diarization_enabled = state
        .live_diarization
        .load(std::sync::atomic::Ordering::Acquire);
    let base = crate::storage::storage_base(&config.storage_dir);
    let db = state.db().await?;
    // Absent args mean a stop the frontend never answered (the handshake
    // fallback) — the mirrored drafts stand in. A frontend stop always
    // sends its strings, empty included, so `None` is never "cleared".
    let (user_notes, meeting_title) = if user_notes.is_none() && meeting_title.is_none() {
        let mirrored = state.recording_drafts.lock().expect("drafts poisoned").clone();
        match mirrored {
            Some((notes, title)) => (Some(notes), Some(title)),
            None => (None, None),
        }
    } else {
        (user_notes, meeting_title)
    };
    let user_title = meeting_title
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // Which meeting is in flight, from the recovery scratch. It is *not*
    // cleared here: finalize still has the slow part ahead of it (speaker
    // pipeline, LLM refinement), and a crash in there must still leave the
    // next launch something to re-run from. The background task below
    // clears it once the meeting is committed.
    let meeting_id =
        crate::recovery::active_meeting_id(&base).ok_or_else(|| "No active recording".to_string())?;

    // --- Foreground (fast path): stop recorder, hand off to background. ---
    let recorder = state
        .recorder
        .lock()
        .await
        .take()
        .ok_or_else(|| AppError::internal("No active recorder"))?;
    let wav_path = recorder.stop().map_err(AppError::internal)?;

    let session_arc = state
        .session
        .lock()
        .await
        .take()
        .ok_or_else(|| AppError::internal("No active session"))?;

    let segments_acc = state.current_segments.clone();

    // Whether this session's provider labels are final (snapshotted at start).
    let labels_authoritative = state
        .labels_authoritative
        .load(std::sync::atomic::Ordering::Acquire);

    // Starred moments accumulated during the recording (any stop path —
    // button, hotkey, tray, auto-stop — picks them up here). Their notes
    // anchors arrive from the frontend just after `recording-stopped`
    // fires, so the merge happens in the background task below.
    let star_seconds = std::mem::take(&mut *state.stars.lock().await);

    // Any stop (manual, hotkey, or auto) ends the auto-started tracking;
    // the swapped-out value feeds the telemetry event below.
    let auto_started = state
        .auto_started
        .swap(false, std::sync::atomic::Ordering::AcqRel);

    // Tell the UI we're done recording â€” it transitions to the processing view
    // (which renders the checklist). Everything below runs detached.
    app.emit("recording-stopped", ())
        .map_err(|e| e.to_string())?;
    if let Err(e) = crate::tray::update_tray_recording_state(&app, false) {
        tracing::warn!("failed to update tray icon: {e}");
    }

    // --- Background: bounded finish, encode, refine, write notes. ---
    let app_bg = app.clone();
    tokio::spawn(async move {
        // 1. Wait briefly for the transcription session to finalize tail audio.
        //    Source of truth for segments is `segments_acc`, populated by the
        //    event forwarder during recording; we don't use finish()'s return.
        if let Some(session) = session_arc.lock().await.take() {
            match tokio::time::timeout(SESSION_FINISH_TIMEOUT, session.finish()).await {
                Ok(Ok(_)) => {
                    tracing::info!("Transcription session finished cleanly");
                }
                Ok(Err(e)) => {
                    tracing::warn!("Transcription session finish errored: {}", e);
                }
                Err(_) => {
                    tracing::warn!(
                        "Transcription session finish timed out after {:?} â€” using segments accumulated so far",
                        SESSION_FINISH_TIMEOUT
                    );
                }
            }
        }

        // Snapshot accumulated segments and hand off to the shared pipeline.
        let segments = segments_acc.lock().await.clone();
        let started_at = meeting_start_time(&meeting_id);

        // The *configured* provider: neither the power policy's per-meeting
        // choice nor a mid-recording cloud→local fallback shows here — the
        // latter is error{transcription_failed} ([telemetry.md]).
        let duration_secs = segments.last().map(|s| s.end as u64).unwrap_or_else(|| {
            chrono::Utc::now()
                .signed_duration_since(started_at)
                .num_seconds()
                .max(0) as u64
        });
        crate::telemetry::track(
            &app_bg.state::<AppState>(),
            "meeting_recorded",
            serde_json::json!({
                "provider": config.transcription_provider,
                "duration_bucket": crate::telemetry::meeting_bucket(duration_secs),
                "auto_started": auto_started,
            }),
        );

        // Attach the notes anchors the frontend sent at stop (matched by
        // the exact timestamp; a missing anchor just means no notes line).
        let anchors =
            std::mem::take(&mut *app_bg.state::<AppState>().star_anchors.lock().await);
        let stars: Vec<Star> = star_seconds
            .into_iter()
            .map(|seconds| Star {
                seconds,
                note_block: anchors
                    .iter()
                    .find(|a| a.seconds == seconds)
                    .and_then(|a| a.note_block),
            })
            .collect();
        finalize_meeting(
            app_bg,
            db,
            base.clone(),
            config,
            meeting_id,
            started_at,
            segments,
            AudioSource::Wav(wav_path),
            labels_authoritative,
            stars,
            user_notes,
            user_title,
        )
        .await;
        // The meeting is committed (or its save failed and said so): the
        // scratch has nothing left to protect. Until this line, a crash
        // anywhere in finalize is recoverable at the next launch.
        crate::recovery::clear(&base);
    });

    Ok(())
}

/// Accept the "call detected" prompt: start recording and mark it
/// auto-started so it also auto-stops when the call ends.
#[tauri::command]
pub async fn accept_detected_meeting(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state
        .auto_started
        .store(true, std::sync::atomic::Ordering::Release);
    crate::telemetry::track(
        &state,
        "detection_response",
        serde_json::json!({ "action": "accepted" }),
    );
    let result = start_recording(app.clone(), app.state()).await;
    if result.is_err() {
        state
            .auto_started
            .store(false, std::sync::atomic::Ordering::Release);
    }
    result
}

/// Dismiss the "call detected" prompt for the rest of the current call.
/// Emits `meeting-dismissed` so both prompt surfaces (in-app banner,
/// notice window) come down together whichever one answered.
#[tauri::command]
pub async fn dismiss_detected_meeting(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state
        .detection_dismissed
        .store(true, std::sync::atomic::Ordering::Release);
    crate::telemetry::track(
        &state,
        "detection_response",
        serde_json::json!({ "action": "dismissed" }),
    );
    let _ = app.emit("meeting-dismissed", ());
    Ok(())
}

#[cfg(test)]
mod diarization_tests {
    use super::*;

    #[test]
    fn a_plausible_meeting_keeps_its_labels() {
        // Six people round a table is a meeting, not a malfunction.
        for distinct in 1..=MAX_LIVE_SPEAKERS {
            assert!(!diarization_has_run_away(distinct), "{distinct} speakers");
        }
    }

    #[test]
    fn one_speaker_too_many_stands_the_labels_down() {
        // The real failure is one voice splitting into a crowd, not a
        // genuinely large meeting: past the ceiling the labels have
        // stopped being evidence of anything.
        assert!(diarization_has_run_away(MAX_LIVE_SPEAKERS + 1));
        assert!(diarization_has_run_away(40));
    }
}
