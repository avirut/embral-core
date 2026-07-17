//! Dictation: hotkey-driven mic-only speech-to-text into any app.
//!
//! Flow: global hotkey → mic stream into a transcription session from the
//! provider seam (dictation's own provider tree — on-device or the cloud
//! relay) → on stop: optional AI cleanup → clipboard (+ optional paste into
//! the focused app) → history row in the DB. The overlay window is a minimal
//! listening indicator; nothing renders the words live.
//!
//! Dictation and meeting recording are mutually exclusive — both need the
//! microphone and the transcription engine's attention.

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use embral_types::AppConfig;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;

use crate::audio::recorder::MicStream;
use crate::transcription::{TranscriptionEvent, TranscriptionSession};
use crate::AppState;

/// A tap shorter than this is toggle mode; holding longer means push-to-talk
/// (stop on release).
pub const HOLD_THRESHOLD: Duration = Duration::from_millis(700);

/// Label of the overlay window.
const OVERLAY: &str = "dictation";

/// How long stop() waits for the session to flush its tail. Same rationale
/// as the meetings pipeline: nothing new arrives after a few seconds.
const FINISH_TIMEOUT: Duration = Duration::from_secs(8);

pub struct ActiveDictation {
    mic: Option<MicStream>,
    /// Shared with the audio bridge; stop() takes it to call finish().
    session: Arc<Mutex<Option<Box<dyn TranscriptionSession>>>>,
    bridge: tokio::task::JoinHandle<()>,
    /// Segment texts mirrored by the event consumer — the fallback when
    /// finish() errors or times out, so a flaky session still delivers what
    /// was heard.
    heard: Arc<std::sync::Mutex<Vec<String>>>,
}

/// Pure decision rule for the dictation hotkey (unit-tested): what to do on
/// a press/release given whether a session is active and how long ago the
/// initiating press happened.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum HotkeyAction {
    Start,
    Stop,
    Nothing,
}

pub fn on_press(active: bool) -> HotkeyAction {
    if active {
        HotkeyAction::Stop // second tap ends a toggle-mode session
    } else {
        HotkeyAction::Start
    }
}

pub fn on_release(active: bool, held: Duration) -> HotkeyAction {
    if active && held >= HOLD_THRESHOLD {
        HotkeyAction::Stop // push-to-talk: released after holding
    } else {
        HotkeyAction::Nothing // short tap: stay in toggle mode
    }
}

/// Whether this dictation configuration requires the on-device model on
/// disk before starting: the device is the primary, or it is where an
/// out-of-hours cloud session lands. Cloud with "disabled" needs nothing
/// local — failing without a fallback is what the user asked for.
fn needs_local_model(config: &AppConfig) -> bool {
    match config.dictation_provider {
        embral_types::TranscriptionProvider::Local => true,
        #[cfg(feature = "cloud")]
        embral_types::TranscriptionProvider::Cloud => {
            config.dictation_out_of_hours == embral_types::CloudOutOfHours::Local
        }
    }
}

/// Start a dictation session: a transcription session from the provider
/// seam, the overlay indicator, the mic streaming into it.
pub async fn start(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    if state.recorder.lock().await.is_some() {
        return Err("Can't dictate during a meeting recording".to_string());
    }
    if state.enrolling.load(Ordering::Acquire) {
        return Err("Can't dictate while recording a voice reference".to_string());
    }
    let mut slot = state.dictation.lock().await;
    if slot.is_some() {
        return Err("Dictation is already running".to_string());
    }

    let config = state.config.lock().await.clone();
    if needs_local_model(&config) {
        let model_id = config.dictation_asr_model_id();
        if !state.engine.model_present(&model_id) {
            return Err(format!(
                "The dictation speech model isn't downloaded ({model_id}) — check Settings → Transcription"
            ));
        }
    }
    #[cfg(feature = "cloud")]
    if config.dictation_provider == embral_types::TranscriptionProvider::Cloud
        && config.cloud_session_token.is_empty()
    {
        return Err("Sign in on the Account page to dictate with embral cloud".to_string());
    }

    // The session comes first — before the overlay (a failed start must not
    // leave a stuck indicator) and before the mic (so no audio is dropped
    // while weights come up or the relay connects).
    let provider = crate::transcription::build_dictation_provider(&config, state.engine.clone());
    let (event_tx, mut event_rx) =
        tokio::sync::mpsc::unbounded_channel::<TranscriptionEvent>();
    let session_result = provider.start_session(event_tx.clone()).await;
    // A cloud refusal (out of hours, unreachable) degrades per dictation's
    // out-of-hours setting, same rule as meetings (`on_cloud_failure`).
    #[cfg(feature = "cloud")]
    let session_result = match session_result {
        Err(e) if config.dictation_provider == embral_types::TranscriptionProvider::Cloud => {
            let model_present = state
                .engine
                .model_present(&config.dictation_asr_model_id());
            match crate::config::on_cloud_failure(config.dictation_out_of_hours, model_present) {
                crate::config::CloudFailureAction::SwitchToLocal => {
                    tracing::warn!("cloud dictation unavailable ({e}); using this device");
                    crate::transcription::build_local_dictation_provider(
                        &config,
                        state.engine.clone(),
                    )
                    .start_session(event_tx.clone())
                    .await
                }
                crate::config::CloudFailureAction::DisableTranscription => Err(anyhow::anyhow!(
                    "cloud dictation is unavailable ({e}), and dictation is set not to use this device"
                )),
                crate::config::CloudFailureAction::Fail => Err(e),
            }
        }
        other => other,
    };
    let session = session_result.map_err(|e| e.to_string())?;

    show_overlay(app)?;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Vec<f32>>();
    let mic_device = {
        let name = config.mic_device.trim();
        if name.is_empty() { None } else { Some(name.to_string()) }
    };
    let mic = match MicStream::start(mic_device.as_deref(), tx) {
        Ok(mic) => mic,
        Err(e) => {
            hide_overlay(app);
            // Shut the session down cleanly rather than leaking a relay
            // socket or an engine stream.
            let _ = tokio::time::timeout(FINISH_TIMEOUT, session.finish()).await;
            return Err(e.to_string());
        }
    };

    let session_arc: Arc<Mutex<Option<Box<dyn TranscriptionSession>>>> =
        Arc::new(Mutex::new(Some(session)));

    // Audio bridge: mic chunks into the session, whoever the provider is.
    let session_for_bridge = session_arc.clone();
    let bridge = tokio::spawn(async move {
        while let Some(chunk) = rx.recv().await {
            let guard = session_for_bridge.lock().await;
            if let Some(s) = guard.as_ref() {
                if let Err(e) = s.send_audio(&chunk).await {
                    tracing::warn!("dictation send_audio failed: {e}");
                }
            }
        }
    });

    // Event consumer: finish() is the source of truth for the text, so
    // Interims are ignored and Segments only mirrored (the finish-timeout
    // fallback). `Failed` mid-session means the session is gone — deliver
    // what was heard instead of dictating into the void (cloud cut off,
    // connection drop; dictations are seconds long, there is no mid-session
    // swap).
    let heard: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    let heard_for_consumer = heard.clone();
    let app_for_consumer = app.clone();
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match event {
                TranscriptionEvent::Segment(seg) => {
                    let text = seg.text.trim().to_string();
                    if !text.is_empty() {
                        heard_for_consumer
                            .lock()
                            .expect("dictation heard poisoned")
                            .push(text);
                    }
                }
                TranscriptionEvent::Failed { message } => {
                    tracing::warn!(
                        "dictation transcription ended early ({message}); delivering what was heard"
                    );
                    if let Err(e) = stop(&app_for_consumer).await {
                        tracing::warn!("auto-stop after dictation failure: {e}");
                    }
                    break;
                }
                TranscriptionEvent::Interim { .. } => {}
                TranscriptionEvent::Done => break,
            }
        }
    });

    *slot = Some(ActiveDictation {
        mic: Some(mic),
        session: session_arc,
        bridge,
        heard,
    });
    state.dictating.store(true, Ordering::Release);
    let _ = app.emit_to(OVERLAY, "dictation-started", ());
    let _ = app.emit("dictation-active", true);
    tracing::info!(provider = ?config.dictation_provider, "dictation started");
    Ok(())
}

/// Stop the session and run the output pipeline. Returns the pasted text.
pub async fn stop(app: &AppHandle) -> Result<String, String> {
    let state = app.state::<AppState>();
    let Some(mut active) = state.dictation.lock().await.take() else {
        return Err("No dictation running".to_string());
    };
    state.dictating.store(false, Ordering::Release);
    let _ = app.emit_to(OVERLAY, "dictation-finishing", ());

    // Dropping the mic ends the stream; the bridge drains the last chunks
    // into the session, then finish() flushes the tail and returns every
    // finalized segment (the seam's contract).
    active.mic.take();
    let _ = active.bridge.await;
    let raw = match active.session.lock().await.take() {
        Some(session) => {
            match tokio::time::timeout(FINISH_TIMEOUT, session.finish()).await {
                Ok(Ok(segments)) => join_segments(segments.iter().map(|s| s.text.as_str())),
                outcome => {
                    // Errored or timed out: the mirror the event consumer
                    // kept is what was actually heard.
                    match outcome {
                        Ok(Err(e)) => tracing::warn!("dictation finish errored: {e}"),
                        _ => tracing::warn!("dictation finish timed out"),
                    }
                    let heard = active.heard.lock().expect("dictation heard poisoned");
                    join_segments(heard.iter().map(String::as_str))
                }
            }
        }
        // The session already died mid-dictation (the Failed auto-stop):
        // the mirror is all there is.
        None => {
            let heard = active.heard.lock().expect("dictation heard poisoned");
            join_segments(heard.iter().map(String::as_str))
        }
    };

    let config = state.config.lock().await.clone();
    hide_overlay(app);
    let _ = app.emit("dictation-active", false);

    if raw.is_empty() {
        let _ = app.emit_to(OVERLAY, "dictation-complete", "");
        return Ok(String::new());
    }

    let focused = focused_app();

    // Cleanup per the configured tier; every failure shape delivers the raw
    // text rather than losing the dictation.
    let cleaned = match crate::llm::resolved_cleanup_config(&state.llm, &config).await {
        Some(cfg) => match embral_notes::clean_dictation(&cfg, &raw).await {
            Ok(text) => {
                state.llm.touch();
                Some(text)
            }
            Err(e) => {
                tracing::warn!("dictation cleanup failed — using raw text: {e}");
                None
            }
        },
        None => None,
    };

    let output = cleaned.clone().unwrap_or_else(|| raw.clone());

    // History first — losing the paste is recoverable, losing the text isn't.
    if let Ok(db) = state.db().await {
        match db.add_dictation(&raw, cleaned.as_deref(), focused.as_deref()) {
            Ok(id) => crate::search_index::sync_dictation(&db, &state.search, id),
            Err(e) => tracing::warn!("failed to save dictation history: {e}"),
        }
    }

    deliver(
        &output,
        config.dictation_copy_clipboard,
        config.dictation_auto_paste,
    );
    let _ = app.emit_to(OVERLAY, "dictation-complete", &output);
    let _ = app.emit("dictation-complete", &output);
    tracing::info!(
        chars = output.len(),
        cleaned = cleaned.is_some(),
        app = focused.as_deref().unwrap_or("?"),
        "dictation finished"
    );
    Ok(output)
}

/// Abort without any output. The session still gets a bounded finish so
/// engine streams and relay sockets close cleanly; the result is discarded.
pub async fn cancel(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let Some(mut active) = state.dictation.lock().await.take() else {
        return Ok(());
    };
    state.dictating.store(false, Ordering::Release);
    active.mic.take();
    let _ = active.bridge.await;
    if let Some(session) = active.session.lock().await.take() {
        let _ = tokio::time::timeout(FINISH_TIMEOUT, session.finish()).await;
    }
    hide_overlay(app);
    let _ = app.emit("dictation-active", false);
    tracing::info!("dictation cancelled");
    Ok(())
}

/// Non-empty segment texts joined into one line of dictated speech.
fn join_segments<'a>(texts: impl Iterator<Item = &'a str>) -> String {
    texts
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Hand the finished text to the user per the two output switches. Pasting
/// always stages the text on the clipboard (that is how Ctrl+V works); with
/// the clipboard switch *off*, the previous contents come back once the
/// target app has read it. With it on, the text stays. Neither switch: the
/// text lives only in history.
fn deliver(text: &str, copy: bool, paste: bool) {
    if !copy && !paste {
        return;
    }
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("clipboard unavailable: {e}");
            return;
        }
    };
    let previous = clipboard.get_text().ok();
    if let Err(e) = clipboard.set_text(text.to_string()) {
        tracing::warn!("clipboard write failed: {e}");
        return;
    }
    if !paste {
        return;
    }
    send_ctrl_v();
    // Give the target app a moment to read the clipboard, then put the
    // user's old contents back — only when they didn't ask to keep the
    // text there.
    if !copy {
        if let Some(prev) = previous {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(600));
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let _ = cb.set_text(prev);
                }
            });
        }
    }
}

/// Synthesize a Ctrl+V keystroke.
fn send_ctrl_v() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
        VK_CONTROL, VK_V,
    };
    let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let inputs = [
        key(VK_CONTROL, false),
        key(VK_V, false),
        key(VK_V, true),
        key(VK_CONTROL, true),
    ];
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

/// Process name of the app that currently has focus (e.g. `notepad.exe`).
fn focused_app() -> Option<String> {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
    };
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 || pid == std::process::id() {
            return None;
        }
        crate::autodetect::wasapi::process_name(pid)
    }
}

/// The overlay's fixed size: the minimal listening indicator, the only
/// appearance there is.
const OVERLAY_SIZE: (f64, f64) = (280.0, 64.0);

/// Create (once) and show the overlay near the bottom of the current
/// monitor. Never focused — the paste target must keep focus.
fn show_overlay(app: &AppHandle) -> Result<(), String> {
    let (w, h) = OVERLAY_SIZE;
    let window = match app.get_webview_window(OVERLAY) {
        Some(w) => w,
        None => tauri::WebviewWindowBuilder::new(
            app,
            OVERLAY,
            tauri::WebviewUrl::App("/dictation".into()),
        )
        .title("Dictation")
        .inner_size(w, h)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .resizable(false)
        .visible(false)
        .build()
        .map_err(|e| format!("overlay window failed: {e}"))?,
    };

    let _ = window.set_size(tauri::LogicalSize::new(w, h));
    if let Ok(Some(monitor)) = window.current_monitor() {
        let scale = monitor.scale_factor();
        let screen = monitor.size().to_logical::<f64>(scale);
        let pos = monitor.position().to_logical::<f64>(scale);
        let _ = window.set_position(tauri::LogicalPosition::new(
            pos.x + (screen.width - w) / 2.0,
            pos.y + screen.height - h - 64.0,
        ));
    }
    window.show().map_err(|e| e.to_string())?;
    Ok(())
}

fn hide_overlay(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(OVERLAY) {
        let _ = w.hide();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "cloud")]
    #[test]
    fn local_model_needed_only_where_the_device_transcribes() {
        use embral_types::{CloudOutOfHours, TranscriptionProvider};

        let mut config = AppConfig::default();

        // Device is the primary: always.
        config.dictation_provider = TranscriptionProvider::Local;
        assert!(needs_local_model(&config));

        // Cloud landing on the device out of hours: still needed.
        config.dictation_provider = TranscriptionProvider::Cloud;
        config.dictation_out_of_hours = CloudOutOfHours::Local;
        assert!(needs_local_model(&config));

        // Cloud with "disabled": failing without a fallback is the ask.
        config.dictation_out_of_hours = CloudOutOfHours::Disabled;
        assert!(!needs_local_model(&config));
    }

    #[test]
    fn tap_toggles_and_hold_pushes_to_talk() {
        // Idle press starts.
        assert_eq!(on_press(false), HotkeyAction::Start);
        // Quick release (tap) keeps the session running…
        assert_eq!(on_release(true, Duration::from_millis(200)), HotkeyAction::Nothing);
        // …and the next press stops it.
        assert_eq!(on_press(true), HotkeyAction::Stop);
        // Holding past the threshold stops on release.
        assert_eq!(on_release(true, Duration::from_millis(900)), HotkeyAction::Stop);
        // Release with nothing running is inert.
        assert_eq!(on_release(false, Duration::from_secs(2)), HotkeyAction::Nothing);
    }
}
