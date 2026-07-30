//! Thin adapter over the `embral-notes` crate.
//!
//! All note-generation logic (the prompt, the transport, title + filename
//! helpers) now lives in `embral-notes`, where it is unit-tested without the
//! Tauri toolchain. This module only maps `AppConfig` into the crate's narrow
//! `NotesConfig` and re-exports the helpers the rest of the app already
//! imports as `crate::refinement::…`.

use anyhow::Result;
use embral_types::{AppConfig, LlmProfile};

pub use embral_notes::{apply_title, extract_title, sanitize_filename};

/// Resolve an engine id against this build's fixed engine list; "" = none.
pub fn profile_by_id(_config: &AppConfig, id: &str) -> Option<LlmProfile> {
    if id.trim().is_empty() {
        return None;
    }
    embral_types::available_profiles().into_iter().find(|p| p.id == id)
}

/// The engine that writes meeting summaries; `None` when summaries are off.
pub fn summaries_profile(config: &AppConfig) -> Option<LlmProfile> {
    // The switch is the product decision ("should meetings be summarized"),
    // the engine only answers "by what". Off means off, whatever the engine.
    if !config.summaries_enabled {
        return None;
    }
    profile_by_id(config, &config.summaries_profile_id)
}

/// Map one profile to the notes crate's transport config. The built-in
/// provider's endpoint is resolved by the caller (it needs the running
/// sidecar's port) before this config is used.
pub fn notes_config(profile: &LlmProfile) -> embral_notes::NotesConfig {
    embral_notes::NotesConfig {
        provider: profile.provider,
        model: profile.model.clone(),
        endpoint: profile.endpoint.clone(),
        api_key: profile.api_key.clone(),
    }
}

/// Run best-effort post-meeting integrations (the Markdown export). Gated on
/// its config fields; failures are logged, never propagated — a broken vault
/// path must not affect the saved meeting. `export_md` is the composed
/// document ([integrations.md]).
pub fn run_post_meeting_integrations(
    config: &AppConfig,
    record: &embral_types::MeetingRecord,
    export_md: &str,
) {
    if config.obsidian_export_enabled && !config.obsidian_vault_dir.trim().is_empty() {
        let base = crate::storage::storage_base(&config.storage_dir);
        match embral_notes::integrations::export_to_obsidian(
            &config.obsidian_vault_dir,
            record,
            export_md,
            &config.export_filename_template,
            config.export_metadata_format,
            Some(&base),
        ) {
            Ok(path) => tracing::info!("Obsidian export written to {}", path.display()),
            Err(e) => tracing::warn!("Obsidian export failed: {}", e),
        }
    }
}

/// Generate structured notes from the (timestamped) transcript using the
/// given profile. `notes_cfg` arrives pre-built so the caller can resolve the
/// built-in provider's runtime endpoint first.
#[allow(clippy::too_many_arguments)]
pub async fn refine_notes(
    notes_cfg: &embral_notes::NotesConfig,
    config: &AppConfig,
    meeting_id: &str,
    start_time: &str,
    duration_minutes: u32,
    meeting_title: Option<&str>,
    transcript: &str,
    user_notes: Option<&str>,
    image_text: &[(String, String)],
) -> Result<String> {
    embral_notes::refine_notes(
        notes_cfg,
        meeting_id,
        start_time,
        duration_minutes,
        meeting_title,
        transcript,
        user_notes,
        &config.summary_prompt,
        image_text,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use embral_types::BUILTIN_PROFILE_ID;

    #[test]
    fn summaries_switch_overrides_the_engine() {
        let mut config = AppConfig::default();
        config.summaries_profile_id = BUILTIN_PROFILE_ID.to_string();

        config.summaries_enabled = false;
        assert!(
            summaries_profile(&config).is_none(),
            "off means off, even with an engine selected"
        );

        config.summaries_enabled = true;
        assert_eq!(
            summaries_profile(&config).map(|p| p.id),
            Some(BUILTIN_PROFILE_ID.to_string())
        );
    }
}
