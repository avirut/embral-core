//! Post-meeting integrations: mirror notes into an Obsidian vault (or any
//! folder) and send a JSON summary to a webhook. Both are best-effort side
//! effects the Tauri app fires after a meeting's index entry is written;
//! neither may block or fail the core save (mirroring the existing non-fatal
//! MP3/LLM handling).
//!
//! Wire concerns are kept pure where possible: [`render_filename`],
//! [`to_inline_metadata`], and [`webhook_payload`] are unit-tested; the
//! IO/network wrappers ([`export_to_obsidian`], [`post_webhook`]) are thin.

use anyhow::Result;
use chrono::{DateTime, Utc};
use embral_types::{ExportMetadataFormat, MeetingRecord, WebhookMethod};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::text::sanitize_filename;

/// Render an export filename stem from a user template. Tokens: `{date}`
/// (YYYY-MM-DD), `{time}` (HH-MM), `{year}` `{month}` `{day}` `{hour}`
/// `{minute}`, and `{title}` (slugified: lowercase, words joined by `-`).
/// The result is filesystem-safe and never empty; callers append the
/// extension. Internal library filenames are unaffected by this — it applies
/// to exported copies only.
pub fn render_filename(template: &str, title: &str, started_at: &DateTime<Utc>) -> String {
    let slug: String = {
        let lowered = title.to_lowercase();
        let mut out = String::with_capacity(lowered.len());
        let mut prev_dash = false;
        for c in lowered.chars() {
            if c.is_ascii_alphanumeric() {
                out.push(c);
                prev_dash = false;
            } else if !prev_dash && !out.is_empty() {
                out.push('-');
                prev_dash = true;
            }
        }
        let out = out.trim_matches('-').to_string();
        if out.is_empty() {
            "untitled".to_string()
        } else {
            out
        }
    };

    let template = if template.trim().is_empty() {
        "{date}-{time}-{title}"
    } else {
        template
    };
    let rendered = template
        .replace("{date}", &started_at.format("%Y-%m-%d").to_string())
        .replace("{time}", &started_at.format("%H-%M").to_string())
        .replace("{year}", &started_at.format("%Y").to_string())
        .replace("{month}", &started_at.format("%m").to_string())
        .replace("{day}", &started_at.format("%d").to_string())
        .replace("{hour}", &started_at.format("%H").to_string())
        .replace("{minute}", &started_at.format("%M").to_string())
        .replace("{title}", &slug);

    let safe = sanitize_filename(rendered.trim());
    if safe.trim().is_empty() {
        "untitled".to_string()
    } else {
        safe
    }
}

/// Drop a leading `# Heading` line, so a caller can render the title itself.
fn strip_leading_h1(markdown: &str) -> &str {
    let trimmed = markdown.trim_start();
    match trimmed.strip_prefix("# ") {
        Some(rest) => rest.split_once('\n').map(|(_, body)| body).unwrap_or(""),
        None => trimmed,
    }
    .trim_start_matches('\n')
}

/// The document that leaves the app: everything the meeting produced, not just
/// the summary. A meeting with summaries off would otherwise export an empty
/// file, and even a summarized meeting is worth more in a vault with the user's
/// own notes and the transcript beside it.
///
/// `summary_body` and `user_notes` may be empty; their sections then disappear
/// rather than exporting a heading with nothing under it.
pub fn compose_export(
    frontmatter: &str,
    title: &str,
    summary_body: &str,
    user_notes: &str,
    transcript_text: &str,
) -> String {
    let mut out = String::new();
    let frontmatter = frontmatter.trim_end();
    if !frontmatter.is_empty() {
        out.push_str(frontmatter);
        out.push('\n');
    }

    let title = if title.trim().is_empty() {
        "Untitled Meeting"
    } else {
        title.trim()
    };
    out.push_str(&format!("# {title}\n"));

    let summary = strip_leading_h1(summary_body).trim();
    if !summary.is_empty() {
        out.push_str(&format!("\n{summary}\n"));
    }

    let notes = user_notes.trim();
    if !notes.is_empty() {
        out.push_str(&format!("\n## My notes\n\n{notes}\n"));
    }

    let transcript = transcript_text.trim();
    let transcript = if transcript.is_empty() {
        "_No transcript segments were captured._"
    } else {
        transcript
    };
    out.push_str(&format!("\n## Transcript\n\n{transcript}\n"));

    out
}

/// Convert a notes document's YAML frontmatter into a human-readable block
/// under the H1 (the "Inline" metadata style): the frontmatter is removed and
/// `**Date:** … / **Duration:** … / **Participants:** …` lines are inserted
/// after the title heading. Documents without frontmatter pass through
/// unchanged.
pub fn to_inline_metadata(markdown: &str) -> String {
    let Some((fields, body)) = split_frontmatter(markdown) else {
        return markdown.to_string();
    };

    let mut meta_lines: Vec<String> = Vec::new();
    if let Some(start) = fields
        .get("start_time")
        .and_then(|v| DateTime::parse_from_rfc3339(v).ok())
    {
        meta_lines.push(format!(
            "**Date:** {}",
            start.format("%A, %B %-d, %Y %H:%M")
        ));
    }
    if let Some(mins) = fields.get("duration_minutes") {
        meta_lines.push(format!("**Duration:** {} minutes", mins));
    }
    if let Some(attendees) = fields.get("attendees") {
        let names: Vec<String> = serde_json::from_str(attendees).unwrap_or_default();
        if !names.is_empty() {
            meta_lines.push(format!("**Participants:** {}", names.join(", ")));
        }
    }
    if meta_lines.is_empty() {
        return body.to_string();
    }
    let block = meta_lines.join("\n\n");

    // Insert after the first H1 when present, else prepend.
    let mut out: Vec<&str> = Vec::new();
    let mut inserted = false;
    for line in body.lines() {
        out.push(line);
        if !inserted && line.starts_with("# ") {
            out.push("");
            out.push(&block);
            inserted = true;
        }
    }
    if !inserted {
        return format!("{}\n\n{}", block, body.trim_start());
    }
    out.join("\n")
}

/// Parse a leading YAML frontmatter block into (fields, body-after-block).
fn split_frontmatter(
    markdown: &str,
) -> Option<(std::collections::HashMap<String, String>, &str)> {
    let rest = markdown.strip_prefix("---")?;
    let end = rest.find("\n---")?;
    let block = &rest[..end];
    let after = &rest[end + 4..];
    let body = after
        .strip_prefix('\n')
        .unwrap_or(after)
        .trim_start_matches('\n');

    let mut fields = std::collections::HashMap::new();
    for line in block.lines() {
        if let Some((k, v)) = line.split_once(':') {
            fields.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    Some((fields, body))
}

/// Write the meeting's notes into `vault_dir` (created if missing), named by
/// the user's filename template, with metadata rendered per `format`.
/// Returns the path written.
pub fn export_to_obsidian(
    vault_dir: &str,
    record: &MeetingRecord,
    notes_md: &str,
    filename_template: &str,
    format: ExportMetadataFormat,
) -> Result<PathBuf> {
    let dir = Path::new(vault_dir);
    std::fs::create_dir_all(dir)?;
    let stem = render_filename(filename_template, &record.title, &record.date);
    let path = dir.join(format!("{stem}.md"));
    let content = match format {
        // Frontmatter passes through — Obsidian reads it as note properties.
        ExportMetadataFormat::Frontmatter => notes_md.to_string(),
        ExportMetadataFormat::Inline => to_inline_metadata(notes_md),
    };
    std::fs::write(&path, content)?;
    Ok(path)
}

/// The JSON body sent to the configured webhook when a meeting finishes.
/// Stable, self-describing shape so downstream automations (Zapier, n8n, a
/// homelab script) can consume it without scraping files. `notes_markdown` is
/// the summary and is empty when summaries are off; the user's own notes ride
/// beside it, so a consumer always gets everything the meeting produced.
pub fn webhook_payload(
    record: &MeetingRecord,
    notes_md: &str,
    user_notes_md: &str,
    transcript_md: &str,
) -> Value {
    json!({
        "event": "meeting.finished",
        "meeting": {
            "id": record.id,
            "title": record.title,
            "date": record.date,
            "duration_seconds": record.duration_seconds,
        },
        "notes_markdown": notes_md,
        "user_notes_markdown": user_notes_md,
        "transcript_markdown": transcript_md,
    })
}

/// Send the webhook payload. Best-effort: the caller logs and swallows errors.
pub async fn post_webhook(
    url: &str,
    method: WebhookMethod,
    record: &MeetingRecord,
    notes_md: &str,
    user_notes_md: &str,
    transcript_md: &str,
) -> Result<()> {
    let payload = webhook_payload(record, notes_md, user_notes_md, transcript_md);
    let client = reqwest::Client::new();
    let request = match method {
        WebhookMethod::Post => client.post(url),
        WebhookMethod::Put => client.put(url),
    };
    request.json(&payload).send().await?.error_for_status()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn record() -> MeetingRecord {
        MeetingRecord {
            id: "260326T143000_a3f9b2".into(),
            title: "Q3: Pipeline Review".into(),
            date: Utc.with_ymd_and_hms(2026, 3, 26, 14, 30, 0).unwrap(),
            duration_seconds: 3480,
            chunks: 1,
            notes_path: "notes/x.md".into(),
            transcript_path: "transcripts/x.md".into(),
            audio_path: "audio/x.mp3".into(),
        }
    }

    #[test]
    fn filename_template_renders_all_tokens() {
        let at = Utc.with_ymd_and_hms(2026, 5, 3, 10, 30, 0).unwrap();
        assert_eq!(
            render_filename("{date}-{time}-{title}", "Weekly Sync", &at),
            "2026-05-03-10-30-weekly-sync"
        );
        assert_eq!(
            render_filename("{year}/{month} {title}", "A B", &at),
            "202605 a-b" // '/' is filesystem-illegal and stripped
        );
    }

    #[test]
    fn filename_slug_strips_punctuation() {
        let at = Utc.with_ymd_and_hms(2026, 5, 3, 10, 30, 0).unwrap();
        assert_eq!(
            render_filename("{title}", "Q3: Pipeline — Review!", &at),
            "q3-pipeline-review"
        );
    }

    #[test]
    fn filename_never_empty() {
        let at = Utc.with_ymd_and_hms(2026, 5, 3, 10, 30, 0).unwrap();
        // Empty template falls back to the default; an unsluggable title
        // becomes "untitled".
        assert_eq!(render_filename("", "***", &at), "2026-05-03-10-30-untitled");
        assert_eq!(render_filename("{title}", "***", &at), "untitled");
    }

    #[test]
    fn inline_metadata_replaces_frontmatter() {
        let md = "---\nstart_time: 2026-05-03T10:30:00Z\nduration_minutes: 45\nmeeting_id: x\nattendees: [\"Alice\",\"Bob\"]\n---\n\n# Weekly Sync\n\nBody text.";
        let out = to_inline_metadata(md);
        assert!(!out.contains("---"));
        assert!(out.starts_with("# Weekly Sync"));
        assert!(out.contains("**Date:** Sunday, May 3, 2026 10:30"));
        assert!(out.contains("**Duration:** 45 minutes"));
        assert!(out.contains("**Participants:** Alice, Bob"));
        assert!(out.contains("Body text."));
    }

    #[test]
    fn inline_metadata_passthrough_without_frontmatter() {
        assert_eq!(to_inline_metadata("# T\n\nbody"), "# T\n\nbody");
    }

    #[test]
    fn export_writes_templated_file_with_inline_metadata() {
        let dir = std::env::temp_dir().join(format!("embral-export-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let notes =
            "---\nstart_time: 2026-03-26T14:30:00Z\nduration_minutes: 58\nmeeting_id: x\nattendees: [\"Sarah\"]\n---\n\n# Q3: Pipeline Review\n\nbody";
        let path = export_to_obsidian(
            dir.to_str().unwrap(),
            &record(),
            notes,
            "{date} {title}",
            ExportMetadataFormat::Inline,
        )
        .unwrap();
        assert!(path.ends_with("2026-03-26 q3-pipeline-review.md"));
        let written = std::fs::read_to_string(&path).unwrap();
        assert!(written.contains("**Participants:** Sarah"));
        assert!(!written.starts_with("---"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn payload_has_stable_shape() {
        let p = webhook_payload(&record(), "# N", "my notes", "T");
        assert_eq!(p["event"], "meeting.finished");
        assert_eq!(p["meeting"]["id"], "260326T143000_a3f9b2");
        assert_eq!(p["meeting"]["title"], "Q3: Pipeline Review");
        assert_eq!(p["meeting"]["duration_seconds"], 3480);
        assert_eq!(p["notes_markdown"], "# N");
        assert_eq!(p["user_notes_markdown"], "my notes");
        assert_eq!(p["transcript_markdown"], "T");
    }

    const FRONTMATTER: &str = "---\nstart_time: 2026-03-26T14:30:00Z\n---\n";

    #[test]
    fn export_carries_summary_notes_and_transcript() {
        let out = compose_export(
            FRONTMATTER,
            "Weekly Sync",
            "# Weekly Sync\n\n## Decisions\n\nShip it.",
            "John: ship on Friday",
            "Alice: are we ready?",
        );

        // Frontmatter first (Obsidian reads it as note properties), one title.
        assert!(out.starts_with("---\nstart_time:"));
        assert_eq!(out.matches("# Weekly Sync").count(), 1);
        // The summary's own H1 was dropped, its body kept.
        assert!(out.contains("## Decisions"));
        assert!(out.contains("Ship it."));
        assert!(out.contains("## My notes\n\nJohn: ship on Friday"));
        assert!(out.contains("## Transcript\n\nAlice: are we ready?"));
    }

    #[test]
    fn export_without_a_summary_is_still_worth_having() {
        // Summaries off: the export is the user's notes and the transcript,
        // not an empty file.
        let out = compose_export(FRONTMATTER, "Weekly Sync", "", "my notes", "the words");
        assert!(out.contains("# Weekly Sync"));
        assert!(out.contains("## My notes\n\nmy notes"));
        assert!(out.contains("## Transcript\n\nthe words"));
    }

    #[test]
    fn export_omits_empty_sections() {
        // No summary and no user notes: no headings with nothing under them.
        let out = compose_export(FRONTMATTER, "T", "", "   ", "the words");
        assert!(!out.contains("## My notes"));
        assert!(out.contains("## Transcript"));

        // An empty transcript still says so rather than trailing off.
        let empty = compose_export(FRONTMATTER, "T", "", "", "");
        assert!(empty.contains("_No transcript segments were captured._"));
    }

    #[test]
    fn exported_summary_survives_the_inline_metadata_pass() {
        // The composed document is what export_to_obsidian renders, so it has
        // to round-trip through the Inline style too.
        let composed = compose_export(
            "---\nstart_time: 2026-05-03T10:30:00Z\nduration_minutes: 45\nattendees: [\"Alice\"]\n---\n",
            "Weekly Sync",
            "Body.",
            "",
            "words",
        );
        let inline = to_inline_metadata(&composed);
        assert!(!inline.starts_with("---"));
        assert!(inline.contains("**Participants:** Alice"));
        assert!(inline.contains("## Transcript"));
    }
}
