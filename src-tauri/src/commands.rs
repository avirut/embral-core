use embral_db::MeetingRow;
use embral_types::{AppConfig, MeetingRecord, MeetingSummary};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::Mutex;

use crate::audio::{encoder, recorder::Recorder};
use crate::transcription::{self, TranscriptionEvent, TranscriptionSession};
use crate::AppState;
use embral_notes::transcript::format_transcript;

/// Whether the configured local model is on disk — the gate for falling
/// back from cloud transcription mid-recording.
#[cfg(feature = "cloud")]
fn local_model_present(config: &AppConfig) -> bool {
    embral_engine::catalog::find(&config.meeting_asr_model()).is_some_and(|m| m.present())
}

#[cfg(feature = "cloud")]
use crate::transcription::TranscriptionProvider;

#[derive(serde::Serialize)]
pub struct MeetingDetail {
    pub record: MeetingRecord,
    pub notes_markdown: String,
    pub transcript_markdown: String,
    pub audio_path: Option<String>,
    pub audio_exists: bool,
    pub attendees: Vec<String>,
    /// Structured transcript; empty for legacy meetings that only have
    /// markdown (the UI falls back to the raw editor then).
    pub segments: Vec<embral_types::TranscriptionSegment>,
    /// Pending "Speaker N looks like X" suggestions from the user's notes.
    pub name_suggestions: Vec<crate::speaker_commands::NameSuggestionView>,
    /// User-starred moments (empty when none).
    pub stars: Vec<Star>,
    /// The user's raw live notes, verbatim (the Notes tab).
    pub user_notes: String,
}

/// One starred moment: when it happened, and (when the notes editor was
/// mounted at stop) which top-level block of the user's notes it sits on.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Star {
    pub seconds: f64,
    pub note_block: Option<u32>,
}

/// Also used by the audio janitor in `storage.rs`.
pub(crate) fn resolve_indexed_path(base: &Path, indexed_path: &str) -> Result<PathBuf, String> {
    let path = Path::new(indexed_path);
    if path.is_absolute() {
        return Err("Indexed meeting path must be relative".to_string());
    }
    if path.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err("Indexed meeting path escapes the storage directory".to_string());
    }
    Ok(base.join(path))
}

fn strip_frontmatter(markdown: &str) -> &str {
    if !markdown.starts_with("---") {
        return markdown;
    }
    let Some(end) = markdown.find("\n---") else {
        return markdown;
    };
    let closing_end = markdown[end + 4..]
        .find('\n')
        .map(|offset| end + 4 + offset + 1)
        .unwrap_or(markdown.len());
    markdown[closing_end..].trim_start()
}

fn frontmatter_value(markdown: &str, key: &str) -> Option<String> {
    if !markdown.starts_with("---") {
        return None;
    }
    let end = markdown.find("\n---")?;
    let block = &markdown[3..end];
    for line in block.lines() {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim() == key {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

fn parse_attendees_value(value: &str) -> Vec<String> {
    if let Ok(names) = serde_json::from_str::<Vec<String>>(value) {
        return normalize_attendees(names);
    }

    let trimmed = value.trim().trim_start_matches('[').trim_end_matches(']');
    if trimmed.is_empty() {
        return Vec::new();
    }
    normalize_attendees(trimmed.split(',').map(ToString::to_string).collect())
}

/// Also used by the legacy index.json import in `storage.rs`.
pub(crate) fn parse_attendees(markdown: &str) -> Vec<String> {
    frontmatter_value(markdown, "attendees")
        .map(|value| parse_attendees_value(&value))
        .unwrap_or_default()
}

fn normalize_attendees(attendees: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for attendee in attendees {
        let attendee = attendee.trim().trim_matches('"').to_string();
        if !attendee.is_empty() && !out.iter().any(|existing| existing == &attendee) {
            out.push(attendee);
        }
    }
    out
}

fn canonical_frontmatter(
    start_time: &str,
    duration_minutes: u32,
    meeting_id: &str,
    attendees: &[String],
) -> String {
    let attendees = serde_json::to_string(attendees).unwrap_or_else(|_| "[]".to_string());
    format!(
        "---\nstart_time: {}\nduration_minutes: {}\nmeeting_id: {}\nattendees: {}\n---\n",
        start_time, duration_minutes, meeting_id, attendees
    )
}

fn prepend_frontmatter(markdown: &str, frontmatter: &str) -> String {
    format!(
        "{}\n{}",
        frontmatter.trim_end(),
        strip_frontmatter(markdown)
    )
}

fn attendees_from_segments(segments: &[embral_types::TranscriptionSegment]) -> Vec<String> {
    let mut speakers = Vec::new();
    for segment in segments {
        if let Some(speaker) = segment.speaker.as_deref() {
            let speaker = speaker.trim();
            if !speaker.is_empty() && !speakers.iter().any(|s| s == speaker) {
                speakers.push(speaker.to_string());
            }
        }
    }
    speakers
}

pub(crate) fn fallback_duration_minutes(record: &MeetingRecord) -> u32 {
    ((record.duration_seconds as f64 / 60.0).ceil() as u32).max(1)
}

fn canonicalize_start_time(value: &str) -> Option<String> {
    chrono::DateTime::parse_from_rfc3339(value).ok().map(|dt| {
        dt.with_timezone(&chrono::Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    })
}

fn canonicalize_frontmatter(
    markdown: &str,
    record: &MeetingRecord,
    attendees: &[String],
) -> String {
    let start_time = frontmatter_value(markdown, "start_time")
        .and_then(|value| canonicalize_start_time(&value))
        .unwrap_or_else(|| {
            record
                .date
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        });
    let duration_minutes = frontmatter_value(markdown, "duration_minutes")
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or_else(|| fallback_duration_minutes(record));
    let meeting_id = frontmatter_value(markdown, "meeting_id").unwrap_or_else(|| record.id.clone());
    let frontmatter = canonical_frontmatter(&start_time, duration_minutes, &meeting_id, attendees);
    prepend_frontmatter(markdown, &frontmatter)
}

pub(crate) fn write_indexed_text(base: &Path, indexed_path: &str, text: &str) -> Result<(), String> {
    let path = resolve_indexed_path(base, indexed_path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, text).map_err(|e| e.to_string())
}

fn remove_indexed_file(base: &Path, indexed_path: &str) -> Result<(), String> {
    if indexed_path.trim().is_empty() {
        return Ok(());
    }
    let path = resolve_indexed_path(base, indexed_path)?;
    if path.is_file() {
        std::fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn rename_indexed_file(base: &Path, old_path: &str, new_path: &str) -> Result<(), String> {
    if old_path.trim().is_empty() || old_path == new_path {
        return Ok(());
    }
    let old = resolve_indexed_path(base, old_path)?;
    let new = resolve_indexed_path(base, new_path)?;
    if let Some(parent) = new.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    if old.is_file() {
        std::fs::rename(old, new).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Fetch a meeting row or produce the standard not-found message.
pub(crate) fn require_row(db: &embral_db::Db, id: &str) -> Result<MeetingRow, String> {
    db.get_meeting(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Meeting {} not found", id))
}

/// Build the frontend detail payload from a DB row. Markdown comes straight
/// from the database; only the audio file is checked on disk.
pub(crate) fn meeting_detail(
    db: &embral_db::Db,
    base: &Path,
    row: MeetingRow,
) -> Result<MeetingDetail, String> {
    let audio_path_value = row.audio_path.trim();
    let audio_file_path = if audio_path_value.is_empty() {
        None
    } else {
        Some(resolve_indexed_path(base, audio_path_value)?)
    };
    let audio_exists = audio_file_path.as_ref().is_some_and(|path| path.is_file());
    let audio_path = if audio_exists {
        audio_file_path.map(|path| path.to_string_lossy().to_string())
    } else {
        None
    };

    let segments = db.get_segments(&row.id).map_err(|e| e.to_string())?;
    let name_suggestions =
        crate::speaker_commands::name_suggestion_views(db, &row.id).unwrap_or_default();
    let stars = db
        .get_stars(&row.id)
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();
    let user_notes = db.get_user_notes(&row.id).unwrap_or_default();

    Ok(MeetingDetail {
        record: row.to_record(),
        notes_markdown: row.notes_md,
        transcript_markdown: row.transcript_md,
        audio_path,
        audio_exists,
        attendees: row.attendees,
        segments,
        name_suggestions,
        stars,
        user_notes,
    })
}

fn meeting_timestamp_prefix(record: &MeetingRecord) -> String {
    record
        .id
        .get(..13.min(record.id.len()))
        .unwrap_or(&record.id)
        .to_string()
}

fn meeting_start_time(meeting_id: &str) -> chrono::DateTime<chrono::Utc> {
    let prefix = meeting_id
        .split_once('_')
        .map(|(prefix, _)| prefix)
        .unwrap_or(meeting_id);
    chrono::NaiveDateTime::parse_from_str(prefix, "%y%m%dT%H%M%S")
        .map(|dt| chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(dt, chrono::Utc))
        .unwrap_or_else(|_| chrono::Utc::now())
}

fn normalize_title(title: String) -> Result<String, String> {
    let title = title.trim().to_string();
    if title.is_empty() {
        Err("Meeting title cannot be empty".to_string())
    } else {
        Ok(title)
    }
}

fn transcript_frontmatter(
    meeting_id: &str,
    start_time: &str,
    duration_minutes: u32,
    attendees: &[String],
) -> String {
    canonical_frontmatter(start_time, duration_minutes, meeting_id, attendees)
}

pub(crate) fn format_transcript_document(
    title: &str,
    meeting_id: &str,
    start_time: &str,
    duration_minutes: u32,
    attendees: &[String],
    transcript_text: &str,
) -> String {
    let heading = if title.trim().is_empty() {
        "Transcript".to_string()
    } else {
        format!("{} Transcript", title.trim())
    };
    let transcript_body = if transcript_text.trim().is_empty() {
        "_No transcript segments were captured._"
    } else {
        transcript_text.trim()
    };

    format!(
        "{}\n# {}\n\n{}",
        transcript_frontmatter(meeting_id, start_time, duration_minutes, attendees),
        heading,
        transcript_body
    )
}

/// One choke point for every start path (button, hotkey, detection accept,
/// auto-start): a refused start counts once, whatever refused it.
#[tauri::command]
pub async fn start_recording(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
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

async fn start_recording_inner(app: AppHandle, state: &State<'_, AppState>) -> Result<(), String> {
    let config = state.config.lock().await.clone();

    if !crate::config::is_configured(&config) {
        return Err("Transcription isn't set up yet — download the speech model or sign in from Settings.".to_string());
    }
    if state.dictating.load(std::sync::atomic::Ordering::Acquire) {
        return Err("Can't record during a dictation — finish it first.".to_string());
    }

    let base = crate::storage::storage_base(&config.storage_dir);
    crate::storage::init_storage_dirs(&base).map_err(|e| e.to_string())?;

    let meeting_id = crate::storage::generate_meeting_id();
    let wav_path = base.join("audio").join(format!("{}.wav", meeting_id));

    // Reset the backend-side segment accumulator. The event forwarder below
    // populates it; stop_recording reads from it as source of truth.
    state.current_segments.lock().await.clear();
    state.live_label_renames.lock().await.clear();
    state.stars.lock().await.clear();
    state.star_anchors.lock().await.clear();
    let segments_acc = state.current_segments.clone();

    // Build transcription provider and open session
    let provider = transcription::build_provider(&config, state.engine.clone());
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
        Err(e) if config.transcription_provider == embral_types::TranscriptionProvider::Cloud => {
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
                        serde_json::json!({ "message": e.to_string() }),
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
                            serde_json::json!({ "message": "embral cloud is unreachable" }),
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
                    segments_acc_for_forwarder.lock().await.push(seg.clone());
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
                                    serde_json::json!({ "message": message }),
                                );
                                break;
                            }
                            crate::config::CloudFailureAction::Fail => {
                                tracing::error!(
                                    "cloud transcription failed with no local model to fall back to: {message}"
                                );
                                let _ = app_clone.emit(
                                    "transcription-failed",
                                    serde_json::json!({ "message": message }),
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
                                    serde_json::json!({ "message": message }),
                                );
                            }
                            Err(e) => {
                                tracing::error!("local fallback failed to start: {e}");
                                let _ = app_clone.emit(
                                    "transcription-failed",
                                    serde_json::json!({ "message": message }),
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
                            serde_json::json!({ "message": message }),
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
    let recorder = Recorder::start(wav_path, Some(audio_tx), mic, output, Some(level_cb))
        .map_err(|e| e.to_string())?;

    // Store meeting ID in session's Arc so stop_recording can derive the path
    // (we store it as a thread-local state via the meeting_id field below)
    *state.recorder.lock().await = Some(recorder);
    // Share the session Arc with AppState so stop_recording can take ownership later.
    // We must NOT .take() the inner Box here â€” that would leave the audio-bridge clone
    // pointing at a None and silently drop every audio chunk.
    *state.session.lock().await = Some(session_arc);

    // Store meeting_id in config state so stop_recording knows which meeting is active
    // We reuse the config slot as a minimal side-channel â€” cleaner than adding another field
    // Actually, store meeting_id in a dedicated way by embedding it in AppState.
    // For now, encode it as a special key in storage: we store the in-progress meeting ID
    // in a temp file at base/in_progress.txt
    {
        let tmp = base.join("in_progress.txt");
        let _ = std::fs::write(&tmp, &meeting_id);
    }

    // Emit recording-started with provider capabilities and the start
    // instant — the frontend derives elapsed time from it, so the timer
    // survives view remounts instead of restarting from a local counter.
    let started_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    app.emit(
        "recording-started",
        serde_json::json!({ "capabilities": capabilities, "started_at": started_at }),
    )
    .map_err(|e| e.to_string())?;

    if let Err(e) = crate::tray::update_tray_recording_state(&app, true) {
        tracing::warn!("failed to update tray icon: {e}");
    }

    Ok(())
}

#[tauri::command]
pub async fn pause_recording(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if let Some(recorder) = state.recorder.lock().await.as_ref() {
        recorder.pause();
    }
    if let Err(e) = crate::tray::update_tray_recording_state(&app, false) {
        tracing::warn!("failed to update tray icon: {e}");
    }
    Ok(())
}

#[tauri::command]
pub async fn resume_recording(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    if let Some(recorder) = state.recorder.lock().await.as_ref() {
        recorder.resume();
    }
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
pub async fn star_moment(state: State<'_, AppState>, seconds: f64) -> Result<f64, String> {
    if state.recorder.lock().await.is_none() {
        return Err("No active recording".to_string());
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

    state.stars.lock().await.push(star_secs);
    crate::telemetry::track(&state, "star_used", serde_json::json!({}));
    Ok(star_secs)
}

/// Remove one starred moment (a gutter-star click during the recording).
#[tauri::command]
pub async fn unstar_moment(state: State<'_, AppState>, seconds: f64) -> Result<(), String> {
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
) -> Result<(), String> {
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
) -> Result<(), String> {
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

/// Time we're willing to block in the post-stop pipeline waiting for the
/// transcription session to finalize tail audio. A streaming provider can
/// keep its socket open for ~60s post-stop with empty heartbeats while
/// internally processing a backlog — but no NEW tokens arrive during that
/// window. Waiting past ~5–10s buys nothing.
const SESSION_FINISH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

/// Where a finished meeting's audio comes from.
pub(crate) enum AudioSource {
    /// A recorder-written WAV: encoded to MP3, then the WAV is deleted
    /// (encode-then-delete even when audio isn't retained â€” unchanged
    /// pre-refactor behavior for live recordings).
    Wav(PathBuf),
    /// Decoded PCM from an import: encoded to MP3 only when audio is retained.
    /// Arc'd so the speaker pipeline can share it without copying an hour of
    /// PCM.
    Samples(Arc<Vec<f32>>),
}

/// Everything between "we have the finalized segments" and "the meeting is
/// saved and announced": MP3 encode, LLM refinement, markdown + DB writes,
/// index export, integrations, and the completion event. Shared by
/// `stop_recording` (live) and `import_recording` (files); behavior for the
/// live path is unchanged by the extraction.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn finalize_meeting(
    app: AppHandle,
    db: Arc<embral_db::Db>,
    base: PathBuf,
    config: AppConfig,
    meeting_id: String,
    started_at: chrono::DateTime<chrono::Utc>,
    mut segments: Vec<embral_types::TranscriptionSegment>,
    audio: AudioSource,
    labels_authoritative: bool,
    // User-starred moments (with their notes anchors when known).
    stars: Vec<Star>,
    user_notes: Option<String>,
    user_title: Option<String>,
) {
    // --- Speaker pipeline (before formatting, so names reach the transcript,
    // the notes LLM, and the attendee list). Authoritative provider labels
    // (cloud live diarization) are kept; the local provider's provisional
    // live labels are re-derived here from the full recording, which the
    // pipeline overwrites. A missing model or any failure degrades to the
    // labels we already have.
    let engine = app.state::<AppState>().engine.clone();
    if config.diarization_enabled
        && engine.speaker_id_present()
        && !segments.is_empty()
        && !(labels_authoritative && segments.iter().any(|s| s.speaker.is_some()))
    {
        let samples: Option<Arc<Vec<f32>>> = match &audio {
            AudioSource::Wav(p) => match crate::speakers::read_wav_16k(p) {
                Ok(s) => Some(Arc::new(s)),
                Err(e) => {
                    tracing::warn!("could not read recording for diarization: {e}");
                    None
                }
            },
            AudioSource::Samples(s) => Some(s.clone()),
        };
        if let Some(samples) = samples {
            let db2 = db.clone();
            let config2 = config.clone();
            let engine2 = engine.clone();
            let mut segs = segments.clone();
            let outcome = tokio::task::spawn_blocking(move || {
                let labeled = crate::speakers::run(&engine2, &db2, &config2, &samples, &mut segs);
                (segs, labeled)
            })
            .await;
            match outcome {
                Ok((segs, Ok(()))) => segments = segs,
                Ok((_, Err(e))) => tracing::warn!("speaker pipeline failed: {e}"),
                Err(e) => tracing::error!("speaker pipeline panicked: {e}"),
            }
        }
    }

    // --- Name speakers from the user's typed notes ([speakers.md]) — before
    // formatting for the same reason as the pipeline above. Automatic mode
    // renames segments here; suggest mode returns pending suggestions that
    // persist below and surface in the meeting view.
    let name_suggestions = {
        let state = app.state::<AppState>();
        crate::notes_matching::run(
            &state.search,
            &state.llm,
            &db,
            &config,
            user_notes.as_deref().unwrap_or(""),
            &mut segments,
        )
        .await
    };
    let name_suggestions_json =
        serde_json::to_string(&name_suggestions).unwrap_or_else(|_| "[]".into());

    let transcript_text = format_transcript(&segments);
    let _ = app.emit("transcription-final-complete", &transcript_text);

    // Encode MP3 (non-fatal on failure; we can still write notes).
    let mp3_path = base.join("audio").join(format!("{}.mp3", meeting_id));
    match audio {
        AudioSource::Wav(wav_path) => {
            match encoder::encode_wav_to_mp3(&wav_path, &mp3_path) {
                Ok(()) => {
                    // Audio is playable well before the notes finish — let
                    // the pending meeting mount its player now. (The file
                    // is renamed at persist time; the completed detail
                    // brings the final path.)
                    let _ = app.emit(
                        "pending-audio-ready",
                        mp3_path.to_string_lossy().to_string(),
                    );
                }
                Err(e) => {
                    tracing::error!("MP3 encode failed: {}", e);
                    let _ = app.emit("processing-error", format!("encode failed: {}", e));
                    crate::telemetry::track(
                        &app.state::<AppState>(),
                        "error",
                        serde_json::json!({ "category": "encode_failed" }),
                    );
                }
            }
            let _ = std::fs::remove_file(&wav_path);
        }
        AudioSource::Samples(samples) => {
            if config.retain_audio {
                if let Err(e) = encoder::encode_samples_to_mp3(samples.as_slice(), 16_000, &mp3_path) {
                    tracing::error!("MP3 encode failed: {}", e);
                    let _ = app.emit("processing-error", format!("encode failed: {}", e));
                    crate::telemetry::track(
                        &app.state::<AppState>(),
                        "error",
                        serde_json::json!({ "category": "encode_failed" }),
                    );
                }
            }
        }
    }

    // LLM refinement.
    let _ = app.emit("notes-generation-started", ());

    // No segments — transcription was disabled or produced nothing. The
    // wall clock is the only duration signal left.
    let duration_minutes = segments
        .last()
        .map(|s| (s.end / 60.0).ceil() as u32)
        .unwrap_or_else(|| {
            (chrono::Utc::now().signed_duration_since(started_at).num_seconds() as f64 / 60.0)
                .ceil() as u32
        })
        .max(1);

    let start_time = started_at.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    let fallback_attendees = attendees_from_segments(&segments);

    // `None` = this meeting has no summary: either summaries are off, or the
    // engine failed, or there is nothing to summarize. Nothing fake is
    // written in its place — a "summary" that is a copy of the transcript
    // (or, on an empty one, an invention) is worse than no summary at all.
    let summary_md: Option<String> = match crate::refinement::summaries_profile(&config)
        .filter(|_| !segments.is_empty())
    {
        Some(profile) => {
            let sidecar = &app.state::<AppState>().llm;
            let generated = match crate::llm::resolved_notes_config(sidecar, &config, &profile).await {
                Ok(notes_cfg) => {
                    crate::refinement::refine_notes(
                        &notes_cfg,
                        &config,
                        &meeting_id,
                        &start_time,
                        duration_minutes,
                        user_title.as_deref(),
                        &transcript_text,
                        user_notes.as_deref(),
                    )
                    .await
                }
                Err(e) => Err(e),
            };
            sidecar.touch();
            match &generated {
                Ok(_) => {
                    // "cloud" is CLOUD_PROFILE_ID, spelled out because the
                    // constant is cfg-gated to the cloud edition.
                    let engine = match profile.id.as_str() {
                        "" | embral_types::BUILTIN_PROFILE_ID => "builtin",
                        "cloud" => "cloud",
                        _ => "custom",
                    };
                    crate::telemetry::track(
                        &app.state::<AppState>(),
                        "notes_generated",
                        serde_json::json!({ "engine": engine }),
                    );
                }
                Err(e) => {
                    tracing::error!("LLM refinement failed: {e}");
                    crate::telemetry::track(
                        &app.state::<AppState>(),
                        "error",
                        serde_json::json!({ "category": "notes_failed" }),
                    );
                }
            }
            generated.ok()
        }
        None => {
            if segments.is_empty() {
                tracing::info!("no transcript — nothing to summarize");
            } else {
                tracing::info!("summaries are off — this meeting keeps its notes and transcript");
            }
            None
        }
    };

    let summary_md = summary_md.map(|md| match user_title.as_deref() {
        Some(title) => crate::refinement::apply_title(&md, title),
        None => md,
    });

    let inferred_attendees = summary_md
        .as_deref()
        .map(parse_attendees)
        .unwrap_or_default();
    let attendees = if inferred_attendees.is_empty() {
        fallback_attendees
    } else {
        inferred_attendees
    };

    // The user's raw notes persist verbatim in their own column (the Notes
    // tab renders them with the star anchors); the summary document stays
    // pure synthesis.
    let title = user_title.clone().unwrap_or_else(|| {
        summary_md
            .as_deref()
            .and_then(crate::refinement::extract_title)
            .unwrap_or_else(|| "Untitled Meeting".to_string())
    });
    let frontmatter = canonical_frontmatter(&start_time, duration_minutes, &meeting_id, &attendees);
    let notes_document = summary_md
        .as_deref()
        .map(|md| prepend_frontmatter(md, &frontmatter));

    let safe_title = crate::refinement::sanitize_filename(&title);
    // meeting_id is "YYMMDDTHHMMSS_XXXXXX" â€” timestamp prefix is first 13 chars
    let ts_prefix = &meeting_id[..13.min(meeting_id.len())];
    let final_stem = format!("{} - {}", ts_prefix, safe_title);
    let final_markdown_filename = format!("{}.md", final_stem);
    let final_audio_filename = format!("{}.mp3", final_stem);
    let final_notes_path = base.join("notes").join(&final_markdown_filename);

    // No summary, no notes file: the transcript is the meeting's document.
    if let Some(document) = notes_document.as_deref() {
        let placeholder = base.join("notes").join(format!("{}.md", meeting_id));
        if let Err(e) = std::fs::write(&placeholder, document) {
            let _ = app.emit("processing-error", e.to_string());
            crate::telemetry::track(
                &app.state::<AppState>(),
                "error",
                serde_json::json!({ "category": "save_failed" }),
            );
            return;
        }
        let _ = std::fs::rename(&placeholder, &final_notes_path);
    }

    // No segments, no transcript document — same rule as the summary: an
    // empty shell of a file helps nobody.
    let transcript_markdown = if segments.is_empty() {
        String::new()
    } else {
        format_transcript_document(
            &title,
            &meeting_id,
            &start_time,
            duration_minutes,
            &attendees,
            &transcript_text,
        )
    };
    let final_transcript_path = base.join("transcripts").join(&final_markdown_filename);
    if !transcript_markdown.is_empty() {
        if let Err(e) = std::fs::write(&final_transcript_path, &transcript_markdown) {
            tracing::error!("Failed to write transcript: {}", e);
            let _ = app.emit("processing-error", e.to_string());
            crate::telemetry::track(
                &app.state::<AppState>(),
                "error",
                serde_json::json!({ "category": "save_failed" }),
            );
        }
    }

    let final_mp3_path = base.join("audio").join(&final_audio_filename);
    let mut retained_audio_filename = final_audio_filename.clone();
    if mp3_path.exists() {
        if let Err(e) = std::fs::rename(&mp3_path, &final_mp3_path) {
            tracing::warn!("Failed to rename MP3: {}", e);
            retained_audio_filename = format!("{}.mp3", meeting_id);
        }
    }

    let duration_secs = segments.last().map(|s| s.end as u64).unwrap_or_else(|| {
        chrono::Utc::now()
            .signed_duration_since(started_at)
            .num_seconds()
            .max(0) as u64
    });
    // True whether the rename succeeded (final path) or failed (id-named
    // fallback) â€” either way a playable file exists on disk.
    let audio_present = final_mp3_path.exists() || base.join(&retained_audio_filename).exists()
        || base.join("audio").join(&retained_audio_filename).exists();

    // Persist to the database (source of truth), then regenerate the
    // index.json export the MCP servers read.
    let row = MeetingRow {
        id: meeting_id.clone(),
        title: title.clone(),
        started_at,
        duration_seconds: duration_secs,
        notes_md: notes_document.clone().unwrap_or_default(),
        transcript_md: transcript_markdown.clone(),
        attendees: attendees.clone(),
        audio_path: if config.retain_audio && audio_present {
            format!("audio/{}", retained_audio_filename)
        } else {
            String::new()
        },
        // Empty when the meeting has no summary — there is no file to point at.
        notes_path: match notes_document {
            Some(_) => format!("notes/{}", final_markdown_filename),
            None => String::new(),
        },
        // Empty when the meeting has no transcript — no file was written.
        transcript_path: if transcript_markdown.is_empty() {
            String::new()
        } else {
            format!("transcripts/{}", final_markdown_filename)
        },
    };
    let record = row.to_record();

    if let Err(e) = db
        .upsert_meeting(&row)
        .and_then(|()| db.replace_segments(&meeting_id, &segments))
        .and_then(|()| db.set_name_suggestions(&meeting_id, &name_suggestions_json))
        .and_then(|()| {
            let json = serde_json::to_string(&stars).unwrap_or_else(|_| "[]".into());
            db.set_stars(&meeting_id, &json)
        })
        .and_then(|()| db.set_user_notes(&meeting_id, user_notes.as_deref().unwrap_or("")))
        .and_then(|()| crate::storage::export_index(&db, &base))
    {
        let _ = app.emit("processing-error", e.to_string());
        crate::telemetry::track(
            &app.state::<AppState>(),
            "error",
            serde_json::json!({ "category": "save_failed" }),
        );
        return;
    }
    crate::search_index::sync_meeting(&db, &app.state::<AppState>().search, &meeting_id);

    if !config.retain_audio {
        let _ = std::fs::remove_file(&final_mp3_path);
        let _ = std::fs::remove_file(&mp3_path);
    }

    // Best-effort fan-out to the Markdown export. The copy carries what the
    // include switches say — summary, the user's own notes, transcript, each
    // defaulting in.
    let summary_body = summary_md.as_deref().unwrap_or("");
    let user_notes_md = user_notes.as_deref().unwrap_or("");
    let export_document = embral_notes::integrations::compose_export(
        &frontmatter,
        &title,
        config.export_include_summary.then_some(summary_body),
        config.export_include_notes.then_some(user_notes_md),
        config
            .export_include_transcript
            .then_some(transcript_text.as_str()),
    );
    crate::refinement::run_post_meeting_integrations(&config, &record, &export_document);

    let _ = app.emit("notes-generation-complete", &record);
}

#[tauri::command]
pub async fn stop_recording(
    app: AppHandle,
    state: State<'_, AppState>,
    user_notes: Option<String>,
    meeting_title: Option<String>,
) -> Result<(), String> {
    let config = state.config.lock().await.clone();
    let base = crate::storage::storage_base(&config.storage_dir);
    let db = state.db().await?;
    let user_title = meeting_title
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    // Read in-progress meeting ID
    let meeting_id = std::fs::read_to_string(base.join("in_progress.txt"))
        .map_err(|_| "No active recording".to_string())?;
    let _ = std::fs::remove_file(base.join("in_progress.txt"));

    // --- Foreground (fast path): stop recorder, hand off to background. ---
    let recorder = state
        .recorder
        .lock()
        .await
        .take()
        .ok_or("No active recorder")?;
    let wav_path = recorder.stop().map_err(|e| e.to_string())?;

    let session_arc = state
        .session
        .lock()
        .await
        .take()
        .ok_or("No active session")?;

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

        // The *configured* provider; a mid-recording cloud→local fallback
        // shows up as error{transcription_failed}, not here ([telemetry.md]).
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
            base,
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
    });

    Ok(())
}

/// Accept the "call detected" prompt: start recording and mark it
/// auto-started so it also auto-stops when the call ends.
#[tauri::command]
pub async fn accept_detected_meeting(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
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
#[tauri::command]
pub async fn dismiss_detected_meeting(state: State<'_, AppState>) -> Result<(), String> {
    state
        .detection_dismissed
        .store(true, std::sync::atomic::Ordering::Release);
    crate::telemetry::track(
        &state,
        "detection_response",
        serde_json::json!({ "action": "dismissed" }),
    );
    Ok(())
}

/// Transcribe an existing audio file with the local engine and store it like
/// any other meeting. Local models only (any catalog model works; the
/// offline parakeet-tdt gives the best quality). Emits `import-started`,
/// `import-progress {fraction}`, and then the standard processing events.
#[tauri::command]
pub async fn import_recording(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    title: Option<String>,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;

    if state.recorder.lock().await.is_some() {
        return Err("Can't import while a recording is in progress.".to_string());
    }
    if state
        .importing
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("An import is already in progress.".to_string());
    }

    let config = state.config.lock().await.clone();
    if !state.engine.model_present(&config.meeting_asr_model()) {
        state.importing.store(false, Ordering::Release);
        return Err(
            "Importing needs a local speech model â€” download one in Settings â†’ Transcription."
                .to_string(),
        );
    }

    let source = PathBuf::from(&path);
    if !source.is_file() {
        state.importing.store(false, Ordering::Release);
        return Err(format!("File not found: {path}"));
    }

    let base = crate::storage::storage_base(&config.storage_dir);
    crate::storage::init_storage_dirs(&base).map_err(|e| e.to_string())?;
    let db = state.db().await.inspect_err(|_| {
        state.importing.store(false, Ordering::Release);
    })?;

    let meeting_id = crate::storage::generate_meeting_id();
    // The recording happened when the file was made, not now.
    let started_at = std::fs::metadata(&source)
        .and_then(|m| m.modified())
        .map(chrono::DateTime::<chrono::Utc>::from)
        .unwrap_or_else(|_| chrono::Utc::now());
    let user_title = title
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            source
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
        });

    let _ = app.emit("import-started", ());

    let engine = state.engine.clone();
    let importing_flag = state.importing_handle();
    let app_bg = app.clone();
    tokio::spawn(async move {
        // Reset the guard on every exit path.
        struct Guard(Arc<std::sync::atomic::AtomicBool>);
        impl Drop for Guard {
            fn drop(&mut self) {
                self.0.store(false, std::sync::atomic::Ordering::Release);
            }
        }
        let _guard = Guard(importing_flag);

        // Decode + transcribe on a blocking thread (CPU-bound throughout).
        let app_progress = app_bg.clone();
        let model_id = config.meeting_asr_model();
        let vocabulary = config.vocabulary.clone();
        let result = tokio::task::spawn_blocking(move || -> anyhow::Result<(Vec<embral_types::TranscriptionSegment>, Vec<f32>)> {
            let samples = embral_engine::decode::decode_to_pcm16k(&source)?;
            // No live labels: the offline pipeline diarizes the import whole.
            let mut session = engine.create_session(&model_id, &vocabulary, false)?;

            let mut segments = Vec::new();
            let chunk = 4800; // 0.3 s â€” small enough for steady progress
            let total = samples.len().max(1);
            let mut last_emit_pct = 0u32;
            for (i, part) in samples.chunks(chunk).enumerate() {
                for ev in session.accept(part) {
                    if let embral_engine::SessionEvent::Final { text, start, end, .. } = ev {
                        segments.push(embral_types::TranscriptionSegment {
                            speaker: None,
                            speaker_id: None,
                            text,
                            start,
                            end,
                        });
                    }
                }
                let pct = ((i * chunk).min(total) * 100 / total) as u32;
                if pct >= last_emit_pct + 2 {
                    last_emit_pct = pct;
                    let _ = app_progress.emit(
                        "import-progress",
                        serde_json::json!({ "fraction": pct as f64 / 100.0 }),
                    );
                }
            }
            for ev in session.finish() {
                if let embral_engine::SessionEvent::Final { text, start, end, .. } = ev {
                    segments.push(embral_types::TranscriptionSegment {
                        speaker: None,
                        speaker_id: None,
                        text,
                        start,
                        end,
                    });
                }
            }
            Ok((segments, samples))
        })
        .await;

        let (segments, samples) = match result {
            Ok(Ok(pair)) => pair,
            Ok(Err(e)) => {
                tracing::error!("import failed: {e}");
                let _ = app_bg.emit("processing-error", format!("Import failed: {e}"));
                crate::telemetry::track(
                    &app_bg.state::<AppState>(),
                    "error",
                    serde_json::json!({ "category": "import_failed" }),
                );
                return;
            }
            Err(e) => {
                tracing::error!("import task panicked: {e}");
                let _ = app_bg.emit("processing-error", "Import failed unexpectedly.".to_string());
                crate::telemetry::track(
                    &app_bg.state::<AppState>(),
                    "error",
                    serde_json::json!({ "category": "import_failed" }),
                );
                return;
            }
        };
        crate::telemetry::track(
            &app_bg.state::<AppState>(),
            "meeting_imported",
            serde_json::json!({}),
        );

        finalize_meeting(
            app_bg,
            db,
            base,
            config,
            meeting_id,
            started_at,
            segments,
            AudioSource::Samples(Arc::new(samples)),
            false, // imported segments are unlabeled; always diarize
            Vec::new(), // no stars on imports
            None,
            user_title,
        )
        .await;
    });

    Ok(())
}

#[tauri::command]
pub async fn get_meetings(
    state: State<'_, AppState>,
    limit: Option<u32>,
    since: Option<String>,
) -> Result<Vec<MeetingSummary>, String> {
    let db = state.db().await?;
    let since = parse_since(since)?;
    let rows = db.list_meetings(limit, since).map_err(|e| e.to_string())?;
    Ok(rows
        .iter()
        .map(|r| MeetingSummary::from(&r.to_record()))
        .collect())
}

#[tauri::command]
pub async fn get_meeting_records(
    state: State<'_, AppState>,
    limit: Option<u32>,
    since: Option<String>,
) -> Result<Vec<MeetingRecord>, String> {
    let db = state.db().await?;
    let since = parse_since(since)?;
    let rows = db.list_meetings(limit, since).map_err(|e| e.to_string())?;
    Ok(rows.iter().map(MeetingRow::to_record).collect())
}

fn parse_since(since: Option<String>) -> Result<Option<chrono::DateTime<chrono::Utc>>, String> {
    since
        .map(|s| {
            chrono::DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .map_err(|e| e.to_string())
        })
        .transpose()
}

#[tauri::command]
pub async fn get_meeting(state: State<'_, AppState>, id: String) -> Result<String, String> {
    let db = state.db().await?;
    Ok(require_row(&db, &id)?.notes_md)
}

#[tauri::command]
pub async fn get_meeting_detail(
    state: State<'_, AppState>,
    id: String,
) -> Result<MeetingDetail, String> {
    let config = state.config.lock().await.clone();
    let base = crate::storage::storage_base(&config.storage_dir);
    let db = state.db().await?;
    meeting_detail(&db, &base, require_row(&db, &id)?)
}

#[derive(serde::Serialize)]
pub struct LibraryMeetingHit {
    pub id: String,
    pub title: String,
    pub started_at: String,
    pub snippet: String,
}

#[derive(serde::Serialize)]
pub struct LibraryDictationHit {
    pub id: i64,
    pub snippet: String,
    pub created_at: String,
}

#[derive(serde::Serialize)]
pub struct LibrarySearchResults {
    pub meetings: Vec<LibraryMeetingHit>,
    pub dictations: Vec<LibraryDictationHit>,
}

/// A semantic-only hit carries no FTS excerpt; lead with the passage.
fn hit_snippet(hit: &embral_search::Hit) -> String {
    hit.snippet.clone().unwrap_or_else(|| {
        let mut text: String = hit.text.chars().take(140).collect();
        if text.len() < hit.text.len() {
            text.push('…');
        }
        text
    })
}

/// The palette's search: the hybrid engine over meetings (best passage per
/// meeting) and dictations in one call. The vector leg joins only when the
/// embed worker is already warm — a keystroke never waits on a model load;
/// a cold worker gets a background warm-up and the next keystroke benefits.
#[tauri::command]
pub async fn search_library(
    app: AppHandle,
    state: State<'_, AppState>,
    query: String,
    limit: Option<u32>,
) -> Result<LibrarySearchResults, String> {
    // Timed end to end. The legs are benched separately (embral-search's
    // bench harness); this line says where keystroke time actually goes.
    let started = std::time::Instant::now();
    crate::telemetry::track(&state, "search_used", serde_json::json!({}));
    let db = state.db().await?;
    let acquire = started.elapsed();

    let q = query.trim().to_string();
    let mut vector: Option<Vec<f32>> = None;
    let mut embed = std::time::Duration::ZERO;
    if q.chars().count() >= 4 && embral_search::model::present() {
        if state.search.is_warm().await {
            let t = std::time::Instant::now();
            match state.search.embed_query(&q).await {
                Ok(v) => vector = Some(v),
                Err(e) => tracing::debug!("query embed failed, keyword-only: {e}"),
            }
            embed = t.elapsed();
        } else {
            crate::search_index::SearchRuntime::warm_up(app.clone());
        }
    }

    let limit = limit.unwrap_or(12) as usize;
    let mut args = embral_search::SearchArgs::new(&q, embral_search::OwnerKind::Meetings);
    // Chunk-level hits collapse to meetings below; fetch extra so dense
    // meetings don't crowd out the rest.
    args.limit = limit * 3;
    args.prefix_last_token = true;
    let chunk_hits =
        embral_search::search(&db, &args, vector.as_deref()).map_err(|e| e.to_string())?;

    let mut seen = std::collections::HashSet::new();
    let mut meetings = Vec::new();
    for hit in &chunk_hits {
        let Some(meeting_id) = hit.meeting_id.clone() else { continue };
        if !seen.insert(meeting_id.clone()) {
            continue;
        }
        meetings.push(LibraryMeetingHit {
            id: meeting_id,
            title: hit.title.clone().unwrap_or_default(),
            started_at: hit.date.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            snippet: hit_snippet(hit),
        });
        if meetings.len() >= limit {
            break;
        }
    }

    let mut args = embral_search::SearchArgs::new(&q, embral_search::OwnerKind::Dictations);
    args.limit = 5;
    args.prefix_last_token = true;
    let dictations = embral_search::search(&db, &args, vector.as_deref())
        .map_err(|e| e.to_string())?
        .iter()
        .filter_map(|hit| {
            Some(LibraryDictationHit {
                id: hit.dictation_id?,
                snippet: hit_snippet(hit),
                created_at: hit.date.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            })
        })
        .collect();

    // Debug, not info: this fires on every debounced keystroke, and a shipped
    // log should not be a keylogger of what the user searched for.
    tracing::debug!(
        meetings = meetings.len(),
        semantic = vector.is_some(),
        acquire_ms = acquire.as_secs_f64() * 1000.0,
        embed_ms = embed.as_secs_f64() * 1000.0,
        total_ms = started.elapsed().as_secs_f64() * 1000.0,
        "search"
    );
    Ok(LibrarySearchResults { meetings, dictations })
}

#[tauri::command]
pub async fn update_meeting_title(
    state: State<'_, AppState>,
    id: String,
    title: String,
) -> Result<MeetingDetail, String> {
    let title = normalize_title(title)?;
    let config = state.config.lock().await.clone();
    let base = crate::storage::storage_base(&config.storage_dir);
    let db = state.db().await?;
    let mut row = require_row(&db, &id)?;
    let old_record = row.to_record();

    let safe_title = crate::refinement::sanitize_filename(&title);
    let stem = format!("{} - {}", meeting_timestamp_prefix(&old_record), safe_title);
    let new_notes_path = format!("notes/{}.md", stem);
    let new_transcript_path = format!("transcripts/{}.md", stem);
    let new_audio_path = if row.audio_path.trim().is_empty() {
        String::new()
    } else {
        format!("audio/{}.mp3", stem)
    };

    rename_indexed_file(&base, &row.notes_path, &new_notes_path)?;
    rename_indexed_file(&base, &row.transcript_path, &new_transcript_path)?;
    if !new_audio_path.is_empty() {
        rename_indexed_file(&base, &row.audio_path, &new_audio_path)?;
    }

    // A meeting recorded with summaries off has no notes document, and must
    // not gain a path to a file nobody ever wrote.
    let has_summary = !row.notes_md.trim().is_empty();
    if has_summary {
        let titled = crate::refinement::apply_title(&row.notes_md, &title);
        row.notes_md = canonicalize_frontmatter(&titled, &old_record, &row.attendees);
        write_indexed_text(&base, &new_notes_path, &row.notes_md)?;
    }
    if !row.transcript_md.trim().is_empty() {
        let titled =
            crate::refinement::apply_title(&row.transcript_md, &format!("{} Transcript", title));
        row.transcript_md = canonicalize_frontmatter(&titled, &old_record, &row.attendees);
        write_indexed_text(&base, &new_transcript_path, &row.transcript_md)?;
    }

    row.title = title;
    row.notes_path = if has_summary {
        new_notes_path
    } else {
        String::new()
    };
    // Same rule as notes: a transcript-less meeting (transcription disabled)
    // must not gain a path to a file nobody ever wrote.
    row.transcript_path = if row.transcript_md.trim().is_empty() {
        String::new()
    } else {
        new_transcript_path
    };
    row.audio_path = new_audio_path;
    db.upsert_meeting(&row).map_err(|e| e.to_string())?;
    crate::storage::export_index(&db, &base).map_err(|e| e.to_string())?;
    crate::search_index::sync_meeting(&db, &state.search, &row.id);

    meeting_detail(&db, &base, row)
}

#[tauri::command]
pub async fn update_meeting_notes(
    state: State<'_, AppState>,
    id: String,
    markdown: String,
) -> Result<MeetingDetail, String> {
    let config = state.config.lock().await.clone();
    let base = crate::storage::storage_base(&config.storage_dir);
    let db = state.db().await?;
    let mut row = require_row(&db, &id)?;
    let record = row.to_record();

    let mut attendees = parse_attendees(&markdown);
    if attendees.is_empty() {
        attendees = row.attendees.clone();
    }
    row.notes_md = canonicalize_frontmatter(&markdown, &record, &attendees);
    row.attendees = attendees;
    write_indexed_text(&base, &row.notes_path, &row.notes_md)?;
    db.upsert_meeting(&row).map_err(|e| e.to_string())?;
    crate::storage::export_index(&db, &base).map_err(|e| e.to_string())?;
    crate::search_index::sync_meeting(&db, &state.search, &id);
    meeting_detail(&db, &base, row)
}

#[tauri::command]
pub async fn update_meeting_transcript(
    state: State<'_, AppState>,
    id: String,
    markdown: String,
) -> Result<MeetingDetail, String> {
    let config = state.config.lock().await.clone();
    let base = crate::storage::storage_base(&config.storage_dir);
    let db = state.db().await?;
    let mut row = require_row(&db, &id)?;
    let record = row.to_record();

    let mut attendees = parse_attendees(&markdown);
    if attendees.is_empty() {
        attendees = row.attendees.clone();
    }
    row.transcript_md = canonicalize_frontmatter(&markdown, &record, &attendees);
    row.attendees = attendees;
    write_indexed_text(&base, &row.transcript_path, &row.transcript_md)?;
    db.upsert_meeting(&row).map_err(|e| e.to_string())?;
    crate::storage::export_index(&db, &base).map_err(|e| e.to_string())?;
    crate::search_index::sync_meeting(&db, &state.search, &id);
    meeting_detail(&db, &base, row)
}

/// Delete several meetings at once (the list's multi-select). The index is
/// exported **once** at the end rather than per row, and a missing meeting is
/// not an error — it is already in the state the caller wanted.
#[tauri::command]
pub async fn delete_meetings(state: State<'_, AppState>, ids: Vec<String>) -> Result<(), String> {
    let config = state.config.lock().await.clone();
    let base = crate::storage::storage_base(&config.storage_dir);
    let db = state.db().await?;

    for id in &ids {
        let Some(row) = db.get_meeting(id).map_err(|e| e.to_string())? else {
            continue;
        };
        remove_indexed_file(&base, &row.notes_path)?;
        remove_indexed_file(&base, &row.transcript_path)?;
        remove_indexed_file(&base, &row.audio_path)?;
        db.delete_meeting(id).map_err(|e| e.to_string())?;
    }
    crate::storage::export_index(&db, &base).map_err(|e| e.to_string())?;
    crate::search_index::after_delete(&db);
    Ok(())
}

#[tauri::command]
pub async fn delete_meeting(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let config = state.config.lock().await.clone();
    let base = crate::storage::storage_base(&config.storage_dir);
    let db = state.db().await?;
    let row = require_row(&db, &id)?;

    remove_indexed_file(&base, &row.notes_path)?;
    remove_indexed_file(&base, &row.transcript_path)?;
    remove_indexed_file(&base, &row.audio_path)?;
    db.delete_meeting(&id).map_err(|e| e.to_string())?;
    crate::storage::export_index(&db, &base).map_err(|e| e.to_string())?;
    crate::search_index::after_delete(&db);
    Ok(())
}

#[tauri::command]
pub async fn get_config() -> Result<AppConfig, String> {
    crate::config::load_config().map_err(|e| e.to_string())
}

/// Restore every setting to its default (onboarding included, so it runs
/// again on the next frontend config load). Meetings, profiles, and
/// downloaded models are untouched.
#[derive(serde::Deserialize)]
pub struct ResetScopes {
    pub settings: bool,
    pub meetings: bool,
    pub profiles: bool,
    pub dictations: bool,
    pub models: bool,
}

/// Whether installing an update (which restarts the app) is safe right
/// now. Returns the human-readable reason to wait, or `None` when clear —
/// the updater UI refuses the restart while any of these are live, so an
/// update can never kill a recording, a dictation, an import, or a voice
/// enrollment mid-flight.
#[tauri::command]
pub async fn update_guard(state: State<'_, AppState>) -> Result<Option<String>, String> {
    use std::sync::atomic::Ordering;
    if state.recorder.lock().await.is_some() {
        return Ok(Some("A recording is in progress".to_string()));
    }
    if state.dictating.load(Ordering::Acquire) {
        return Ok(Some("A dictation is in progress".to_string()));
    }
    if state.importing.load(Ordering::Acquire) {
        return Ok(Some("An import is in progress".to_string()));
    }
    Ok(None)
}

/// The scoped reset behind About → Reset…: each flag deletes one body of
/// data outright — config to defaults, meetings (rows + their files),
/// speaker profiles, dictation history, downloaded models.
/// Refused while anything is using the mic; no scope is reversible.
#[tauri::command]
pub async fn reset_app_data(
    scopes: ResetScopes,
    state: State<'_, AppState>,
) -> Result<AppConfig, String> {
    if state.recorder.lock().await.is_some() {
        return Err("Stop the recording before resetting".to_string());
    }
    if state.dictating.load(std::sync::atomic::Ordering::Acquire) {
        return Err("Stop dictating before resetting".to_string());
    }

    let config = state.config.lock().await.clone();
    let base = crate::storage::storage_base(&config.storage_dir);

    if scopes.meetings || scopes.profiles || scopes.dictations {
        let db = state.db().await?;

        if scopes.meetings {
            // Files first (the rows carry the paths), then the rows, then
            // the index the MCP servers read.
            for row in db.list_meetings(None, None).map_err(|e| e.to_string())? {
                remove_indexed_file(&base, &row.notes_path)?;
                remove_indexed_file(&base, &row.transcript_path)?;
                remove_indexed_file(&base, &row.audio_path)?;
            }
            let n = db.clear_meetings().map_err(|e| e.to_string())?;
            crate::storage::export_index(&db, &base).map_err(|e| e.to_string())?;
            tracing::info!(removed = n, "reset cleared meetings");
        }

        if scopes.profiles {
            db.clear_speakers().map_err(|e| e.to_string())?;
            // Voice clips are no longer recorded; sweep any left behind by
            // older versions.
            let _ = std::fs::remove_dir_all(base.join("voices"));
            tracing::info!("reset cleared speaker profiles");
        }

        if scopes.dictations {
            let n = db.clear_dictations().map_err(|e| e.to_string())?;
            tracing::info!(removed = n, "reset cleared dictation history");
        }

        if scopes.meetings && scopes.dictations {
            // Nothing owns a chunk anymore; drop the whole search index.
            if let Err(e) = embral_search::clear_index(&db) {
                tracing::warn!("reset couldn't clear the search index: {e:#}");
            }
        } else if scopes.meetings || scopes.dictations {
            crate::search_index::after_delete(&db);
        }
    }

    if scopes.models {
        // The LLM sidecar holds its weights open; release before deleting —
        // same for the embedding worker and its model files.
        state.llm.shutdown();
        state.search.shutdown().await;
        for model in embral_engine::catalog::MODELS {
            state.engine.evict(model.id);
            if let Err(e) = embral_engine::catalog::delete(model.id) {
                tracing::warn!(model = model.id, "reset couldn't delete model: {e}");
            }
        }
        tracing::info!("reset cleared downloaded models");
    }

    if scopes.settings {
        let fresh = AppConfig::default();
        crate::config::save_config(&fresh).map_err(|e| e.to_string())?;
        *state.config.lock().await = fresh;
        // Defaults have telemetry off; keep the sync mirror honest.
        #[cfg(feature = "cloud")]
        state
            .telemetry
            .enabled
            .store(false, std::sync::atomic::Ordering::Release);
    }

    Ok(state.config.lock().await.clone())
}

#[tauri::command]
pub async fn save_config(
    app: AppHandle,
    mut config: AppConfig,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let hotkeys_changed = {
        let current = state.config.lock().await;
        current.record_hotkey != config.record_hotkey
            || current.dictation_hotkey != config.dictation_hotkey
    };
    // The telemetry install id (cloud edition) lives and dies with the
    // opt-in: minted when enabled without one, cleared (with the snapshot
    // date) on opt-out so opting out genuinely severs history
    // ([telemetry.md]).
    #[cfg(feature = "cloud")]
    {
        if config.telemetry_enabled && config.telemetry_install_id.is_empty() {
            config.telemetry_install_id = uuid::Uuid::new_v4().to_string();
        }
        if !config.telemetry_enabled {
            config.telemetry_install_id.clear();
            config.telemetry_last_snapshot.clear();
        }
        state
            .telemetry
            .enabled
            .store(config.telemetry_enabled, std::sync::atomic::Ordering::Release);
    }
    crate::config::save_config(&config).map_err(|e| e.to_string())?;
    let record = config.record_hotkey.clone();
    let dictation = config.dictation_hotkey.clone();
    // A changed recording-disc override applies on the spot.
    crate::tray::set_recording_color(&config.tray_recording_color);
    *state.config.lock().await = config;
    let _ = crate::tray::refresh(&app);
    if hotkeys_changed {
        // Surface an invalid combo to the settings UI; config stays saved so
        // the user can correct it.
        crate::hotkey::apply(&app, &record, &dictation)?;
    }
    Ok(())
}

/// Render the export filename template against a sample meeting, for the live
/// preview in Settings. Uses the same Rust renderer as real exports so the
/// preview can't drift.
#[tauri::command]
pub async fn preview_export_filename(template: String) -> Result<String, String> {
    let sample_time = chrono::Utc::now();
    let stem = embral_notes::integrations::render_filename(&template, "Weekly sync", &sample_time);
    Ok(format!("{stem}.md"))
}

/// Names of the machine's audio devices, for the Settings pickers. An empty
/// selection in config means "system default", so these lists are additive.
#[derive(serde::Serialize)]
pub struct AudioDevices {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
}

#[tauri::command]
pub async fn list_audio_devices() -> Result<AudioDevices, String> {
    // Device enumeration can block on driver calls; keep it off the runtime.
    tokio::task::spawn_blocking(|| {
        use cpal::traits::{DeviceTrait, HostTrait};
        fn names<I: Iterator<Item = cpal::Device>, E>(devices: Result<I, E>) -> Vec<String> {
            devices
                .map(|it| it.filter_map(|d| d.name().ok()).collect())
                .unwrap_or_default()
        }
        let host = cpal::default_host();
        AudioDevices {
            inputs: names(host.input_devices()),
            outputs: names(host.output_devices()),
        }
    })
    .await
    .map_err(|e| e.to_string())
}

// --- Local model management (sherpa-onnx engine catalog) ---

#[tauri::command]
pub async fn asr_models_status() -> Result<Vec<embral_engine::ModelStatus>, String> {
    Ok(embral_engine::catalog::statuses())
}

/// Whether the built-in LLM sidecar is currently loaded in memory.
#[tauri::command]
pub async fn llm_status(state: State<'_, AppState>) -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({ "running": state.llm.is_running() }))
}

/// The two halves of the summary prompt for the settings editor: the
/// editable default body and the locked output contract.
#[tauri::command]
pub async fn get_summary_prompt_parts() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "default": embral_notes::prompt::DEFAULT_SUMMARY_PROMPT,
        "contract": embral_notes::prompt::OUTPUT_CONTRACT,
    }))
}

// --- Dictation ---

#[tauri::command]
pub async fn start_dictation(app: AppHandle) -> Result<(), String> {
    crate::dictation::start(&app).await
}

#[tauri::command]
pub async fn stop_dictation(app: AppHandle) -> Result<String, String> {
    crate::dictation::stop(&app).await
}

#[tauri::command]
pub async fn cancel_dictation(app: AppHandle) -> Result<(), String> {
    crate::dictation::cancel(&app).await
}

#[tauri::command]
pub async fn list_dictations(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<embral_db::DictationRow>, String> {
    let db = state.db().await?;
    db.list_dictations(limit.unwrap_or(100)).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_dictation(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let db = state.db().await?;
    db.delete_dictation(id).map_err(|e| e.to_string())?;
    crate::search_index::after_delete(&db);
    Ok(())
}

/// Download one catalog model, emitting `model-download-progress` throughout
/// and `model-download-complete` at the end. Concurrent downloads of the same
/// model are rejected; different models may download in parallel.
#[tauri::command]
pub async fn download_asr_model(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: String,
) -> Result<(), String> {
    {
        let mut in_flight = state.model_downloads.lock().expect("downloads mutex");
        if !in_flight.insert(model_id.clone()) {
            return Err("This model is already downloading.".to_string());
        }
    }
    struct Guard<'a> {
        set: &'a std::sync::Mutex<std::collections::HashSet<String>>,
        id: String,
    }
    impl Drop for Guard<'_> {
        fn drop(&mut self) {
            self.set.lock().expect("downloads mutex").remove(&self.id);
        }
    }
    let _guard = Guard {
        set: &state.model_downloads,
        id: model_id.clone(),
    };

    // The sidecar holds the runtime exe/DLLs and the weights open; extracting
    // or renaming over them fails on Windows. Stop it first — it restarts on
    // next use.
    if matches!(model_id.as_str(), "llama-server" | "qwen3-4b") {
        state.llm.shutdown();
    }

    let app_progress = app.clone();
    if let Err(e) = embral_engine::catalog::download(&model_id, move |p| {
        let _ = app_progress.emit("model-download-progress", &p);
    })
    .await
    {
        crate::telemetry::track(
            &state,
            "error",
            serde_json::json!({ "category": "model_download_failed" }),
        );
        return Err(e.to_string());
    }

    crate::telemetry::track(
        &state,
        "model_downloaded",
        serde_json::json!({ "model_id": model_id }),
    );
    let _ = app.emit(
        "model-download-complete",
        serde_json::json!({ "model_id": model_id }),
    );
    Ok(())
}

#[tauri::command]
pub async fn delete_asr_model(
    app: AppHandle,
    state: State<'_, AppState>,
    model_id: String,
) -> Result<(), String> {
    if model_id == embral_search::model::MODEL_ID {
        // The embed worker holds the model files open; release them first.
        state.search.shutdown().await;
    }
    // Same for the LLM sidecar: deleting the runtime or weights under a
    // running llama-server leaves NTFS delete-pending files that block
    // every re-download until the process dies.
    if matches!(model_id.as_str(), "llama-server" | "qwen3-4b") {
        state.llm.shutdown();
    }
    embral_engine::catalog::delete(&model_id).map_err(|e| e.to_string())?;
    // Drop any warm recognizer so a re-download loads fresh files.
    state.engine.evict(&model_id);
    crate::telemetry::track(
        &state,
        "model_deleted",
        serde_json::json!({ "model_id": model_id }),
    );
    let _ = app.emit(
        "model-download-complete",
        serde_json::json!({ "model_id": model_id }),
    );
    Ok(())
}

#[tauri::command]
pub async fn open_logs_folder<R: tauri::Runtime>(app: AppHandle<R>) -> Result<(), String> {
    let logs = crate::logs_dir();
    let _ = std::fs::create_dir_all(&logs);
    app.opener()
        .open_path(logs.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn open_notes_folder<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let config = state.config.lock().await;
    let base = crate::storage::storage_base(&config.storage_dir);
    drop(config);
    crate::telemetry::track(&state, "notes_folder_opened", serde_json::json!({}));
    let notes = base.join("notes");
    app.opener()
        .open_path(notes.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}
