//! Tauri commands for the speaker registry, notes-derived name
//! suggestions, and segment-level transcript editing.

use std::path::Path;

use embral_db::{Db, SpeakerRow};
use embral_notes::transcript::{self, format_transcript};
use tauri::State;

use crate::commands::{
    fallback_duration_minutes, format_transcript_document, meeting_detail, require_row,
    write_indexed_text, MeetingDetail,
};
use crate::AppState;

// --- Payloads ---------------------------------------------------------------

/// A registry person plus everything the Profiles page shows.
#[derive(serde::Serialize)]
pub struct SpeakerProfile {
    pub id: String,
    pub name: String,
    pub notes: String,
    /// When this person was added.
    pub created_at: String,
    /// The newest meeting they were in; `None` if they have never been in one.
    /// The list sorts and groups on this (falling back to `created_at`), so its
    /// date headers read as "people you met with today".
    pub last_seen: Option<String>,
}

/// A pending notes-derived name suggestion as the frontend sees it.
#[derive(serde::Serialize)]
pub struct NameSuggestionView {
    pub label: String,
    pub name: String,
}

/// One transcript edit operation.
#[derive(serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SegmentEdit {
    Split { index: usize, char_offset: usize },
    Delete { index: usize },
    /// Set one segment's speaker; `speaker_id` links it to a registry person.
    Reassign {
        index: usize,
        speaker: String,
        speaker_id: Option<String>,
    },
    /// Rename/merge a label across the whole meeting.
    RelabelAll {
        from: String,
        to: String,
        speaker_id: Option<String>,
    },
    /// Remove a label from the meeting: every segment carrying it becomes
    /// unattributed (pill right-click).
    ClearLabel { label: String },
}

/// How a segment edit ripples into the meeting's attendee list.
pub(crate) enum AttendeeFix<'a> {
    /// A rename/merge: swap the old name for the new one.
    Swap(&'a str, &'a str),
    /// A deleted label: drop the name entirely.
    Remove(&'a str),
}

// --- Shared helpers ----------------------------------------------------------

pub(crate) fn name_suggestion_views(
    db: &Db,
    meeting_id: &str,
) -> Result<Vec<NameSuggestionView>, String> {
    Ok(load_name_suggestions(db, meeting_id)?
        .into_iter()
        .map(|s| NameSuggestionView {
            label: s.label,
            name: s.name,
        })
        .collect())
}

fn load_name_suggestions(
    db: &Db,
    meeting_id: &str,
) -> Result<Vec<crate::notes_matching::NameSuggestion>, String> {
    let json = db
        .get_name_suggestions(meeting_id)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::from_str(&json).unwrap_or_default())
}

fn save_name_suggestions(
    db: &Db,
    meeting_id: &str,
    suggestions: &[crate::notes_matching::NameSuggestion],
) -> Result<(), String> {
    let json = serde_json::to_string(suggestions).map_err(|e| e.to_string())?;
    db.set_name_suggestions(meeting_id, &json)
        .map_err(|e| e.to_string())
}

/// Drop pending suggestions for a label that no longer exists (cleared or
/// manually renamed) — a stale suggestion's Apply would rename nothing.
fn prune_name_suggestions(db: &Db, meeting_id: &str, label: &str) -> Result<(), String> {
    let mut suggestions = load_name_suggestions(db, meeting_id)?;
    let before = suggestions.len();
    suggestions.retain(|s| s.label != label);
    if suggestions.len() != before {
        save_name_suggestions(db, meeting_id, &suggestions)?;
    }
    Ok(())
}

/// Rebuild a meeting's transcript document (markdown + file + index export)
/// from its current segments, and return the fresh detail payload. The
/// attendee list is fixed up along the way per `fix`. Every caller is a
/// text mutation, so the search index re-syncs here too — one shared spot,
/// like the export.
fn regenerate_transcript(
    db: &Db,
    runtime: &crate::search_index::SearchRuntime,
    base: &Path,
    meeting_id: &str,
    fix: Option<AttendeeFix<'_>>,
) -> Result<MeetingDetail, String> {
    let mut row = require_row(db, meeting_id)?;
    let segments = db.get_segments(meeting_id).map_err(|e| e.to_string())?;
    let record = row.to_record();
    let start_time = record
        .date
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

    match fix {
        Some(AttendeeFix::Swap(from, to)) => {
            for name in row.attendees.iter_mut() {
                if name == from {
                    *name = to.to_string();
                }
            }
            row.attendees.dedup();
        }
        Some(AttendeeFix::Remove(name)) => {
            row.attendees.retain(|n| n != name);
        }
        None => {}
    }

    row.transcript_md = format_transcript_document(
        &row.title,
        meeting_id,
        &start_time,
        fallback_duration_minutes(&record),
        &row.attendees,
        &format_transcript(&segments),
    );
    write_indexed_text(base, &row.transcript_path, &row.transcript_md)?;
    db.upsert_meeting(&row).map_err(|e| e.to_string())?;
    crate::storage::export_index(db, base).map_err(|e| e.to_string())?;
    crate::search_index::sync_meeting(db, runtime, meeting_id);
    meeting_detail(db, base, row)
}

/// Build the frontend profile for one person. Takes the *activity* row so the
/// list's ordering data (created/last seen) rides along with the identity.
fn profile(list_row: embral_db::SpeakerListRow) -> SpeakerProfile {
    let row = list_row.speaker;
    SpeakerProfile {
        id: row.id,
        name: row.name,
        notes: row.notes,
        created_at: list_row.created_at.to_rfc3339(),
        last_seen: list_row.last_seen.map(|dt| dt.to_rfc3339()),
    }
}

/// One person's profile as the frontend sees it, fetched fresh — used by the
/// commands that return the profile they just changed.
fn profile_by_id(db: &Db, id: &str) -> Result<SpeakerProfile, String> {
    let row = db
        .speaker_by_activity(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Profile {id} not found"))?;
    Ok(profile(row))
}

async fn storage_ctx(state: &State<'_, AppState>) -> Result<(std::path::PathBuf, std::sync::Arc<Db>), String> {
    let config = state.config.lock().await.clone();
    let base = crate::storage::storage_base(&config.storage_dir);
    let db = state.db().await?;
    Ok((base, db))
}

// --- Registry ---------------------------------------------------------------

/// The registry as the Profiles page lists it: newest activity first, so the
/// page's date headers mean "who you last met with".
#[tauri::command]
pub async fn list_speakers(state: State<'_, AppState>) -> Result<Vec<SpeakerProfile>, String> {
    let (_, db) = storage_ctx(&state).await?;
    Ok(db
        .list_speakers_by_activity()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(profile)
        .collect())
}

/// Create or update a person. A rename relabels their linked segments across
/// meetings and regenerates those meetings' transcript documents.
#[tauri::command]
pub async fn upsert_speaker(
    state: State<'_, AppState>,
    id: Option<String>,
    name: String,
    notes: String,
) -> Result<SpeakerProfile, String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("Speaker name cannot be empty".to_string());
    }
    let (base, db) = storage_ctx(&state).await?;

    let row = SpeakerRow {
        id: id.unwrap_or_else(|| format!("sp_{}", uuid::Uuid::new_v4().simple())),
        name: name.clone(),
        notes: notes.trim().to_string(),
    };

    let previous = db.get_speaker(&row.id).map_err(|e| e.to_string())?;
    db.upsert_speaker(&row).map_err(|e| e.to_string())?;

    if let Some(prev) = previous.filter(|p| p.name != name) {
        let affected = db
            .relabel_speaker_segments(&row.id, &name)
            .map_err(|e| e.to_string())?;
        for meeting_id in affected {
            regenerate_transcript(
                &db,
                &state.search,
                &base,
                &meeting_id,
                Some(AttendeeFix::Swap(prev.name.as_str(), &name)),
            )?;
        }
    }
    profile_by_id(&db, &row.id)
}

/// Remove a person. Transcript labels survive as plain text.
#[tauri::command]
pub async fn delete_speaker(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let (_, db) = storage_ctx(&state).await?;
    db.delete_speaker(&id).map_err(|e| e.to_string())?;
    Ok(())
}

/// Remove several people at once (the list's multi-select). Same rule as the
/// single delete: transcript labels survive as plain text.
#[tauri::command]
pub async fn delete_speakers(state: State<'_, AppState>, ids: Vec<String>) -> Result<(), String> {
    let (_, db) = storage_ctx(&state).await?;
    for id in &ids {
        db.delete_speaker(id).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// --- Notes-derived name suggestions ------------------------------------------

/// "Yes, Speaker N is this person": link (or create) the profile, relabel
/// the meeting's segments, and regenerate the transcript document.
#[tauri::command]
pub async fn confirm_name_suggestion(
    state: State<'_, AppState>,
    meeting_id: String,
    label: String,
    name: String,
) -> Result<MeetingDetail, String> {
    crate::telemetry::track(
        &state,
        "name_suggestion",
        serde_json::json!({ "action": "confirmed" }),
    );
    let (base, db) = storage_ctx(&state).await?;
    let mut suggestions = load_name_suggestions(&db, &meeting_id)?;
    let Some(pos) = suggestions
        .iter()
        .position(|s| s.label == label && s.name == name)
    else {
        return Err("That suggestion is no longer pending".to_string());
    };
    suggestions.remove(pos);
    suggestions.retain(|s| s.label != label);

    // Find the registry person by name, or create them — the user's
    // approval is the explicit intent that justifies a new profile.
    let speaker_id = match db
        .list_speakers()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|p| p.name.eq_ignore_ascii_case(&name))
    {
        Some(person) => person.id,
        None => {
            let row = SpeakerRow {
                id: format!("sp_{}", uuid::Uuid::new_v4().simple()),
                name: name.clone(),
                notes: String::new(),
            };
            db.upsert_speaker(&row).map_err(|e| e.to_string())?;
            row.id
        }
    };
    db.assign_speaker_label(&meeting_id, &label, &speaker_id, &name)
        .map_err(|e| e.to_string())?;
    save_name_suggestions(&db, &meeting_id, &suggestions)?;
    regenerate_transcript(
        &db,
        &state.search,
        &base,
        &meeting_id,
        Some(AttendeeFix::Swap(label.as_str(), &name)),
    )
}

#[tauri::command]
pub async fn dismiss_name_suggestion(
    state: State<'_, AppState>,
    meeting_id: String,
    label: String,
) -> Result<MeetingDetail, String> {
    crate::telemetry::track(
        &state,
        "name_suggestion",
        serde_json::json!({ "action": "dismissed" }),
    );
    let (base, db) = storage_ctx(&state).await?;
    let mut suggestions = load_name_suggestions(&db, &meeting_id)?;
    suggestions.retain(|s| s.label != label);
    save_name_suggestions(&db, &meeting_id, &suggestions)?;
    meeting_detail(&db, &base, require_row(&db, &meeting_id)?)
}

// --- Segment editing -----------------------------------------------------------

/// Apply one edit to a meeting's structured transcript, then regenerate the
/// transcript document and exports.
#[tauri::command]
pub async fn edit_segments(
    state: State<'_, AppState>,
    meeting_id: String,
    edit: SegmentEdit,
) -> Result<MeetingDetail, String> {
    let kind = match &edit {
        SegmentEdit::Split { .. } => "split",
        SegmentEdit::Delete { .. } => "delete",
        SegmentEdit::Reassign { .. } => "reassign",
        SegmentEdit::RelabelAll { .. } => "relabel_all",
        SegmentEdit::ClearLabel { .. } => "clear_label",
    };
    crate::telemetry::track(&state, "segments_edited", serde_json::json!({ "kind": kind }));
    let (base, db) = storage_ctx(&state).await?;
    let mut segments = db.get_segments(&meeting_id).map_err(|e| e.to_string())?;
    if segments.is_empty() {
        return Err("This meeting has no structured transcript to edit".to_string());
    }

    let mut swap: Option<(String, String)> = None;
    let mut removed: Option<String> = None;
    match edit {
        SegmentEdit::Split { index, char_offset } => {
            transcript::split_segment(&mut segments, index, char_offset);
        }
        SegmentEdit::Delete { index } => {
            transcript::delete_segment(&mut segments, index);
        }
        SegmentEdit::Reassign {
            index,
            speaker,
            speaker_id,
        } => {
            transcript::reassign_speaker(&mut segments, index, &speaker);
            if let Some(seg) = segments.get_mut(index) {
                seg.speaker_id = speaker_id.filter(|id| !id.is_empty());
            }
        }
        SegmentEdit::RelabelAll {
            from,
            to,
            speaker_id,
        } => {
            let to = to.trim().to_string();
            if to.is_empty() {
                return Err("Speaker name cannot be empty".to_string());
            }
            let speaker_id = speaker_id.filter(|id| !id.is_empty());
            for seg in segments.iter_mut() {
                if seg.speaker.as_deref() == Some(from.as_str()) {
                    seg.speaker = Some(to.clone());
                    seg.speaker_id = speaker_id.clone();
                }
            }
            prune_name_suggestions(&db, &meeting_id, &from)?;
            swap = Some((from, to));
        }
        SegmentEdit::ClearLabel { label } => {
            for seg in segments.iter_mut() {
                if seg.speaker.as_deref() == Some(label.as_str()) {
                    seg.speaker = None;
                    seg.speaker_id = None;
                }
            }
            prune_name_suggestions(&db, &meeting_id, &label)?;
            removed = Some(label);
        }
    }

    db.replace_segments(&meeting_id, &segments)
        .map_err(|e| e.to_string())?;
    let fix = match (&swap, &removed) {
        (Some((f, t)), _) => Some(AttendeeFix::Swap(f.as_str(), t.as_str())),
        (_, Some(name)) => Some(AttendeeFix::Remove(name.as_str())),
        _ => None,
    };
    regenerate_transcript(&db, &state.search, &base, &meeting_id, fix)
}
