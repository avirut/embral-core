//! Where the library lives and how the server reaches it.
//!
//! The app is the only writer; this process opens the database **read-only,
//! per tool call** — the server outlives the app (clients keep it running),
//! and a held handle would block the app's own storage resets on Windows.

use std::path::PathBuf;

use embral_db::Db;

pub struct Store {
    pub storage_dir: PathBuf,
}

impl Store {
    /// `EMBRAL_STORAGE_DIR` (non-empty) wins — it's what the `.mcpb`
    /// user_config feeds — else the app's own `config.json` in the default
    /// storage location, else that default itself.
    pub fn from_env() -> Store {
        if let Ok(dir) = std::env::var("EMBRAL_STORAGE_DIR") {
            if !dir.trim().is_empty() {
                return Store {
                    storage_dir: embral_types::resolve_storage_path(dir.trim()),
                };
            }
        }
        let default_base = embral_types::resolve_storage_path(&embral_types::default_storage_dir());
        let configured = std::fs::read_to_string(embral_types::config_path(&default_base))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .and_then(|cfg| cfg["storage_dir"].as_str().map(str::to_string))
            .filter(|dir| !dir.trim().is_empty());
        Store {
            storage_dir: configured
                .map(|dir| embral_types::resolve_storage_path(&dir))
                .unwrap_or(default_base),
        }
    }

    pub fn db_path(&self) -> PathBuf {
        self.storage_dir.join("embral.db")
    }

    /// Open the database read-only, refusing schemas older than this build
    /// (the app migrates on open — running it once catches the library up).
    /// Newer schemas proceed: migrations have been additive.
    pub fn open(&self) -> Result<Db, ToolError> {
        let db_path = self.db_path();
        if !db_path.is_file() {
            return Err(ToolError::StorageNotFound { db_path });
        }
        let db = Db::open_read_only(&db_path).map_err(ToolError::Db)?;
        let found = db.schema_version().map_err(ToolError::Db)?;
        let expected = embral_db::latest_schema_version();
        if found < expected {
            return Err(ToolError::SchemaMismatch { found, expected });
        }
        Ok(db)
    }
}

/// Tool failures the calling model can react to — rendered as execution
/// results (`is_error`), never protocol errors, in the envelope
/// `{ok:false, error:{code, message}}`.
#[derive(Debug)]
pub enum ToolError {
    StorageNotFound { db_path: PathBuf },
    MeetingNotFound { id: String },
    PassageNotFound { id: i64 },
    InvalidArgument { message: String },
    SchemaMismatch { found: i64, expected: i64 },
    Db(anyhow::Error),
}

impl ToolError {
    pub fn code(&self) -> &'static str {
        match self {
            ToolError::StorageNotFound { .. } => "STORAGE_NOT_FOUND",
            ToolError::MeetingNotFound { .. } => "MEETING_NOT_FOUND",
            ToolError::PassageNotFound { .. } => "PASSAGE_NOT_FOUND",
            ToolError::InvalidArgument { .. } => "INVALID_ARGUMENT",
            ToolError::SchemaMismatch { .. } => "SCHEMA_MISMATCH",
            ToolError::Db(_) => "DB_ERROR",
        }
    }

    pub fn message(&self) -> String {
        match self {
            ToolError::StorageNotFound { db_path } => format!(
                "No embral library at {} — record a meeting in embral first, \
                 or point EMBRAL_STORAGE_DIR at the storage folder.",
                db_path.display()
            ),
            ToolError::MeetingNotFound { id } => {
                format!("No meeting with id '{id}' — ids come from list_meetings or search_meetings.")
            }
            ToolError::PassageNotFound { id } => format!(
                "No passage {id} — re-run search; passage ids change when a \
                 meeting is edited."
            ),
            ToolError::InvalidArgument { message } => message.clone(),
            ToolError::SchemaMismatch { found, expected } => format!(
                "The library's schema is v{found} but this server expects v{expected} — \
                 open the embral app once to update it, then retry."
            ),
            ToolError::Db(e) => format!("Database error: {e:#}"),
        }
    }
}

/// The one embedder this process holds, loaded lazily and retried gently.
/// Every failure shape degrades to `None` — search stays keyword-accurate
/// with no semantic leg, never erroring because a model is missing.
pub struct EmbedderSlot(std::sync::Mutex<SlotState>);

enum SlotState {
    NotLoaded,
    Loaded(Box<embral_embedder::Embedder>),
    /// Load or inference failed; retried after a cooldown so a model
    /// downloaded mid-session gets picked up without a restart.
    Failed(std::time::Instant),
}

const RETRY_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(5 * 60);

impl Default for EmbedderSlot {
    fn default() -> Self {
        EmbedderSlot(std::sync::Mutex::new(SlotState::NotLoaded))
    }
}

impl EmbedderSlot {
    pub fn embed_query(&self, text: &str) -> Option<Vec<f32>> {
        let mut slot = self.0.lock().expect("embedder slot poisoned");
        if let SlotState::Failed(at) = &*slot {
            if at.elapsed() < RETRY_COOLDOWN {
                return None;
            }
            *slot = SlotState::NotLoaded;
        }
        if matches!(&*slot, SlotState::NotLoaded) {
            if !embral_search::model::present() {
                return None;
            }
            match embral_embedder::Embedder::load_default() {
                Ok(embedder) => *slot = SlotState::Loaded(Box::new(embedder)),
                Err(e) => {
                    tracing::warn!("embedding model failed to load: {e:#}");
                    *slot = SlotState::Failed(std::time::Instant::now());
                    return None;
                }
            }
        }
        let SlotState::Loaded(embedder) = &mut *slot else {
            return None;
        };
        match embedder.embed_query(text) {
            Ok(vector) => Some(vector),
            Err(e) => {
                tracing::warn!("query embedding failed: {e:#}");
                *slot = SlotState::Failed(std::time::Instant::now());
                None
            }
        }
    }

    /// For `get_storage_status` — whether semantic search is live right now.
    pub fn loaded(&self) -> bool {
        matches!(&*self.0.lock().expect("embedder slot poisoned"), SlotState::Loaded(_))
    }
}
