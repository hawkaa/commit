use rusqlite::{Connection, Result, params};
use uuid::Uuid;

use crate::models::{Subject, SubjectKind};

const CACHE_TTL_SECS: i64 = 3600; // 1 hour

pub struct Database {
    conn: Connection,
}

#[allow(clippy::missing_errors_doc)]
impl Database {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS subjects (
                id TEXT PRIMARY KEY,
                kind TEXT NOT NULL,
                identifier TEXT NOT NULL,
                display_name TEXT NOT NULL,
                endorsement_count INTEGER DEFAULT 0,
                created_at TEXT DEFAULT (datetime('now')),
                UNIQUE(kind, identifier)
            );

            CREATE TABLE IF NOT EXISTS signal_cache (
                subject_id TEXT NOT NULL UNIQUE,
                signals_json TEXT NOT NULL,
                score_json TEXT NOT NULL,
                fetched_at TEXT DEFAULT (datetime('now')),
                FOREIGN KEY (subject_id) REFERENCES subjects(id)
            );

            CREATE TABLE IF NOT EXISTS endorsements (
                id TEXT PRIMARY KEY,
                subject_id TEXT NOT NULL,
                category TEXT NOT NULL,
                proof_hash BLOB NOT NULL,
                proof_type TEXT NOT NULL,
                status TEXT DEFAULT 'pending_attestation',
                created_at TEXT DEFAULT (datetime('now')),
                FOREIGN KEY (subject_id) REFERENCES subjects(id)
            );

            CREATE INDEX IF NOT EXISTS idx_subjects_kind_id ON subjects(kind, identifier);
            CREATE INDEX IF NOT EXISTS idx_signal_cache_subject ON signal_cache(subject_id);
            CREATE INDEX IF NOT EXISTS idx_endorsements_subject ON endorsements(subject_id);",
        )
    }

    pub fn find_subject(&self, kind: &SubjectKind, identifier: &str) -> Result<Option<Subject>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, identifier, display_name, endorsement_count \
             FROM subjects WHERE kind = ? AND identifier = ?",
        )?;
        let result = stmt.query_row(params![kind.as_str(), identifier], |row| {
            let kind_str: String = row.get(1)?;
            Ok(Subject {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap_or_default(),
                kind: SubjectKind::parse(&kind_str).unwrap_or(SubjectKind::Service),
                identifier: row.get(2)?,
                display_name: row.get(3)?,
                endorsement_count: row.get(4)?,
            })
        });
        match result {
            Ok(subject) => Ok(Some(subject)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn upsert_subject(&self, subject: &Subject) -> Result<()> {
        self.conn.execute(
            "INSERT INTO subjects (id, kind, identifier, display_name, endorsement_count)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(kind, identifier) DO UPDATE SET
                display_name = excluded.display_name,
                endorsement_count = excluded.endorsement_count",
            params![
                subject.id.to_string(),
                subject.kind.as_str(),
                subject.identifier,
                subject.display_name,
                subject.endorsement_count,
            ],
        )?;
        Ok(())
    }

    pub fn cache_signals(
        &self,
        subject_id: &Uuid,
        signals_json: &str,
        score_json: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO signal_cache (subject_id, signals_json, score_json, fetched_at)
             VALUES (?, ?, ?, datetime('now'))",
            params![subject_id.to_string(), signals_json, score_json],
        )?;
        Ok(())
    }

    /// Returns `(signals_json, score_json)` if a fresh cache entry exists.
    /// Returns `None` if missing or stale (older than `CACHE_TTL_SECS`).
    pub fn get_cached_signals(&self, subject_id: &Uuid) -> Result<Option<(String, String)>> {
        let mut stmt = self.conn.prepare(
            "SELECT signals_json, score_json FROM signal_cache \
             WHERE subject_id = ? \
             AND (strftime('%s', 'now') - strftime('%s', fetched_at)) < ?",
        )?;
        let result = stmt.query_row(params![subject_id.to_string(), CACHE_TTL_SECS], |row| {
            Ok((row.get(0)?, row.get(1)?))
        });
        match result {
            Ok(data) => Ok(Some(data)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
