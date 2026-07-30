//! OS permission checks and prompts (macOS microphone + accessibility;
//! `not_required` stubs on Windows). All paths are fully qualified.

use embral_types::AppError;
/// The microphone permission's current state, without prompting
/// ([shell.md](../../docs/shell.md) §Onboarding; `not_required` on Windows).
#[tauri::command]
pub fn mic_permission() -> crate::platform::types::PermissionState {
    crate::platform::permissions::check_microphone()
}

/// Ask the OS for the microphone; resolves when the user answers (prompts
/// only from the not-determined state — a denial needs System Settings).
#[tauri::command]
pub async fn request_mic_permission() -> Result<crate::platform::types::PermissionState, AppError> {
    Ok(crate::platform::permissions::request_microphone().await)
}

/// Whether synthetic keystrokes (dictation auto-paste) are allowed
/// ([dictation.md](../../docs/dictation.md); `not_required` on Windows).
#[tauri::command]
pub fn accessibility_permission() -> crate::platform::types::PermissionState {
    crate::platform::permissions::check_accessibility()
}

/// Show the OS's one-time Accessibility prompt; the grant itself happens
/// in System Settings, so the returned state is whatever is true now.
#[tauri::command]
pub fn request_accessibility_permission() -> crate::platform::types::PermissionState {
    crate::platform::permissions::request_accessibility()
}
