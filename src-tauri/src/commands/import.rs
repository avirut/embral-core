//! Importing an existing audio file as a meeting (local engine only).

use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

use embral_types::AppError;
use crate::AppState;

use super::finalize::{finalize_meeting, AudioSource};

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
) -> Result<(), AppError> {
    use std::sync::atomic::Ordering;

    if state.recorder.lock().await.is_some() {
        return Err(AppError::CantImportWhileRecording);
    }
    if state
        .importing
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(AppError::ImportAlreadyRunning);
    }

    let config = state.config.lock().await.clone();
    if !state.engine.model_present(&config.meeting_asr_model()) {
        state.importing.store(false, Ordering::Release);
        return Err(AppError::NeedsLocalModel);
    }

    let source = PathBuf::from(&path);
    if !source.is_file() {
        state.importing.store(false, Ordering::Release);
        return Err(AppError::FileNotFound { path: path.clone() });
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
                let _ = app_bg.emit("processing-error", &AppError::ImportFailed { detail: e.to_string() });
                crate::telemetry::track(
                    &app_bg.state::<AppState>(),
                    "error",
                    serde_json::json!({ "category": "import_failed" }),
                );
                return;
            }
            Err(e) => {
                tracing::error!("import task panicked: {e}");
                let _ = app_bg.emit("processing-error", &AppError::ImportFailed { detail: e.to_string() });
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
