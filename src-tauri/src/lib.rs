mod audio;
mod autodetect;
#[cfg(feature = "cloud")]
mod cloud;
mod commands;
mod config;
mod dictation;
mod hotkey;
mod llm;
mod mcp_clients;
mod refinement;
mod search_index;
mod speaker_commands;
mod speakers;
mod storage;
mod transcription;
mod tray;

use std::sync::Arc;
use tauri::Manager;

pub type SharedSession =
    Arc<tokio::sync::Mutex<Option<Box<dyn transcription::TranscriptionSession>>>>;

/// Source of truth for the in-progress recording's finalized segments. The
/// event-forwarder task in `start_recording` appends every `Segment` event to
/// this Vec; `stop_recording` reads from it after the recv-task has had a
/// bounded window to finalize. This decouples segment ownership from the
/// (sometimes very slow) `TranscriptionSession::finish()` return value.
pub type SharedSegments = Arc<tokio::sync::Mutex<Vec<embral_types::TranscriptionSegment>>>;

pub struct AppState {
    pub recorder: tokio::sync::Mutex<Option<audio::recorder::Recorder>>,
    pub session: tokio::sync::Mutex<Option<SharedSession>>,
    pub config: tokio::sync::Mutex<embral_types::AppConfig>,
    pub current_segments: SharedSegments,
    /// Warm local speech engine: recognizers load once per app run and stay
    /// cached, so recording start is instant after the first use.
    pub engine: Arc<embral_engine::Engine>,
    /// The open database, tagged with the storage base it belongs to so a
    /// `storage_dir` change in Settings transparently reopens against the new
    /// location (see [`AppState::db`]).
    db: tokio::sync::Mutex<Option<(std::path::PathBuf, Arc<embral_db::Db>)>>,
    /// Model ids with a download in flight, so concurrent `download_asr_model`
    /// calls for the same model are rejected rather than racing on files.
    pub model_downloads: std::sync::Mutex<std::collections::HashSet<String>>,
    /// True while an import is transcribing, so concurrent imports (and
    /// imports during recordings) are rejected.
    pub importing: Arc<std::sync::atomic::AtomicBool>,
    /// True when the current recording was started by meeting detection (or
    /// by accepting its prompt) — only such recordings may auto-stop.
    pub auto_started: std::sync::atomic::AtomicBool,
    /// The active session provider's `labels_authoritative` capability,
    /// snapshotted at start so `stop_recording` can tell the finalize
    /// pipeline whether provider labels must be kept or re-diarized.
    pub labels_authoritative: std::sync::atomic::AtomicBool,
    /// Live speaker renames (pill edits during the recording): old label →
    /// user-given name. Applied to already-accumulated segments when set and
    /// to every later segment by the event forwarder; cleared at start.
    pub live_label_renames:
        tokio::sync::Mutex<std::collections::HashMap<String, String>>,
    /// User-starred moments (seconds into the recording), accumulated by
    /// `star_moment` and drained by `stop_recording`; cleared at start.
    pub stars: tokio::sync::Mutex<Vec<f64>>,
    /// Where each star sits in the user's notes, sent by the frontend on
    /// `recording-stopped` (`set_star_anchors`) and merged into the stars
    /// before they persist; cleared at start.
    pub star_anchors: tokio::sync::Mutex<Vec<commands::Star>>,
    /// True after the user dismissed the "call detected" prompt; suppresses
    /// re-prompting until the current call ends.
    pub detection_dismissed: std::sync::atomic::AtomicBool,
    /// True while a voice-reference enrollment capture is running (they don't
    /// coexist with recordings — both want the mic).
    pub enrolling: std::sync::atomic::AtomicBool,
    /// Set to stop the running enrollment capture early.
    pub enroll_cancel: Arc<std::sync::atomic::AtomicBool>,
    /// The built-in LLM child process (llama-server), started on demand.
    pub llm: llm::LlmSidecar,
    /// The search-index runtime: the embed child process (`embral-mcp
    /// embed`) and the worker's wake-up bell.
    pub search: search_index::SearchRuntime,
    /// The running dictation session, if any.
    pub dictation: tokio::sync::Mutex<Option<dictation::ActiveDictation>>,
    /// Mirror of `dictation.is_some()` readable from sync contexts (the
    /// global-shortcut handler decides tap-vs-hold without locking).
    pub dictating: std::sync::atomic::AtomicBool,
    /// When the dictation hotkey press that started the session happened.
    pub dictation_pressed_at: std::sync::Mutex<Option<std::time::Instant>>,
}

impl AppState {
    pub fn new(config: embral_types::AppConfig) -> Self {
        Self {
            recorder: tokio::sync::Mutex::new(None),
            session: tokio::sync::Mutex::new(None),
            config: tokio::sync::Mutex::new(config),
            current_segments: Arc::new(tokio::sync::Mutex::new(Vec::new())),
            engine: Arc::new(embral_engine::Engine::new()),
            db: tokio::sync::Mutex::new(None),
            model_downloads: std::sync::Mutex::new(std::collections::HashSet::new()),
            importing: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            auto_started: std::sync::atomic::AtomicBool::new(false),
            labels_authoritative: std::sync::atomic::AtomicBool::new(false),
            live_label_renames: tokio::sync::Mutex::new(std::collections::HashMap::new()),
            stars: tokio::sync::Mutex::new(Vec::new()),
            star_anchors: tokio::sync::Mutex::new(Vec::new()),
            detection_dismissed: std::sync::atomic::AtomicBool::new(false),
            enrolling: std::sync::atomic::AtomicBool::new(false),
            enroll_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            llm: llm::LlmSidecar::default(),
            search: search_index::SearchRuntime::default(),
            dictation: tokio::sync::Mutex::new(None),
            dictating: std::sync::atomic::AtomicBool::new(false),
            dictation_pressed_at: std::sync::Mutex::new(None),
        }
    }

    /// Clone of the importing flag for a background task's drop guard.
    pub fn importing_handle(&self) -> Arc<std::sync::atomic::AtomicBool> {
        self.importing.clone()
    }

    /// The database for the *currently configured* storage dir, opening (and
    /// importing any legacy index.json) on first use or after the dir changes.
    pub async fn db(&self) -> Result<Arc<embral_db::Db>, String> {
        let base = {
            let config = self.config.lock().await;
            storage::storage_base(&config.storage_dir)
        };
        let mut guard = self.db.lock().await;
        if let Some((open_base, db)) = guard.as_ref() {
            if *open_base == base {
                return Ok(db.clone());
            }
        }
        let db = storage::open_db(&base).map_err(|e| e.to_string())?;
        let db = Arc::new(db);
        *guard = Some((base, db.clone()));
        Ok(db)
    }
}

/// `%LOCALAPPDATA%/embral/logs` — next to the models dir, not user data.
pub fn logs_dir() -> std::path::PathBuf {
    dirs::data_local_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("embral")
        .join("logs")
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Default to `info`: a clean recording emits only the standardized
    // per-session spine (connect → ready → ~20s heartbeat → finish) plus any
    // warn/error. The per-message/per-frame firehose lives at `trace` — opt in
    // with e.g. `RUST_LOG=embral_lib=trace` for deep protocol debugging.
    //
    // Logs go to stderr AND a daily-rolling file under
    // `%LOCALAPPDATA%/embral/logs` (surfaced via Settings → About → Open logs
    // folder) so users can attach them to bug reports.
    {
        use tracing_subscriber::layer::SubscriberExt;
        use tracing_subscriber::util::SubscriberInitExt;

        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

        let logs_dir = logs_dir();
        let _ = std::fs::create_dir_all(&logs_dir);
        let file_appender = tracing_appender::rolling::daily(&logs_dir, "embral.log");
        let (file_writer, guard) = tracing_appender::non_blocking(file_appender);
        // The guard flushes on drop; the app runs for the process lifetime,
        // so parking it forever is the correct lifetime.
        Box::leak(Box::new(guard));

        tracing_subscriber::registry()
            .with(filter)
            .with(tracing_subscriber::fmt::layer())
            .with(
                tracing_subscriber::fmt::layer()
                    .with_writer(file_writer)
                    .with_ansi(false),
            )
            .init();
    }
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        "embral starting"
    );
    let config = config::load_config().unwrap_or_default();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(hotkey::plugin())
        .manage(AppState::new(config))
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .setup(|app| {
            tray::create_tray(app)?;
            // The app always lives in the tray: the window never opens on
            // launch (users open it from the tray icon), and launch-at-login
            // is always on — both by design, neither is a setting.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.hide();
            }
            {
                use tauri_plugin_autostart::ManagerExt;
                let autostart = app.autolaunch();
                if !autostart.is_enabled().unwrap_or(false) {
                    if let Err(e) = autostart.enable() {
                        tracing::warn!("failed to enable launch at login: {e}");
                    }
                }
            }
            // Audio janitor: prune old audio per the retention setting, on
            // startup and every 12 hours. Reads the config each tick so a
            // changed setting applies without restart; skips when disabled.
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        let state = handle.state::<AppState>();
                        let (days, meeting_days, dictation_days, dictation_count, base) = {
                            let config = state.config.lock().await;
                            // Both dictation criteria sit behind the one
                            // auto-delete switch; either at 0 is off.
                            let (d_days, d_count) = if config.dictation_auto_delete {
                                (config.dictation_retention_days, config.dictation_retention_count)
                            } else {
                                (0, 0)
                            };
                            (
                                config.audio_retention_days,
                                config.meeting_retention_days,
                                d_days,
                                d_count,
                                storage::storage_base(&config.storage_dir),
                            )
                        };
                        if days > 0 || meeting_days > 0 || dictation_days > 0 || dictation_count > 0 {
                            match state.db().await {
                                Ok(db) => {
                                    if meeting_days > 0 {
                                        match storage::prune_old_meetings(&db, &base, meeting_days)
                                        {
                                            Ok(n) if n > 0 => {
                                                tracing::info!(pruned = n, "janitor removed old meetings")
                                            }
                                            Ok(_) => {}
                                            Err(e) => tracing::warn!("meeting janitor failed: {e}"),
                                        }
                                    }
                                    if days > 0 {
                                        match storage::prune_old_audio(&db, &base, days) {
                                            Ok(n) if n > 0 => {
                                                tracing::info!(pruned = n, "janitor removed old audio")
                                            }
                                            Ok(_) => {}
                                            Err(e) => tracing::warn!("janitor failed: {e}"),
                                        }
                                    }
                                    match db.prune_dictations(dictation_days) {
                                        Ok(n) if n > 0 => {
                                            tracing::info!(pruned = n, "janitor removed old dictations")
                                        }
                                        Ok(_) => {}
                                        Err(e) => tracing::warn!("dictation janitor failed: {e}"),
                                    }
                                    match db.prune_dictations_beyond(dictation_count) {
                                        Ok(n) if n > 0 => {
                                            tracing::info!(
                                                pruned = n,
                                                "janitor trimmed dictations beyond the keep-count"
                                            )
                                        }
                                        Ok(_) => {}
                                        Err(e) => tracing::warn!("dictation janitor failed: {e}"),
                                    }
                                    // Pruned owners cascade their chunks; the
                                    // vectors are orphans until swept.
                                    search_index::after_delete(&db);
                                }
                                Err(e) => tracing::warn!("janitor could not open db: {e}"),
                            }
                        }
                        tokio::time::sleep(std::time::Duration::from_secs(12 * 3600)).await;
                    }
                });
            }
            // Built-in LLM idle eviction: check every minute; the sidecar
            // frees ~3 GB of RAM when it hasn't been used for a while.
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                        let state = handle.state::<AppState>();
                        let (keep_warm, idle_minutes) = {
                            let config = state.config.lock().await;
                            // Keep-warm only means something while a summary
                            // or cleanup engine actually lives on the device;
                            // otherwise a one-off use must not pin ~3 GB.
                            (
                                config.llm_keep_warm && llm::uses_local_llm(&config),
                                config.llm_idle_minutes,
                            )
                        };
                        state.llm.evict_if_idle(keep_warm, idle_minutes);
                    }
                });
            }
            // Meeting auto-detection poller (policy-gated internally).
            autodetect::spawn(app.handle().clone());
            // Search-index worker: backfills chunks at boot, embeds pending
            // passages whenever a mutation pings it.
            search_index::spawn_worker(app.handle().clone());
            // Register the record + dictation hotkeys from config (empty = none).
            {
                let (record, dictation) = {
                    let state = app.state::<AppState>();
                    let config = state.config.blocking_lock();
                    (config.record_hotkey.clone(), config.dictation_hotkey.clone())
                };
                if let Err(e) = hotkey::apply(app.handle(), &record, &dictation) {
                    tracing::warn!("{e}");
                }
            }
            Ok(())
        })
        .invoke_handler(app_handler())
        .build(tauri::generate_context!())
        .expect("error while running tauri application")
        .run(|app, event| {
            // Don't orphan the llama-server child when the app quits. (The
            // embed child also exits on its own when its stdin closes.)
            if let tauri::RunEvent::Exit = event {
                app.state::<AppState>().llm.shutdown();
                app.state::<AppState>().search.shutdown_blocking();
            }
        });
}

// `generate_handler!` can't cfg individual entries, so the shared list
// lives in one macro and the cloud build appends its commands.
macro_rules! app_handler_with {
    ($($extra:path),* $(,)?) => {
        tauri::generate_handler![
            commands::reset_app_data,
            commands::start_recording,
            commands::pause_recording,
            commands::resume_recording,
            commands::rename_live_speaker,
            commands::star_moment,
            commands::unstar_moment,
            commands::set_star_anchors,
            commands::stop_recording,
            commands::import_recording,
            commands::accept_detected_meeting,
            commands::dismiss_detected_meeting,
            commands::get_meetings,
            commands::get_meeting_records,
            commands::get_meeting,
            commands::get_meeting_detail,
            commands::update_meeting_title,
            commands::update_meeting_notes,
            commands::update_meeting_transcript,
            commands::delete_meeting,
            commands::delete_meetings,
            commands::search_library,
            commands::get_config,
            commands::save_config,
            commands::open_notes_folder,
            commands::list_audio_devices,
            commands::preview_export_filename,
            commands::test_webhook,
            commands::open_logs_folder,
            commands::update_guard,
            mcp_clients::mcp_setup_info,
            mcp_clients::mcp_clients_status,
            mcp_clients::mcp_register,
            mcp_clients::mcp_unregister,
            commands::asr_models_status,
            commands::download_asr_model,
            commands::delete_asr_model,
            commands::llm_status,
            commands::get_summary_prompt_parts,
            commands::start_dictation,
            commands::stop_dictation,
            commands::cancel_dictation,
            commands::list_dictations,
            commands::delete_dictation,
            speaker_commands::list_speakers,
            speaker_commands::upsert_speaker,
            speaker_commands::delete_speaker,
            speaker_commands::delete_speakers,
            speaker_commands::record_voice_reference,
            speaker_commands::cancel_voice_reference,
            speaker_commands::delete_voice_reference,
            speaker_commands::confirm_speaker_suggestion,
            speaker_commands::dismiss_speaker_suggestion,
            speaker_commands::edit_segments,
            $($extra),*
        ]
    };
}

#[cfg(feature = "cloud")]
fn app_handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    app_handler_with![
        cloud::commands::cloud_request_code,
        cloud::commands::cloud_verify_code,
        cloud::commands::cloud_account_status,
        cloud::commands::cloud_sign_out,
        cloud::commands::cloud_revoke_device,
        cloud::commands::cloud_billing_url,
        cloud::commands::cloud_billing_tiers,
        cloud::commands::cloud_adopt_provider,
    ]
}

#[cfg(not(feature = "cloud"))]
fn app_handler() -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    app_handler_with![]
}
