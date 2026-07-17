//! Building passages ("chunks") out of a meeting's four documents and out
//! of dictations. The transcript chunker reuses the tested paragraph rules
//! in `embral-notes::transcript` — a passage is packed paragraphs, never a
//! new segmentation theory.

use chrono::{DateTime, Utc};
use embral_notes::transcript::paragraphs;
use embral_types::TranscriptionSegment;
use sha2::{Digest, Sha256};

/// Where a chunk's text came from. A user-written note is a stronger signal
/// than a generated summary, and both differ from verbatim speech — search
/// keeps them distinct rather than blending everything into one soup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Transcript,
    UserNotes,
    Summary,
    Dictation,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Transcript => "transcript",
            Source::UserNotes => "user_notes",
            Source::Summary => "summary",
            Source::Dictation => "dictation",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BuiltChunk {
    pub source: Source,
    pub chunk_index: u32,
    /// The verbatim passage — what results quote.
    pub text: String,
    /// Context header + text — what gets embedded and hashed.
    pub embedding_text: String,
    pub start_secs: Option<f64>,
    pub end_secs: Option<f64>,
    pub speakers: Vec<String>,
    pub speaker_ids: Vec<String>,
    pub content_hash: String,
}

pub struct MeetingDocs<'a> {
    pub title: &'a str,
    pub started_at: DateTime<Utc>,
    pub segments: &'a [TranscriptionSegment],
    pub user_notes: &'a str,
    pub summary_md: &'a str,
    pub transcript_md: &'a str,
}

/// Passage word budget: pack whole paragraphs up to the cap; a paragraph
/// that alone exceeds it stays one oversized chunk (paragraphs are already
/// length-capped for transcripts; prose blocks are rarely this long).
const MAX_WORDS: usize = 400;
/// Overlap: each chunk re-opens with its predecessor's final unit so a
/// thought split across the boundary is findable from either side —
/// skipped when that unit alone is most of a budget.
const MAX_OVERLAP_WORDS: usize = 120;

fn words(text: &str) -> usize {
    text.split_whitespace().count()
}

fn content_hash(embedding_text: &str) -> String {
    let digest = Sha256::digest(embedding_text.as_bytes());
    digest[..16].iter().map(|b| format!("{b:02x}")).collect()
}

/// One packable unit of text — a transcript paragraph or a prose block.
struct Unit {
    text: String,
    start: Option<f64>,
    end: Option<f64>,
    speaker: Option<String>,
    speaker_id: Option<String>,
}

/// Pack consecutive units into chunk-sized groups (indices into `units`).
fn pack(units: &[Unit]) -> Vec<Vec<usize>> {
    let mut groups: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut count = 0usize;

    for (i, unit) in units.iter().enumerate() {
        let w = words(&unit.text);
        if !current.is_empty() && count + w > MAX_WORDS {
            let overlap = *current.last().expect("non-empty group");
            groups.push(std::mem::take(&mut current));
            count = 0;
            if words(&units[overlap].text) <= MAX_OVERLAP_WORDS {
                current.push(overlap);
                count = words(&units[overlap].text);
            }
        }
        current.push(i);
        count += w;
    }
    // Every flush is immediately followed by a push, so a non-empty tail is
    // never the bare overlap seed.
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

fn header(title: &str, date: DateTime<Utc>, speakers: &[String]) -> String {
    let day = date.format("%Y-%m-%d");
    if speakers.is_empty() {
        format!("{title} — {day}.")
    } else {
        format!("{title} — {day}. {}", speakers.join(", "))
    }
}

fn build(source: Source, units: &[Unit], title: &str, date: DateTime<Utc>) -> Vec<BuiltChunk> {
    let mut out = Vec::new();
    for group in pack(units) {
        let text = group
            .iter()
            .map(|&i| units[i].text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let mut speakers: Vec<String> = Vec::new();
        let mut speaker_ids: Vec<String> = Vec::new();
        for &i in &group {
            if let Some(s) = &units[i].speaker {
                if !speakers.iter().any(|e| e == s) {
                    speakers.push(s.clone());
                }
            }
            if let Some(id) = &units[i].speaker_id {
                if !speaker_ids.iter().any(|e| e == id) {
                    speaker_ids.push(id.clone());
                }
            }
        }
        let embedding_text = format!("{}\n{}", header(title, date, &speakers), text);
        out.push(BuiltChunk {
            source,
            chunk_index: out.len() as u32,
            content_hash: content_hash(&embedding_text),
            start_secs: group.iter().filter_map(|&i| units[i].start).next(),
            end_secs: group.iter().rev().filter_map(|&i| units[i].end).next(),
            text,
            embedding_text,
            speakers,
            speaker_ids,
        })
    }
    out
}

/// Strip YAML frontmatter and a leading `# ` title line — document
/// scaffolding, not content.
fn strip_scaffolding(md: &str) -> &str {
    let mut rest = md.trim_start();
    if let Some(after) = rest.strip_prefix("---") {
        if let Some(end) = after.find("\n---") {
            rest = after[end + 4..].trim_start();
        }
    }
    if rest.starts_with("# ") {
        rest = rest.split_once('\n').map(|(_, r)| r).unwrap_or("");
    }
    rest.trim()
}

/// Blank-line blocks of prose (headings ride with their block position).
fn prose_units(text: &str) -> Vec<Unit> {
    text.split("\n\n")
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .map(|b| Unit {
            text: b.to_string(),
            start: None,
            end: None,
            speaker: None,
            speaker_id: None,
        })
        .collect()
}

/// Best-effort `Name: text` speaker extraction for transcript documents
/// that no longer have segments (legacy imports).
fn labeled_prose_units(text: &str) -> Vec<Unit> {
    prose_units(text)
        .into_iter()
        .map(|mut u| {
            if let Some((name, _)) = u.text.split_once(": ") {
                if !name.is_empty() && name.len() <= 40 && !name.contains('\n') {
                    u.speaker = Some(name.to_string());
                }
            }
            u
        })
        .collect()
}

pub fn chunk_meeting(docs: &MeetingDocs) -> Vec<BuiltChunk> {
    let mut out = Vec::new();

    // Transcript: paragraphs from segments when we have them; the rendered
    // document is the fallback for meetings that predate segment storage.
    if !docs.segments.is_empty() {
        let units: Vec<Unit> = paragraphs(docs.segments)
            .into_iter()
            .map(|p| Unit {
                text: p.text,
                start: Some(p.start),
                end: Some(p.end),
                speaker: p.speaker,
                speaker_id: p.speaker_id,
            })
            .collect();
        out.extend(build(Source::Transcript, &units, docs.title, docs.started_at));
    } else {
        let body = strip_scaffolding(docs.transcript_md);
        // The transcript-less placeholder is document scaffolding, not
        // content — indexed, it wins semantic queries it has no answer to.
        if !body.is_empty() && body != "_No transcript segments were captured._" {
            let units = labeled_prose_units(body);
            out.extend(build(Source::Transcript, &units, docs.title, docs.started_at));
        }
    }

    let notes = docs.user_notes.trim();
    if !notes.is_empty() {
        out.extend(build(
            Source::UserNotes,
            &prose_units(notes),
            docs.title,
            docs.started_at,
        ));
    }

    let summary = strip_scaffolding(docs.summary_md);
    if !summary.is_empty() {
        out.extend(build(
            Source::Summary,
            &prose_units(summary),
            docs.title,
            docs.started_at,
        ));
    }

    out
}

/// Dictations are usually one thought — chunked only when long.
pub fn chunk_dictation(created_at: DateTime<Utc>, text: &str) -> Vec<BuiltChunk> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let units = if words(text) > MAX_WORDS {
        prose_units(text)
    } else {
        vec![Unit {
            text: text.to_string(),
            start: None,
            end: None,
            speaker: None,
            speaker_id: None,
        }]
    };
    build(Source::Dictation, &units, "Dictation", created_at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn seg(speaker: Option<&str>, text: &str, start: f64, end: f64) -> TranscriptionSegment {
        TranscriptionSegment {
            speaker: speaker.map(String::from),
            speaker_id: speaker.map(|s| format!("id-{s}")),
            text: text.to_string(),
            start,
            end,
        }
    }

    fn date() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 1, 10, 0, 0).unwrap()
    }

    fn docs<'a>(segments: &'a [TranscriptionSegment]) -> MeetingDocs<'a> {
        MeetingDocs {
            title: "Planning Sync",
            started_at: date(),
            segments,
            user_notes: "",
            summary_md: "",
            transcript_md: "",
        }
    }

    #[test]
    fn passages_respect_the_word_budget_and_overlap() {
        // 40 paragraphs of ~50 words each (alternating speakers force breaks).
        let sentence = "these are exactly ten words of filler for the test.";
        let long: String = std::iter::repeat(sentence).take(5).collect::<Vec<_>>().join(" ");
        let segments: Vec<_> = (0..40)
            .map(|i| {
                seg(
                    Some(if i % 2 == 0 { "A" } else { "B" }),
                    &long,
                    i as f64 * 10.0,
                    i as f64 * 10.0 + 9.0,
                )
            })
            .collect();

        let chunks = chunk_meeting(&docs(&segments));
        assert!(chunks.len() > 1);
        for c in &chunks {
            assert!(words(&c.text) <= MAX_WORDS + MAX_OVERLAP_WORDS, "chunk too big");
        }
        // Overlap: each later chunk begins with its predecessor's last paragraph.
        for pair in chunks.windows(2) {
            let last_para = pair[0].text.split("\n\n").last().unwrap();
            assert!(pair[1].text.starts_with(last_para));
        }
        // Speakers and timing carried.
        assert!(chunks[0].speakers.contains(&"A".to_string()));
        assert!(chunks[0].speaker_ids.contains(&"id-A".to_string()));
        assert_eq!(chunks[0].start_secs, Some(0.0));
    }

    #[test]
    fn embedding_text_carries_the_context_header() {
        let segments = [seg(Some("Alice"), "We should ship the beta.", 0.0, 2.0)];
        let chunks = chunk_meeting(&docs(&segments));
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0]
            .embedding_text
            .starts_with("Planning Sync — 2026-07-01. Alice\n"));
        assert_eq!(chunks[0].text, "We should ship the beta.");
    }

    #[test]
    fn hash_is_stable_and_content_sensitive() {
        let segments = [seg(Some("Alice"), "We should ship the beta.", 0.0, 2.0)];
        let a = chunk_meeting(&docs(&segments));
        let b = chunk_meeting(&docs(&segments));
        assert_eq!(a[0].content_hash, b[0].content_hash);

        let mut d = docs(&segments);
        d.title = "Renamed Sync"; // header feeds the hash — a rename re-embeds
        let c = chunk_meeting(&d);
        assert_ne!(a[0].content_hash, c[0].content_hash);
    }

    #[test]
    fn all_four_sources_chunk_distinctly() {
        let segments = [seg(Some("Alice"), "Spoken words here.", 0.0, 2.0)];
        let mut d = docs(&segments);
        d.user_notes = "my own shorthand note";
        d.summary_md = "---\nmeeting_id: x\n---\n# Planning Sync\n\n## Key Takeaways\n\nShip it.";
        let chunks = chunk_meeting(&d);

        let sources: Vec<Source> = chunks.iter().map(|c| c.source).collect();
        assert!(sources.contains(&Source::Transcript));
        assert!(sources.contains(&Source::UserNotes));
        assert!(sources.contains(&Source::Summary));
        // Frontmatter and the title line never become content.
        let summary = chunks.iter().find(|c| c.source == Source::Summary).unwrap();
        assert!(!summary.text.contains("meeting_id"));
        assert!(!summary.text.contains("# Planning Sync"));
        assert!(summary.text.contains("Ship it."));
    }

    #[test]
    fn segmentless_meetings_fall_back_to_the_rendered_transcript() {
        let mut d = docs(&[]);
        d.transcript_md =
            "---\nmeeting_id: x\n---\n# Old Import Transcript\n\nDana: We agreed on the vendor.\n\nUnattributed closing remarks.";
        let chunks = chunk_meeting(&d);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].source, Source::Transcript);
        assert!(chunks[0].speakers.contains(&"Dana".to_string()));
        assert!(chunks[0].start_secs.is_none());
    }

    #[test]
    fn the_no_transcript_placeholder_is_not_content() {
        let mut d = docs(&[]);
        d.transcript_md =
            "---\nmeeting_id: x\n---\n# Quiet Meeting Transcript\n\n_No transcript segments were captured._";
        assert!(chunk_meeting(&d).is_empty());
    }

    #[test]
    fn dictations_stay_whole_until_long() {
        let short = chunk_dictation(date(), "send the follow-up email tomorrow");
        assert_eq!(short.len(), 1);
        assert_eq!(short[0].source, Source::Dictation);
        assert!(short[0].embedding_text.starts_with("Dictation — 2026-07-01.\n"));

        let para = "word ".repeat(300);
        let long_text = format!("{para}\n\n{para}\n\n{para}");
        let long = chunk_dictation(date(), &long_text);
        assert!(long.len() > 1);

        assert!(chunk_dictation(date(), "   ").is_empty());
    }
}
