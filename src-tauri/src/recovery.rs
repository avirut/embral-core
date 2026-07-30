//! What an interrupted recording leaves behind, and what launch does with
//! it ([recording.md](../../docs/recording.md) §Crash recovery).
//!
//! A recording holds everything that matters in memory until
//! `finalize_meeting` runs: segments in `AppState::current_segments`, the
//! notes draft in `recording_drafts`, stars in `stars`. A force-quit, a
//! panic, a power cut, or an OS kill takes all of it and leaves a WAV
//! nobody references. So the live recording mirrors those three things
//! into a scratch directory as they happen, and the next launch turns the
//! leftovers into an ordinary meeting.
//!
//! The scratch is deliberately *not* the database. Writing segments to
//! SQLite mid-recording would mean creating the meeting row at start, and
//! an in-progress row leaks into every listing query, the search index,
//! and the janitor. These files live for the length of one recording and
//! are deleted by a successful stop, which is the same shape as the
//! `in_progress.txt` they replace ([storage.md](../../docs/storage.md)).

use std::path::{Path, PathBuf};

use embral_types::TranscriptionSegment;

use crate::commands::Star;

/// Bytes per second of recorded audio: 16 kHz, mono, f32.
const BYTES_PER_SEC: u64 = 16_000 * 4;

/// How much audio an interrupted recording needs before it is worth
/// keeping. Below this it is a mis-click or a start that crashed
/// immediately; recovering it would run the whole summarize pipeline over
/// nothing and leave an empty meeting in the list. The user is not asked —
/// approving your own meeting is a chore, and after a crash you may not
/// remember there was one.
const MIN_RECOVERABLE_SECS: u64 = 10;

/// Everything the interrupted recording managed to write down.
pub struct Interrupted {
    pub meeting_id: String,
    pub segments: Vec<TranscriptionSegment>,
    pub user_notes: Option<String>,
    pub user_title: Option<String>,
    pub stars: Vec<Star>,
}

/// The user's own words and marks, rewritten whole on every change (they
/// are small and always superseded, unlike segments, which only ever grow).
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Drafts {
    notes: String,
    title: String,
    #[serde(default)]
    stars: Vec<f64>,
}

fn dir(base: &Path) -> PathBuf {
    base.join("in_progress")
}

fn id_file(base: &Path) -> PathBuf {
    dir(base).join("meeting_id.txt")
}

fn segments_file(base: &Path) -> PathBuf {
    dir(base).join("segments.jsonl")
}

fn drafts_file(base: &Path) -> PathBuf {
    dir(base).join("drafts.json")
}

/// Open the scratch for a recording that is starting. Any leftovers are
/// cleared first: launch already had its chance to recover them, and a
/// second recording must never inherit the first one's words.
pub fn begin(base: &Path, meeting_id: &str) {
    clear(base);
    if let Err(e) = std::fs::create_dir_all(dir(base)) {
        tracing::warn!("could not open the recovery scratch: {e}");
        return;
    }
    if let Err(e) = std::fs::write(id_file(base), meeting_id) {
        tracing::warn!("could not write the recovery scratch: {e}");
    }
}

/// Append one finalized segment. Called from the event forwarder, which
/// already holds it — a few hundred bytes every few seconds.
pub fn append_segment(base: &Path, segment: &TranscriptionSegment) {
    if !dir(base).is_dir() {
        return;
    }
    let Ok(mut line) = serde_json::to_string(segment) else {
        return;
    };
    line.push('\n');
    use std::io::Write;
    let appended = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(segments_file(base))
        .and_then(|mut f| f.write_all(line.as_bytes()));
    if let Err(e) = appended {
        tracing::warn!("could not record a segment for recovery: {e}");
    }
}

/// Mirror the notes/title draft and the stars. Driven by the frontend's
/// existing debounce, so this is not a per-keystroke write.
pub fn write_drafts(base: &Path, notes: &str, title: &str, stars: &[f64]) {
    if !dir(base).is_dir() {
        return;
    }
    let drafts = Drafts {
        notes: notes.to_string(),
        title: title.to_string(),
        stars: stars.to_vec(),
    };
    let written = serde_json::to_string(&drafts)
        .map_err(std::io::Error::other)
        .and_then(|json| std::fs::write(drafts_file(base), json));
    if let Err(e) = written {
        tracing::warn!("could not mirror the notes draft for recovery: {e}");
    }
}

/// The recording finished normally (or its leftovers were just consumed).
pub fn clear(base: &Path) {
    let scratch = dir(base);
    if scratch.exists() {
        if let Err(e) = std::fs::remove_dir_all(&scratch) {
            tracing::warn!("could not clear the recovery scratch: {e}");
        }
    }
}

/// The in-flight meeting's id, for the stop path.
pub fn active_meeting_id(base: &Path) -> Option<String> {
    let id = std::fs::read_to_string(id_file(base)).ok()?;
    let id = id.trim().to_string();
    (!id.is_empty()).then_some(id)
}

/// Whether an interrupted recording carries enough audio to be worth
/// keeping. Pure so the threshold is testable; the caller supplies the
/// WAV's size on disk, which is the honest measure — the header may be up
/// to one flush interval behind the samples actually written.
pub fn worth_recovering(wav_bytes: u64) -> bool {
    wav_bytes.saturating_sub(WAV_HEADER_ALLOWANCE) / BYTES_PER_SEC >= MIN_RECOVERABLE_SECS
}

/// Generous allowance for the RIFF header, so a header layout change can
/// never make a silent file look like a recoverable one.
const WAV_HEADER_ALLOWANCE: u64 = 1024;

/// Read whatever the interrupted recording left, and clear the scratch so
/// the next launch cannot try again. `None` when there is nothing to
/// recover — no scratch, no id, or too little audio to bother with.
pub fn take(base: &Path, wav_for: impl Fn(&str) -> PathBuf) -> Option<Interrupted> {
    let meeting_id = active_meeting_id(base)?;
    let wav = wav_for(&meeting_id);
    let bytes = std::fs::metadata(&wav).map(|m| m.len()).unwrap_or(0);

    if !worth_recovering(bytes) {
        tracing::info!(
            meeting_id,
            bytes,
            "an interrupted recording was too short to keep — discarding it"
        );
        let _ = std::fs::remove_file(&wav);
        // Images pasted into notes that never became a meeting have nothing
        // left to belong to.
        crate::commands::remove_meeting_assets(base, &meeting_id);
        clear(base);
        return None;
    }

    let segments = read_segments(base);
    let drafts: Drafts = std::fs::read_to_string(drafts_file(base))
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();

    tracing::info!(
        meeting_id,
        segments = segments.len(),
        secs = bytes / BYTES_PER_SEC,
        "recovering an interrupted recording"
    );
    clear(base);

    Some(Interrupted {
        meeting_id,
        segments,
        user_notes: (!drafts.notes.is_empty()).then_some(drafts.notes),
        user_title: (!drafts.title.trim().is_empty()).then_some(drafts.title),
        stars: drafts
            .stars
            .into_iter()
            .map(|seconds| Star {
                seconds,
                note_block: None,
            })
            .collect(),
    })
}

/// Parse the appended segments, skipping any trailing line a crash cut in
/// half — the whole point of one JSON object per line.
fn read_segments(base: &Path) -> Vec<TranscriptionSegment> {
    let Ok(text) = std::fs::read_to_string(segments_file(base)) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(text: &str, start: f64) -> TranscriptionSegment {
        TranscriptionSegment {
            speaker: None,
            text: text.to_string(),
            start,
            end: start + 1.0,
            speaker_id: None,
        }
    }

    #[test]
    fn a_recording_that_died_in_its_first_seconds_is_not_worth_keeping() {
        // A mis-click, or a start that crashed at once. Recovering it would
        // summarize nothing and put an empty meeting in the list.
        assert!(!worth_recovering(0));
        assert!(!worth_recovering(1024)); // header only
        assert!(!worth_recovering(1024 + BYTES_PER_SEC * 9));
    }

    #[test]
    fn ten_seconds_of_audio_is_worth_keeping() {
        assert!(worth_recovering(1024 + BYTES_PER_SEC * 10));
        assert!(worth_recovering(1024 + BYTES_PER_SEC * 3600));
    }

    #[test]
    fn the_scratch_round_trips_what_the_recording_wrote() {
        let base = std::env::temp_dir().join(format!("embral-recovery-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        begin(&base, "m-123");
        append_segment(&base, &seg("hello", 0.0));
        append_segment(&base, &seg("there", 1.0));
        write_drafts(&base, "my notes", "My Meeting", &[4.5]);

        // Stand in a long-enough WAV so the threshold passes.
        let wav = base.join("m-123.wav");
        std::fs::write(&wav, vec![0u8; (1024 + BYTES_PER_SEC * 30) as usize]).unwrap();

        let found = take(&base, |id| base.join(format!("{id}.wav"))).expect("recoverable");
        assert_eq!(found.meeting_id, "m-123");
        assert_eq!(found.segments.len(), 2);
        assert_eq!(found.segments[1].text, "there");
        assert_eq!(found.user_notes.as_deref(), Some("my notes"));
        assert_eq!(found.user_title.as_deref(), Some("My Meeting"));
        assert_eq!(found.stars.len(), 1);

        // Taking it clears the scratch: the next launch must not try again.
        assert!(take(&base, |id| base.join(format!("{id}.wav"))).is_none());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_half_written_segment_line_is_skipped_not_fatal() {
        // The crash can land mid-write; one JSON object per line means the
        // torn tail is the only casualty.
        let base = std::env::temp_dir().join(format!("embral-recovery-torn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        begin(&base, "m-torn");
        append_segment(&base, &seg("complete", 0.0));
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(segments_file(&base))
            .unwrap();
        f.write_all(b"{\"text\":\"tor").unwrap();
        drop(f);

        let segments = read_segments(&base);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "complete");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_short_leftover_takes_its_orphan_wav_with_it() {
        let base = std::env::temp_dir().join(format!("embral-recovery-tiny-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        begin(&base, "m-tiny");
        let wav = base.join("m-tiny.wav");
        std::fs::write(&wav, vec![0u8; 2048]).unwrap();

        assert!(take(&base, |id| base.join(format!("{id}.wav"))).is_none());
        assert!(!wav.exists(), "the orphan wav is cleaned up too");
        assert!(!dir(&base).exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
