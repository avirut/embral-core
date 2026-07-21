use anyhow::Result;
use embral_types::AppConfig;
use std::path::PathBuf;

pub fn config_file_path() -> PathBuf {
    dirs::home_dir()
        .expect("cannot find home dir")
        .join("embral")
        .join("config.json")
}

pub fn load_config() -> Result<AppConfig> {
    let path = config_file_path();
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let text = std::fs::read_to_string(&path)?;
    let mut config: AppConfig = serde_json::from_str(&text)?;
    // Older configs stored the `~` shorthand; the UI shows the OS-native
    // absolute path, so normalize once on load (resolution is unchanged).
    if config.storage_dir.starts_with('~') {
        config.storage_dir = embral_types::resolve_storage_path(&config.storage_dir)
            .to_string_lossy()
            .to_string();
    }
    // The cloud URL used to be materialized into config.json; a stored value
    // equal to the production default is that old default, not a
    // customization — clear it so `cloud_url()` can pick per build (dev
    // builds talk to the local server).
    #[cfg(feature = "cloud")]
    if config.cloud_api_url == embral_types::DEFAULT_CLOUD_URL {
        config.cloud_api_url = String::new();
    }
    Ok(config)
}

pub fn save_config(config: &AppConfig) -> Result<()> {
    let path = config_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(config)?)?;
    Ok(())
}

/// Whether the selected local model's files are actually on disk. A deleted
/// managed model degrades to a clean "not configured" gate rather than a
/// cryptic load failure when recording starts.
fn local_model_ready(config: &AppConfig) -> bool {
    embral_engine::catalog::find(&config.meeting_asr_model()).is_some_and(|m| m.present())
}

pub fn is_configured(config: &AppConfig) -> bool {
    match config.transcription_provider {
        embral_types::TranscriptionProvider::Local => local_model_ready(config),
        // Cloud needs a signed-in device — and the local model only when the
        // device is where failures land ("switch to this device"). With
        // "disable transcription" chosen, a cloud session with nothing to
        // fall back to is exactly what the user asked for: the recording
        // continues, no transcript. Hours running out degrades at the relay,
        // not here.
        #[cfg(feature = "cloud")]
        embral_types::TranscriptionProvider::Cloud => {
            !config.cloud_session_token.is_empty()
                && (config.cloud_out_of_hours == embral_types::CloudOutOfHours::Disabled
                    || local_model_ready(config))
        }
    }
}

/// What a failing cloud session does — at start (connect refused: out of
/// hours, unreachable) and mid-recording (the relay's 402 cut, a drop).
/// Pure and tested; the recording itself never stops for any of these.
#[cfg(feature = "cloud")]
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum CloudFailureAction {
    /// Swap in a local session (the "switch to this device" setting).
    SwitchToLocal,
    /// Keep recording with no transcription (the "disable transcription"
    /// setting — honored for every failure shape, not only hours: the user
    /// said this app should not transcribe on the device).
    DisableTranscription,
    /// Nothing to switch to: surface the failure.
    Fail,
}

#[cfg(feature = "cloud")]
pub fn on_cloud_failure(
    out_of_hours: embral_types::CloudOutOfHours,
    local_model_present: bool,
) -> CloudFailureAction {
    match out_of_hours {
        embral_types::CloudOutOfHours::Disabled => CloudFailureAction::DisableTranscription,
        embral_types::CloudOutOfHours::Local if local_model_present => {
            CloudFailureAction::SwitchToLocal
        }
        embral_types::CloudOutOfHours::Local => CloudFailureAction::Fail,
    }
}

#[cfg(all(test, feature = "cloud"))]
mod tests {
    use super::*;
    use embral_types::CloudOutOfHours;

    #[test]
    fn disable_wins_regardless_of_the_local_model() {
        // The user said "don't transcribe on this device" — a downloaded
        // model doesn't override that, and a missing one doesn't error.
        assert_eq!(
            on_cloud_failure(CloudOutOfHours::Disabled, true),
            CloudFailureAction::DisableTranscription
        );
        assert_eq!(
            on_cloud_failure(CloudOutOfHours::Disabled, false),
            CloudFailureAction::DisableTranscription
        );
    }

    #[test]
    fn switch_to_device_needs_the_model() {
        assert_eq!(
            on_cloud_failure(CloudOutOfHours::Local, true),
            CloudFailureAction::SwitchToLocal
        );
        assert_eq!(
            on_cloud_failure(CloudOutOfHours::Local, false),
            CloudFailureAction::Fail
        );
    }
}
