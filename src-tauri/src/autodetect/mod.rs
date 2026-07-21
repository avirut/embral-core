//! Meeting auto-detection: notices when a meeting app is using the
//! microphone and starts/stops recording per the configured policy.
//!
//! - `state` — the pure tick state machine + app matcher (unit-tested).
//! - `wasapi` — the Windows scan for processes with an active mic session.
//! - this module — the poll loop tying config, state, and actions together.

mod state;
pub mod wasapi;

use std::sync::atomic::Ordering;
use std::time::Duration;

use embral_types::AutoStartPolicy;
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;
use state::{match_app, Detection, Detector};

/// Poll cadence. The detection delay is quantized to this.
const POLL_SECS: u64 = 3;

/// Empty polls tolerated before a call counts as over. Not a setting: its
/// only job is surviving a blip (one scan where the app's capture session
/// reads as inactive), so it stays short. Genuinely back-to-back calls are
/// separate meetings and must record as two.
const AUTO_STOP_GRACE_TICKS: u32 = 2;

fn ticks_for(seconds: u32) -> u32 {
    seconds.div_ceil(POLL_SECS as u32).max(1)
}

/// Spawn the detection loop (called once from `setup()`).
pub fn spawn(handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let own_pid = std::process::id();
        // Recreated whenever the timing config changes.
        let mut detector = Detector::new(1, 1);
        let mut current_windows = (0u32, 0u32);

        loop {
            tokio::time::sleep(Duration::from_secs(POLL_SECS)).await;

            let state = handle.state::<AppState>();
            let config = state.config.lock().await.clone();

            if config.auto_start_policy == AutoStartPolicy::Manual {
                // Fully off: also forget any in-flight call state.
                if current_windows != (0, 0) {
                    detector = Detector::new(1, 1);
                    current_windows = (0, 0);
                }
                continue;
            }

            let windows = (
                ticks_for(config.detection_delay_secs),
                AUTO_STOP_GRACE_TICKS,
            );
            if windows != current_windows {
                detector = Detector::new(windows.0, windows.1);
                current_windows = windows;
            }

            // A manual recording in progress: don't track calls against it —
            // auto-stop must never end a recording the user started.
            let recording = state.recorder.lock().await.is_some();
            if recording && !state.auto_started.load(Ordering::Acquire) {
                continue;
            }

            let candidates =
                tokio::task::spawn_blocking(move || wasapi::processes_using_microphone(own_pid))
                    .await
                    .unwrap_or_default();

            let candidate = match config.auto_start_policy {
                // Always: any mic user counts, allowlisted ones preferred as
                // the reported name.
                AutoStartPolicy::Always => candidates
                    .iter()
                    .find(|c| match_app(c, &config.auto_detect_apps))
                    .or_else(|| candidates.first())
                    .cloned(),
                AutoStartPolicy::Selective | AutoStartPolicy::Prompt => candidates
                    .iter()
                    .find(|c| match_app(c, &config.auto_detect_apps))
                    .cloned(),
                AutoStartPolicy::Manual => None,
            };

            match detector.tick(candidate.as_deref()) {
                Some(Detection::Start(app)) => {
                    if recording {
                        continue; // an auto recording is already running
                    }
                    match config.auto_start_policy {
                        AutoStartPolicy::Always | AutoStartPolicy::Selective => {
                            tracing::info!(app, "call detected — starting recording");
                            state.auto_started.store(true, Ordering::Release);
                            if let Err(e) =
                                crate::commands::start_recording(handle.clone(), handle.state())
                                    .await
                            {
                                tracing::warn!("auto-start failed: {e}");
                                state.auto_started.store(false, Ordering::Release);
                            }
                        }
                        AutoStartPolicy::Prompt => {
                            if !state.detection_dismissed.load(Ordering::Acquire) {
                                tracing::info!(app, "call detected — prompting");
                                // Normalized: the raw exe name never leaves
                                // the machine ([telemetry.md]).
                                crate::telemetry::track(
                                    &state,
                                    "meeting_detected",
                                    serde_json::json!({
                                        "app": crate::telemetry::normalize_detected_app(&app)
                                    }),
                                );
                                let _ = handle
                                    .emit("meeting-detected", serde_json::json!({ "app": app }));
                            }
                        }
                        AutoStartPolicy::Manual => {}
                    }
                }
                Some(Detection::Stop) => {
                    // The call is over: reset the per-call prompt suppression
                    // and clear any lingering prompt in the UI.
                    state.detection_dismissed.store(false, Ordering::Release);
                    let _ = handle.emit("meeting-ended", ());
                    if state.auto_started.swap(false, Ordering::AcqRel)
                        && config.auto_stop_enabled
                        && state.recorder.lock().await.is_some()
                    {
                        tracing::info!("call ended — stopping auto-started recording");
                        if let Err(e) =
                            crate::commands::stop_recording(handle.clone(), handle.state(), None, None)
                                .await
                        {
                            tracing::warn!("auto-stop failed: {e}");
                        }
                    }
                }
                None => {}
            }
        }
    });
}
