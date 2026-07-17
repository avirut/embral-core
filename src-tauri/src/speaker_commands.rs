//! Tauri commands for the speaker registry, voice-reference enrollment,
//! match-suggestion handling, and segment-level transcript editing.

use std::path::Path;
use std::sync::atomic::Ordering;

use embral_db::{Db, SpeakerRow, VoiceRefKind};
use embral_notes::transcript::{self, format_transcript};
use tauri::State;

use crate::commands::{
    fallback_duration_minutes, format_transcript_document, meeting_detail, require_row,
    resolve_indexed_path, write_indexed_text, MeetingDetail,
};
use crate::speakers::SpeakerSuggestion;
use crate::AppState;

/// Voice-reference enrollment clip length.
const ENROLL_SECS: f32 = 10.0;
/// Enrolled voice-reference slots per speaker.
pub const VOICE_SLOTS: u32 = 3;

// --- Payloads ---------------------------------------------------------------

/// One enrolled slot's state for the UI.
#[derive(serde::Serialize)]
pub struct VoiceSlotView {
    pub slot: u32,
    pub ref_id: Option<i64>,
    /// Absolute path of the clip for playback, when the file exists.
    pub clip_path: Option<String>,
}

/// A registry person plus everything the Profiles page shows.
#[derive(serde::Serialize)]
pub struct SpeakerProfile {
    pub id: String,
    pub name: String,
    pub notes: String,
    pub is_you: bool,
    pub voice_slots: Vec<VoiceSlotView>,
    pub learned_refs: usize,
    /// When this person was added.
    pub created_at: String,
    /// The newest meeting they were in; `None` if they have never been in one.
    /// The list sorts and groups on this (falling back to `created_at`), so its
    /// date headers read as "people you met with today".
    pub last_seen: Option<String>,
}

/// A pending match suggestion as the frontend sees it (no embedding payload).
#[derive(serde::Serialize)]
pub struct SuggestionView {
    pub label: String,
    pub speaker_id: String,
    pub name: String,
    pub score: f32,
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

pub(crate) fn suggestion_views(db: &Db, meeting_id: &str) -> Result<Vec<SuggestionView>, String> {
    Ok(load_suggestions(db, meeting_id)?
        .iter()
        .map(|s| SuggestionView {
            label: s.label.clone(),
            speaker_id: s.speaker_id.clone(),
            name: s.name.clone(),
            score: s.score,
        })
        .collect())
}

fn load_suggestions(db: &Db, meeting_id: &str) -> Result<Vec<SpeakerSuggestion>, String> {
    let json = db
        .get_speaker_suggestions(meeting_id)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::from_str(&json).unwrap_or_default())
}

fn save_suggestions(
    db: &Db,
    meeting_id: &str,
    suggestions: &[SpeakerSuggestion],
) -> Result<(), String> {
    let json = serde_json::to_string(suggestions).map_err(|e| e.to_string())?;
    db.set_speaker_suggestions(meeting_id, &json)
        .map_err(|e| e.to_string())
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
fn profile(db: &Db, base: &Path, list_row: embral_db::SpeakerListRow) -> Result<SpeakerProfile, String> {
    let row = list_row.speaker;
    let refs = db.list_voice_refs(&row.id).map_err(|e| e.to_string())?;
    let voice_slots = (1..=VOICE_SLOTS)
        .map(|slot| {
            let found = refs
                .iter()
                .find(|r| r.kind == VoiceRefKind::Enrolled && r.slot == Some(slot));
            let clip_path = found
                .and_then(|r| r.clip_path.as_deref())
                .and_then(|p| resolve_indexed_path(base, p).ok())
                .filter(|p| p.is_file())
                .map(|p| p.to_string_lossy().to_string());
            VoiceSlotView {
                slot,
                ref_id: found.map(|r| r.id),
                clip_path,
            }
        })
        .collect();
    let learned_refs = refs
        .iter()
        .filter(|r| r.kind == VoiceRefKind::Learned)
        .count();
    Ok(SpeakerProfile {
        id: row.id,
        name: row.name,
        notes: row.notes,
        is_you: row.is_you,
        voice_slots,
        learned_refs,
        created_at: list_row.created_at.to_rfc3339(),
        last_seen: list_row.last_seen.map(|dt| dt.to_rfc3339()),
    })
}

/// One person's profile as the frontend sees it, fetched fresh — used by the
/// commands that return the profile they just changed.
fn profile_by_id(db: &Db, base: &Path, id: &str) -> Result<SpeakerProfile, String> {
    let row = db
        .speaker_by_activity(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("Profile {id} not found"))?;
    profile(db, base, row)
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
    let (base, db) = storage_ctx(&state).await?;
    db.list_speakers_by_activity()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|row| profile(&db, &base, row))
        .collect()
}

/// Create or update a person. A rename relabels their linked segments across
/// meetings and regenerates those meetings' transcript documents.
#[tauri::command]
pub async fn upsert_speaker(
    state: State<'_, AppState>,
    id: Option<String>,
    name: String,
    notes: String,
    is_you: bool,
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
        is_you,
    };

    let previous = db.get_speaker(&row.id).map_err(|e| e.to_string())?;
    // At most one person is "you".
    if is_you {
        for other in db.list_speakers().map_err(|e| e.to_string())? {
            if other.is_you && other.id != row.id {
                db.upsert_speaker(&SpeakerRow {
                    is_you: false,
                    ..other
                })
                .map_err(|e| e.to_string())?;
            }
        }
    }
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
    profile_by_id(&db, &base, &row.id)
}

/// Remove a person. Their clips are deleted; transcript labels survive as
/// plain text.
#[tauri::command]
pub async fn delete_speaker(state: State<'_, AppState>, id: String) -> Result<(), String> {
    let (base, db) = storage_ctx(&state).await?;
    let refs = db.list_voice_refs(&id).map_err(|e| e.to_string())?;
    db.delete_speaker(&id).map_err(|e| e.to_string())?;
    for r in refs {
        if let Some(clip) = r.clip_path {
            if let Ok(path) = resolve_indexed_path(&base, &clip) {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    Ok(())
}

/// Remove several people at once (the list's multi-select). Same rules as the
/// single delete: clips go, transcript labels survive as plain text.
#[tauri::command]
pub async fn delete_speakers(state: State<'_, AppState>, ids: Vec<String>) -> Result<(), String> {
    let (base, db) = storage_ctx(&state).await?;
    for id in &ids {
        let refs = db.list_voice_refs(id).map_err(|e| e.to_string())?;
        db.delete_speaker(id).map_err(|e| e.to_string())?;
        for r in refs {
            if let Some(clip) = r.clip_path {
                if let Ok(path) = resolve_indexed_path(&base, &clip) {
                    let _ = std::fs::remove_file(path);
                }
            }
        }
    }
    Ok(())
}

// --- Voice-reference enrollment ----------------------------------------------

/// Record a ~10 s mic-only clip into an enrollment slot, embed it, and store
/// both. Blocks for the capture duration (the UI runs its countdown off this
/// command's lifetime).
#[tauri::command]
pub async fn record_voice_reference(
    state: State<'_, AppState>,
    speaker_id: String,
    slot: u32,
) -> Result<SpeakerProfile, String> {
    if !(1..=VOICE_SLOTS).contains(&slot) {
        return Err(format!("Slot must be between 1 and {VOICE_SLOTS}"));
    }
    if !state.engine.speaker_id_present() {
        return Err(
            "The speaker identification models are not downloaded — get them in Settings → Speakers first"
                .to_string(),
        );
    }
    if state.recorder.lock().await.is_some() {
        return Err("Can't record a voice reference during a meeting recording".to_string());
    }
    if state
        .enrolling
        .swap(true, Ordering::AcqRel)
    {
        return Err("Another voice reference is already being recorded".to_string());
    }
    // Reset any stale cancel signal, then make sure the flag is cleared on
    // every exit path below.
    state.enroll_cancel.store(false, Ordering::Release);
    let result = record_voice_reference_inner(&state, &speaker_id, slot).await;
    state.enrolling.store(false, Ordering::Release);
    result
}

async fn record_voice_reference_inner(
    state: &State<'_, AppState>,
    speaker_id: &str,
    slot: u32,
) -> Result<SpeakerProfile, String> {
    let (base, db) = storage_ctx(state).await?;
    let row = db
        .get_speaker(speaker_id)
        .map_err(|e| e.to_string())?
        .ok_or("Speaker not found")?;

    let mic_device = {
        let config = state.config.lock().await;
        let name = config.mic_device.trim().to_string();
        if name.is_empty() { None } else { Some(name) }
    };
    let cancel = state.enroll_cancel.clone();
    let engine = state.engine.clone();

    let (samples, embedding) = tokio::task::spawn_blocking(move || {
        let raw = crate::audio::recorder::capture_mic(mic_device.as_deref(), ENROLL_SECS, cancel)?;
        let trimmed = trim_silence(&raw);
        anyhow::ensure!(
            trimmed.len() >= 16_000,
            "the clip has less than a second of voice in it — try again closer to the microphone"
        );
        let embedding = engine.embed(trimmed)?;
        Ok::<_, anyhow::Error>((trimmed.to_vec(), embedding))
    })
    .await
    .map_err(|e| format!("enrollment task failed: {e}"))?
    .map_err(|e| e.to_string())?;

    // Keep the clip so future embedding models can re-fingerprint it.
    let rel_path = format!("voices/{}-{}.wav", row.id, slot);
    let abs = resolve_indexed_path(&base, &rel_path)?;
    if let Some(parent) = abs.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    write_wav_16k(&abs, &samples).map_err(|e| e.to_string())?;

    let (_, replaced_clips) = db
        .add_voice_ref(
            &row.id,
            VoiceRefKind::Enrolled,
            Some(slot),
            &embedding,
            Some(&rel_path),
            None,
        )
        .map_err(|e| e.to_string())?;
    // Same slot ⇒ same file path; only clean up clips that moved elsewhere.
    for clip in replaced_clips.iter().filter(|c| *c != &rel_path) {
        if let Ok(path) = resolve_indexed_path(&base, clip) {
            let _ = std::fs::remove_file(path);
        }
    }
    profile_by_id(&db, &base, &row.id)
}

/// Stop the in-flight enrollment capture early (keeps what was recorded).
#[tauri::command]
pub async fn cancel_voice_reference(state: State<'_, AppState>) -> Result<(), String> {
    state.enroll_cancel.store(true, Ordering::Release);
    Ok(())
}

#[tauri::command]
pub async fn delete_voice_reference(
    state: State<'_, AppState>,
    ref_id: i64,
) -> Result<(), String> {
    let (base, db) = storage_ctx(&state).await?;
    if let Some(clip) = db.delete_voice_ref(ref_id).map_err(|e| e.to_string())? {
        if let Ok(path) = resolve_indexed_path(&base, &clip) {
            let _ = std::fs::remove_file(path);
        }
    }
    Ok(())
}

// --- Suggestions --------------------------------------------------------------

/// "Yes, Speaker N is this person": relabel the meeting's segments, store the
/// cluster voice as a learned reference, and regenerate the transcript doc.
#[tauri::command]
pub async fn confirm_speaker_suggestion(
    state: State<'_, AppState>,
    meeting_id: String,
    label: String,
    speaker_id: String,
) -> Result<MeetingDetail, String> {
    let (base, db) = storage_ctx(&state).await?;
    let person = db
        .get_speaker(&speaker_id)
        .map_err(|e| e.to_string())?
        .ok_or("Speaker not found")?;

    let mut suggestions = load_suggestions(&db, &meeting_id)?;
    let Some(pos) = suggestions
        .iter()
        .position(|s| s.label == label && s.speaker_id == speaker_id)
    else {
        return Err("That suggestion is no longer pending".to_string());
    };
    let confirmed = suggestions.remove(pos);
    // The label is taken by this person now; drop competing suggestions on it.
    suggestions.retain(|s| s.label != label);

    db.assign_speaker_label(&meeting_id, &label, &speaker_id, &person.name)
        .map_err(|e| e.to_string())?;
    if !confirmed.centroid.is_empty() {
        let _ = db.add_voice_ref(
            &speaker_id,
            VoiceRefKind::Learned,
            None,
            &confirmed.centroid,
            None,
            Some(&meeting_id),
        );
    }
    save_suggestions(&db, &meeting_id, &suggestions)?;
    regenerate_transcript(
        &db,
        &state.search,
        &base,
        &meeting_id,
        Some(AttendeeFix::Swap(label.as_str(), &person.name)),
    )
}

#[tauri::command]
pub async fn dismiss_speaker_suggestion(
    state: State<'_, AppState>,
    meeting_id: String,
    label: String,
    speaker_id: String,
) -> Result<MeetingDetail, String> {
    let (base, db) = storage_ctx(&state).await?;
    let mut suggestions = load_suggestions(&db, &meeting_id)?;
    suggestions.retain(|s| !(s.label == label && s.speaker_id == speaker_id));
    save_suggestions(&db, &meeting_id, &suggestions)?;
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
            swap = Some((from, to));
        }
        SegmentEdit::ClearLabel { label } => {
            for seg in segments.iter_mut() {
                if seg.speaker.as_deref() == Some(label.as_str()) {
                    seg.speaker = None;
                    seg.speaker_id = None;
                }
            }
            // A suggestion for a label that no longer exists is noise.
            let mut suggestions = load_suggestions(&db, &meeting_id)?;
            let before = suggestions.len();
            suggestions.retain(|s| s.label != label);
            if suggestions.len() != before {
                save_suggestions(&db, &meeting_id, &suggestions)?;
            }
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

// --- Small pure helpers ---------------------------------------------------------

/// Drop leading/trailing silence: keep from the first to the last 32 ms frame
/// whose RMS clears a speech floor.
fn trim_silence(samples: &[f32]) -> &[f32] {
    const FRAME: usize = 512;
    const FLOOR: f32 = 0.01;
    let frame_rms = |frame: &[f32]| -> f32 {
        (frame.iter().map(|s| s * s).sum::<f32>() / frame.len().max(1) as f32).sqrt()
    };
    let frames: Vec<bool> = samples
        .chunks(FRAME)
        .map(|f| frame_rms(f) > FLOOR)
        .collect();
    let Some(first) = frames.iter().position(|&v| v) else {
        return &[];
    };
    let last = frames.iter().rposition(|&v| v).unwrap_or(first);
    &samples[first * FRAME..((last + 1) * FRAME).min(samples.len())]
}

fn write_wav_16k(path: &Path, samples: &[f32]) -> anyhow::Result<()> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)?;
    for &s in samples {
        writer.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)?;
    }
    writer.finalize()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_silence_keeps_the_voiced_middle() {
        let mut samples = vec![0.0f32; 4096];
        samples.extend(vec![0.2f32; 8192]);
        samples.extend(vec![0.0f32; 4096]);
        let trimmed = trim_silence(&samples);
        assert!(trimmed.len() >= 8192 && trimmed.len() < samples.len());
        assert!(trimmed.iter().any(|&s| s > 0.1));
    }

    #[test]
    fn trim_silence_of_pure_silence_is_empty() {
        assert!(trim_silence(&vec![0.0f32; 16_000]).is_empty());
        assert!(trim_silence(&[]).is_empty());
    }
}
