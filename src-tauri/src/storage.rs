//! Storage roots, the database handle, and generated exports.
//!
//! Since R1 the SQLite database (`{storage_dir}/embral.db`) is the source of
//! truth. `index.json` and the markdown files under `notes/` / `transcripts/`
//! are *exports* regenerated from the DB after every mutation, so the MCP
//! servers and any user tooling that reads files keep working unchanged.

use anyhow::Result;
use chrono::Utc;
use embral_db::{Db, MeetingRow};
use embral_types::{resolve_storage_path, MeetingRecord};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub fn storage_base(storage_dir: &str) -> PathBuf {
    resolve_storage_path(storage_dir)
}

pub fn init_storage_dirs(base: &Path) -> Result<()> {
    std::fs::create_dir_all(base.join("audio"))?;
    std::fs::create_dir_all(base.join("transcripts"))?;
    std::fs::create_dir_all(base.join("notes"))?;
    Ok(())
}

pub fn generate_meeting_id() -> String {
    let ts = Utc::now().format("%y%m%dT%H%M%S");
    let uid = &Uuid::new_v4().to_string()[..6];
    format!("{}_{}", ts, uid)
}

/// Open (or create) the database under `base`, importing a legacy
/// `index.json` library on first run against this directory.
pub fn open_db(base: &Path) -> Result<Db> {
    let db = Db::open(&base.join("embral.db"))?;
    if db.meeting_count()? == 0 {
        let imported = import_legacy_index(&db, base)?;
        if imported > 0 {
            tracing::info!(
                imported,
                "imported legacy index.json library into the database"
            );
        }
    }
    Ok(db)
}

/// Pre-R1 index reader, kept for the one-time migration.
fn read_legacy_index(base: &Path) -> Result<Vec<MeetingRecord>> {
    let path = base.join("index.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&text)?)
}

/// Build DB rows from a legacy index + its markdown files. Legacy meetings
/// have no structured segments; their transcript text still lands in
/// `transcript_md`, which the FTS index covers.
fn import_legacy_index(db: &Db, base: &Path) -> Result<usize> {
    let records = read_legacy_index(base)?;
    if records.is_empty() {
        return Ok(0);
    }
    let read_md = |rel: &str| -> String {
        if rel.trim().is_empty() {
            return String::new();
        }
        std::fs::read_to_string(base.join(rel)).unwrap_or_default()
    };
    let rows: Vec<MeetingRow> = records
        .iter()
        .map(|r| {
            let notes_md = read_md(&r.notes_path);
            let transcript_md = read_md(&r.transcript_path);
            let attendees = {
                let from_notes = crate::commands::parse_attendees(&notes_md);
                if from_notes.is_empty() {
                    crate::commands::parse_attendees(&transcript_md)
                } else {
                    from_notes
                }
            };
            MeetingRow {
                id: r.id.clone(),
                title: r.title.clone(),
                started_at: r.date,
                duration_seconds: r.duration_seconds,
                notes_md,
                transcript_md,
                attendees,
                audio_path: r.audio_path.clone(),
                notes_path: r.notes_path.clone(),
                transcript_path: r.transcript_path.clone(),
            }
        })
        .collect();
    db.import_legacy(&rows)
}

/// Delete audio files older than `days` (0 = disabled): the file is removed,
/// the row's `audio_path` cleared, and the index re-exported. Transcripts and
/// notes are never touched. Returns how many meetings were pruned.
pub fn prune_old_audio(db: &Db, base: &Path, days: u32) -> Result<usize> {
    if days == 0 {
        return Ok(0);
    }
    let cutoff = Utc::now() - chrono::Duration::days(i64::from(days));
    let mut pruned = 0usize;
    for mut row in db.list_meetings(None, None)? {
        if row.audio_path.trim().is_empty() || row.started_at >= cutoff {
            continue;
        }
        match crate::commands::resolve_indexed_path(base, &row.audio_path) {
            Ok(path) => {
                if path.is_file() {
                    if let Err(e) = std::fs::remove_file(&path) {
                        tracing::warn!("janitor: failed to delete {}: {e}", path.display());
                        continue;
                    }
                }
                row.audio_path = String::new();
                db.upsert_meeting(&row)?;
                pruned += 1;
            }
            Err(e) => tracing::warn!("janitor: skipping {}: {e}", row.id),
        }
    }
    if pruned > 0 {
        export_index(db, base)?;
    }
    Ok(pruned)
}

/// Delete whole meetings older than `days` (0 = disabled): notes,
/// transcript, audio, and the database row (segments cascade). Returns how
/// many meetings were removed.
pub fn prune_old_meetings(db: &Db, base: &Path, days: u32) -> Result<usize> {
    if days == 0 {
        return Ok(0);
    }
    let cutoff = Utc::now() - chrono::Duration::days(i64::from(days));
    let mut pruned = 0usize;
    for row in db.list_meetings(None, None)? {
        if row.started_at >= cutoff {
            continue;
        }
        for indexed in [&row.notes_path, &row.transcript_path, &row.audio_path] {
            if indexed.trim().is_empty() {
                continue;
            }
            match crate::commands::resolve_indexed_path(base, indexed) {
                Ok(path) => {
                    if path.is_file() {
                        if let Err(e) = std::fs::remove_file(&path) {
                            tracing::warn!("janitor: failed to delete {}: {e}", path.display());
                        }
                    }
                }
                Err(e) => tracing::warn!("janitor: skipping file of {}: {e}", row.id),
            }
        }
        db.delete_meeting(&row.id)?;
        pruned += 1;
    }
    if pruned > 0 {
        export_index(db, base)?;
    }
    Ok(pruned)
}

/// Regenerate `index.json` from the database (newest first, same shape the
/// pre-R1 app wrote, so MCP servers keep working unchanged).
pub fn export_index(db: &Db, base: &Path) -> Result<()> {
    let records = db.export_records()?;
    std::fs::write(
        embral_types::index_path(base),
        serde_json::to_string_pretty(&records)?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Boot-path migration against the real demo library fixture: a pre-R1
    /// storage dir (index.json + markdown files) imports on first open, and
    /// the re-exported index round-trips.
    #[test]
    fn legacy_library_imports_on_first_open() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("docs")
            .join("prepop")
            .join("embral-demo");
        if !fixture.join("index.json").is_file() {
            // The demo library lives in docs/, which the public-repo filter
            // drops — the open-core tree skips this test (decode.rs idiom).
            eprintln!("demo fixture missing; skipping");
            return;
        }

        // Copy the fixture into a temp dir so the test never mutates it.
        let tmp = std::env::temp_dir().join(format!("embral-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        for sub in ["notes", "transcripts"] {
            std::fs::create_dir_all(tmp.join(sub)).unwrap();
            for entry in std::fs::read_dir(fixture.join(sub)).unwrap() {
                let entry = entry.unwrap();
                std::fs::copy(entry.path(), tmp.join(sub).join(entry.file_name())).unwrap();
            }
        }
        std::fs::copy(fixture.join("index.json"), tmp.join("index.json")).unwrap();

        let db = open_db(&tmp).expect("open + migrate");
        let n = db.meeting_count().unwrap();
        assert_eq!(n, 10, "all demo meetings imported");

        // Imported content carries the markdown bodies (search happens at
        // chunk level now — embral-search's own tests cover it).
        let rows = db.list_meetings(None, None).unwrap();
        assert!(rows.iter().all(|r| !r.notes_md.is_empty()));

        // Re-export matches the legacy shape (id + paths preserved).
        export_index(&db, &tmp).unwrap();
        let reread: Vec<MeetingRecord> =
            serde_json::from_str(&std::fs::read_to_string(tmp.join("index.json")).unwrap())
                .unwrap();
        assert_eq!(reread.len(), 10);
        assert!(reread.iter().all(|r| r.notes_path.starts_with("notes/")));

        // Second open must not double-import.
        drop(db);
        let db2 = open_db(&tmp).expect("re-open");
        assert_eq!(db2.meeting_count().unwrap(), 10);

        drop(db2);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn janitor_prunes_only_old_audio() {
        let tmp = std::env::temp_dir().join(format!("embral-janitor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join("audio")).unwrap();

        let db = Db::open(&tmp.join("embral.db")).unwrap();
        let mk = |id: &str, days_ago: i64| {
            let audio_rel = format!("audio/{id}.mp3");
            std::fs::write(tmp.join(&audio_rel), b"mp3").unwrap();
            db.upsert_meeting(&MeetingRow {
                id: id.into(),
                title: id.into(),
                started_at: Utc::now() - chrono::Duration::days(days_ago),
                duration_seconds: 60,
                notes_md: String::new(),
                transcript_md: "t".into(),
                attendees: vec![],
                audio_path: audio_rel,
                notes_path: format!("notes/{id}.md"),
                transcript_path: format!("transcripts/{id}.md"),
            })
            .unwrap();
        };
        mk("old", 40);
        mk("recent", 3);

        // Disabled (0 days) prunes nothing.
        assert_eq!(prune_old_audio(&db, &tmp, 0).unwrap(), 0);

        assert_eq!(prune_old_audio(&db, &tmp, 30).unwrap(), 1);
        assert!(!tmp.join("audio/old.mp3").exists());
        assert!(tmp.join("audio/recent.mp3").exists());
        assert_eq!(db.get_meeting("old").unwrap().unwrap().audio_path, "");
        assert_eq!(
            db.get_meeting("recent").unwrap().unwrap().audio_path,
            "audio/recent.mp3"
        );
        // Transcript markdown untouched.
        assert_eq!(db.get_meeting("old").unwrap().unwrap().transcript_md, "t");

        // Re-run is a no-op.
        assert_eq!(prune_old_audio(&db, &tmp, 30).unwrap(), 0);

        drop(db);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn janitor_prunes_whole_meetings() {
        let tmp = std::env::temp_dir().join(format!("embral-mjanitor-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        for sub in ["audio", "notes", "transcripts"] {
            std::fs::create_dir_all(tmp.join(sub)).unwrap();
        }

        let db = Db::open(&tmp.join("embral.db")).unwrap();
        let mk = |id: &str, days_ago: i64| {
            for (sub, ext) in [("audio", "mp3"), ("notes", "md"), ("transcripts", "md")] {
                std::fs::write(tmp.join(format!("{sub}/{id}.{ext}")), b"x").unwrap();
            }
            db.upsert_meeting(&MeetingRow {
                id: id.into(),
                title: id.into(),
                started_at: Utc::now() - chrono::Duration::days(days_ago),
                duration_seconds: 60,
                notes_md: "n".into(),
                transcript_md: "t".into(),
                attendees: vec![],
                audio_path: format!("audio/{id}.mp3"),
                notes_path: format!("notes/{id}.md"),
                transcript_path: format!("transcripts/{id}.md"),
            })
            .unwrap();
        };
        mk("ancient", 400);
        mk("recent", 3);

        // Disabled (0 days) prunes nothing.
        assert_eq!(prune_old_meetings(&db, &tmp, 0).unwrap(), 0);

        assert_eq!(prune_old_meetings(&db, &tmp, 365).unwrap(), 1);
        assert!(db.get_meeting("ancient").unwrap().is_none());
        assert!(!tmp.join("notes/ancient.md").exists());
        assert!(!tmp.join("transcripts/ancient.md").exists());
        assert!(!tmp.join("audio/ancient.mp3").exists());
        // The recent meeting is fully intact.
        assert!(db.get_meeting("recent").unwrap().is_some());
        assert!(tmp.join("notes/recent.md").exists());

        // Re-run is a no-op.
        assert_eq!(prune_old_meetings(&db, &tmp, 365).unwrap(), 0);

        drop(db);
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
