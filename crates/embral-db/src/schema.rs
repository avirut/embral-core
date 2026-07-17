//! Schema creation and versioned migrations.
//!
//! `meta.schema_version` records the applied version; migrations are plain
//! SQL batches applied in order inside a transaction. Adding a table/column
//! later = append a new entry to `MIGRATIONS`.

use anyhow::Result;
use rusqlite::Connection;

const MIGRATIONS: &[&str] = &[
    // v1 — initial schema: meetings + segments + external-content FTS index.
    r#"
    CREATE TABLE meetings (
        id               TEXT PRIMARY KEY,
        title            TEXT NOT NULL,
        started_at       TEXT NOT NULL,
        duration_seconds INTEGER NOT NULL,
        notes_md         TEXT NOT NULL DEFAULT '',
        transcript_md    TEXT NOT NULL DEFAULT '',
        attendees        TEXT NOT NULL DEFAULT '[]',
        audio_path       TEXT NOT NULL DEFAULT '',
        notes_path       TEXT NOT NULL DEFAULT '',
        transcript_path  TEXT NOT NULL DEFAULT '',
        created_at       TEXT NOT NULL,
        updated_at       TEXT NOT NULL
    );

    CREATE INDEX idx_meetings_started_at ON meetings(started_at DESC);

    CREATE TABLE segments (
        id         INTEGER PRIMARY KEY,
        meeting_id TEXT NOT NULL REFERENCES meetings(id) ON DELETE CASCADE,
        idx        INTEGER NOT NULL,
        speaker    TEXT,
        text       TEXT NOT NULL,
        start_secs REAL NOT NULL,
        end_secs   REAL NOT NULL,
        UNIQUE(meeting_id, idx)
    );

    CREATE VIRTUAL TABLE meetings_fts USING fts5(
        title, notes_md, transcript_md,
        content='meetings', content_rowid='rowid'
    );

    CREATE TRIGGER meetings_ai AFTER INSERT ON meetings BEGIN
        INSERT INTO meetings_fts(rowid, title, notes_md, transcript_md)
        VALUES (new.rowid, new.title, new.notes_md, new.transcript_md);
    END;

    CREATE TRIGGER meetings_ad AFTER DELETE ON meetings BEGIN
        INSERT INTO meetings_fts(meetings_fts, rowid, title, notes_md, transcript_md)
        VALUES ('delete', old.rowid, old.title, old.notes_md, old.transcript_md);
    END;

    CREATE TRIGGER meetings_au AFTER UPDATE ON meetings BEGIN
        INSERT INTO meetings_fts(meetings_fts, rowid, title, notes_md, transcript_md)
        VALUES ('delete', old.rowid, old.title, old.notes_md, old.transcript_md);
        INSERT INTO meetings_fts(rowid, title, notes_md, transcript_md)
        VALUES (new.rowid, new.title, new.notes_md, new.transcript_md);
    END;
    "#,
    // v2 — speaker registry: known people, their voice-reference embeddings,
    // a registry link per segment, and per-meeting match suggestions.
    r#"
    CREATE TABLE speakers (
        id         TEXT PRIMARY KEY,
        name       TEXT NOT NULL,
        emails     TEXT NOT NULL DEFAULT '[]',
        notes      TEXT NOT NULL DEFAULT '',
        is_you     INTEGER NOT NULL DEFAULT 0,
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
    );

    CREATE TABLE voice_refs (
        id                INTEGER PRIMARY KEY,
        speaker_id        TEXT NOT NULL REFERENCES speakers(id) ON DELETE CASCADE,
        kind              TEXT NOT NULL,
        slot              INTEGER,
        embedding         BLOB NOT NULL,
        dim               INTEGER NOT NULL,
        clip_path         TEXT,
        source_meeting_id TEXT,
        created_at        TEXT NOT NULL
    );

    CREATE INDEX idx_voice_refs_speaker ON voice_refs(speaker_id);

    ALTER TABLE segments ADD COLUMN speaker_id TEXT;
    ALTER TABLE meetings ADD COLUMN speaker_suggestions TEXT NOT NULL DEFAULT '[]';
    "#,
    // v3 — timestamped chapters on meetings + dictation history.
    r#"
    ALTER TABLE meetings ADD COLUMN chapters_json TEXT NOT NULL DEFAULT '[]';

    CREATE TABLE dictations (
        id           INTEGER PRIMARY KEY,
        raw_text     TEXT NOT NULL,
        cleaned_text TEXT,
        app          TEXT,
        created_at   TEXT NOT NULL
    );
    "#,
    // v4 — user-starred moments replace AI chapters (existing chapter data
    // is discarded deliberately; pre-release no-compat).
    r#"
    ALTER TABLE meetings DROP COLUMN chapters_json;
    ALTER TABLE meetings ADD COLUMN stars_json TEXT NOT NULL DEFAULT '[]';
    "#,
    // v5 — the user's raw live notes, stored verbatim for the Notes tab
    // (previously they only existed appended inside the summary document).
    r#"
    ALTER TABLE meetings ADD COLUMN user_notes TEXT NOT NULL DEFAULT '';
    "#,
    // v6 — speaker emails are gone. Nothing ever read them: matching is
    // embedding-only and meeting attendees are display names. They were a hook
    // for calendar matching, which is not built and not planned.
    r#"
    ALTER TABLE speakers DROP COLUMN emails;
    "#,
    // v7 — the chunk-level retrieval index: passages over transcripts, user
    // notes, summaries, and dictations (embral-search owns the SQL that
    // fills and queries these). The vec0 vector table is deliberately NOT
    // here: its dimensions belong to the embedding model, so embral-search
    // creates and versions it outside migrations (meta keys
    // embedding_model / embedding_dim).
    r#"
    CREATE TABLE chunks (
        id             INTEGER PRIMARY KEY,
        meeting_id     TEXT REFERENCES meetings(id) ON DELETE CASCADE,
        dictation_id   INTEGER REFERENCES dictations(id) ON DELETE CASCADE,
        source         TEXT NOT NULL,
        chunk_index    INTEGER NOT NULL,
        text           TEXT NOT NULL,
        embedding_text TEXT NOT NULL,
        start_secs     REAL,
        end_secs       REAL,
        speakers       TEXT NOT NULL DEFAULT '[]',
        speaker_ids    TEXT NOT NULL DEFAULT '[]',
        content_hash   TEXT NOT NULL,
        embedded_with  TEXT,
        CHECK ((meeting_id IS NULL) != (dictation_id IS NULL))
    );

    CREATE INDEX idx_chunks_meeting   ON chunks(meeting_id);
    CREATE INDEX idx_chunks_dictation ON chunks(dictation_id);
    CREATE INDEX idx_chunks_pending   ON chunks(embedded_with) WHERE embedded_with IS NULL;

    CREATE VIRTUAL TABLE chunks_fts USING fts5(
        text, content='chunks', content_rowid='id'
    );

    CREATE TRIGGER chunks_ai AFTER INSERT ON chunks BEGIN
        INSERT INTO chunks_fts(rowid, text) VALUES (new.id, new.text);
    END;

    CREATE TRIGGER chunks_ad AFTER DELETE ON chunks BEGIN
        INSERT INTO chunks_fts(chunks_fts, rowid, text)
        VALUES ('delete', old.id, old.text);
    END;

    -- Column-scoped, unlike meetings_au: the indexer's chunk_index and
    -- embedded_with updates must not re-tokenize FTS; changed content
    -- arrives as delete+insert.
    CREATE TRIGGER chunks_au AFTER UPDATE OF text ON chunks BEGIN
        INSERT INTO chunks_fts(chunks_fts, rowid, text)
        VALUES ('delete', old.id, old.text);
        INSERT INTO chunks_fts(rowid, text) VALUES (new.id, new.text);
    END;
    "#,
    // v8 — the meeting-level FTS index dies: both former consumers (the
    // palette, the MCP server) now search chunk passages, and nothing reads
    // meetings_fts anymore. This also retires its au trigger, which
    // re-tokenized all three columns on *every* meetings UPDATE.
    r#"
    DROP TRIGGER meetings_ai;
    DROP TRIGGER meetings_ad;
    DROP TRIGGER meetings_au;
    DROP TABLE meetings_fts;
    "#,
];

#[cfg(test)]
mod tests {
    use super::*;

    /// A database created at v1 (real R1-era schema, with data) migrates to
    /// the current version with v2 tables/columns usable and old rows intact.
    #[test]
    fn v1_database_upgrades_in_place() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
            .unwrap();
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.execute(
            "INSERT INTO meta(key, value) VALUES ('schema_version', '1')",
            [],
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO meetings (id, title, started_at, duration_seconds, created_at, updated_at)
             VALUES ('m1', 'Old', '2026-01-01T00:00:00Z', 60, '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');
             INSERT INTO segments (meeting_id, idx, speaker, text, start_secs, end_secs)
             VALUES ('m1', 0, 'Speaker 1', 'hi', 0.0, 1.0);",
        )
        .unwrap();

        migrate(&conn).unwrap();

        let version: String = conn
            .query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len().to_string());
        // Old data intact; new column defaults to NULL / '[]'.
        let (speaker_id, suggestions): (Option<String>, String) = conn
            .query_row(
                "SELECT s.speaker_id, m.speaker_suggestions
                 FROM segments s JOIN meetings m ON m.id = s.meeting_id",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(speaker_id, None);
        assert_eq!(suggestions, "[]");
        // New tables usable.
        conn.execute(
            "INSERT INTO speakers (id, name, created_at, updated_at)
             VALUES ('sp', 'Alice', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [],
        )
        .unwrap();
    }

    /// v6 drops speaker emails. A registry that *has* emails must survive the
    /// drop with its people intact — the column goes, nobody goes with it.
    #[test]
    fn v6_drops_emails_and_keeps_the_people() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)")
            .unwrap();
        // Stand the database up at v5, the version before the drop.
        for migration in &MIGRATIONS[..5] {
            conn.execute_batch(migration).unwrap();
        }
        conn.execute(
            "INSERT INTO meta(key, value) VALUES ('schema_version', '5')",
            [],
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO speakers (id, name, emails, notes, is_you, created_at, updated_at)
             VALUES ('sp_a', 'Alice', '[\"alice@example.com\"]', 'note', 1,
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z');",
        )
        .unwrap();

        migrate(&conn).unwrap();

        let version: String = conn
            .query_row("SELECT value FROM meta WHERE key = 'schema_version'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, MIGRATIONS.len().to_string());

        // The person survived, with everything that was not an email.
        let (name, notes, is_you): (String, String, i64) = conn
            .query_row(
                "SELECT name, notes, is_you FROM speakers WHERE id = 'sp_a'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(name, "Alice");
        assert_eq!(notes, "note");
        assert_eq!(is_you, 1);

        // And the column is really gone.
        assert!(conn
            .query_row("SELECT emails FROM speakers", [], |r| r.get::<_, String>(0))
            .is_err());
    }
}

/// The version this build writes — what `meta.schema_version` becomes after
/// [`migrate`].
pub fn latest_version() -> i64 {
    MIGRATIONS.len() as i64
}

pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
    )?;
    let current: i64 = conn
        .query_row(
            "SELECT value FROM meta WHERE key = 'schema_version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .map(|v| v.parse().unwrap_or(0))
        .unwrap_or(0);

    for (i, sql) in MIGRATIONS.iter().enumerate() {
        let version = (i + 1) as i64;
        if version <= current {
            continue;
        }
        conn.execute_batch("BEGIN")?;
        let applied = conn.execute_batch(sql).and_then(|()| {
            conn.execute(
                "INSERT INTO meta(key, value) VALUES ('schema_version', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                [version.to_string()],
            )
            .map(|_| ())
        });
        match applied {
            Ok(()) => conn.execute_batch("COMMIT")?,
            Err(e) => {
                let _ = conn.execute_batch("ROLLBACK");
                return Err(anyhow::anyhow!("migration to v{version} failed: {e}"));
            }
        }
    }
    Ok(())
}
