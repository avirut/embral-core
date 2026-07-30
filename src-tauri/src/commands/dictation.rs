//! Thin passthroughs to the dictation session (`crate::dictation`) plus the
//! dictation-history list and delete.

use embral_types::AppError;
use tauri::{AppHandle, State};

use crate::AppState;

// --- Dictation ---

#[tauri::command]
pub async fn start_dictation(app: AppHandle) -> Result<(), AppError> {
    crate::dictation::start(&app).await
}

#[tauri::command]
pub async fn stop_dictation(app: AppHandle) -> Result<String, AppError> {
    crate::dictation::stop(&app).await
}

#[tauri::command]
pub async fn cancel_dictation(app: AppHandle) -> Result<(), AppError> {
    crate::dictation::cancel(&app).await
}

#[tauri::command]
pub async fn list_dictations(
    state: State<'_, AppState>,
    limit: Option<u32>,
) -> Result<Vec<embral_db::DictationRow>, AppError> {
    let db = state.db().await?;
    db.list_dictations(limit.unwrap_or(100)).map_err(AppError::internal)
}

#[tauri::command]
pub async fn delete_dictation(state: State<'_, AppState>, id: i64) -> Result<(), AppError> {
    let db = state.db().await?;
    db.delete_dictation(id).map_err(|e| e.to_string())?;
    crate::search_index::after_delete(&db);
    Ok(())
}
