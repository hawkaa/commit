use rusqlite::{Connection, Result, params};
use uuid::Uuid;

use crate::models::{Subject, SubjectKind};

const CACHE_TTL_SECS: i64 = 3600; // 1 hour

pub struct Database {
    conn: Connection,
}

#[derive(Debug)]
pub struct EndorsementRow {
    pub id: String,
    pub subject_id: String,
    pub category: String,
    pub proof_hash: Vec<u8>,
    pub proof_type: String,
    pub status: String,
    pub created_at: String,
}

#[allow(clippy::missing_errors_doc)]
impl Database {
    pub fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA busy_timeout=5000;
             PRAGMA foreign_keys=ON;",
        )?;
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

            CREATE TABLE IF NOT EXISTS attestations (
                id TEXT PRIMARY KEY,
                endorsement_id TEXT NOT NULL,
                tx_hash TEXT,
                chain TEXT NOT NULL DEFAULT 'pending',
                block_number INTEGER,
                attested_at TEXT,
                created_at TEXT DEFAULT (datetime('now')),
                FOREIGN KEY (endorsement_id) REFERENCES endorsements(id)
            );

            CREATE INDEX IF NOT EXISTS idx_subjects_kind_id ON subjects(kind, identifier);
            CREATE INDEX IF NOT EXISTS idx_signal_cache_subject ON signal_cache(subject_id);
            CREATE INDEX IF NOT EXISTS idx_endorsements_subject ON endorsements(subject_id);
            CREATE INDEX IF NOT EXISTS idx_attestations_endorsement ON attestations(endorsement_id);",
        )?;

        // Migration: normalize existing identifiers to lowercase.
        // Deduplicate case-insensitive collisions first (keep earliest by rowid).
        self.conn.execute_batch(
            "DELETE FROM subjects WHERE rowid NOT IN (
                SELECT MIN(rowid) FROM subjects GROUP BY kind, LOWER(identifier)
            );
            UPDATE subjects SET identifier = LOWER(identifier)
                WHERE identifier != LOWER(identifier);",
        )?;

        Ok(())
    }

    pub fn find_subject(&self, kind: &SubjectKind, identifier: &str) -> Result<Option<Subject>> {
        let normalized = identifier.to_lowercase();
        let mut stmt = self.conn.prepare(
            "SELECT id, kind, identifier, display_name, endorsement_count \
             FROM subjects WHERE kind = ? AND identifier = ?",
        )?;
        let result = stmt.query_row(params![kind.as_str(), normalized], |row| {
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
        let normalized_id = subject.identifier.to_lowercase();
        self.conn.execute(
            "INSERT INTO subjects (id, kind, identifier, display_name, endorsement_count)
             VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(kind, identifier) DO UPDATE SET
                display_name = excluded.display_name",
            params![
                subject.id.to_string(),
                subject.kind.as_str(),
                normalized_id,
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

    pub fn create_endorsement(
        &self,
        id: &Uuid,
        subject_id: &Uuid,
        category: &str,
        proof_hash: &[u8],
        proof_type: &str,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO endorsements (id, subject_id, category, proof_hash, proof_type)
             VALUES (?, ?, ?, ?, ?)",
            params![
                id.to_string(),
                subject_id.to_string(),
                category,
                proof_hash,
                proof_type,
            ],
        )?;
        Ok(())
    }

    pub fn get_endorsements_for_subject(&self, subject_id: &Uuid) -> Result<Vec<EndorsementRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, subject_id, category, proof_hash, proof_type, status, created_at
             FROM endorsements WHERE subject_id = ? ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![subject_id.to_string()], |row| {
            Ok(EndorsementRow {
                id: row.get(0)?,
                subject_id: row.get(1)?,
                category: row.get(2)?,
                proof_hash: row.get(3)?,
                proof_type: row.get(4)?,
                status: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?;
        rows.collect()
    }

    pub fn update_endorsement_status(&self, id: &Uuid, status: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE endorsements SET status = ? WHERE id = ?",
            params![status, id.to_string()],
        )?;
        Ok(())
    }

    pub fn create_attestation(&self, id: &Uuid, endorsement_id: &Uuid, chain: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO attestations (id, endorsement_id, chain)
             VALUES (?, ?, ?)",
            params![id.to_string(), endorsement_id.to_string(), chain],
        )?;
        Ok(())
    }

    pub fn update_attestation_tx(&self, id: &Uuid, tx_hash: &str, block_number: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE attestations SET tx_hash = ?, block_number = ?, attested_at = datetime('now')
             WHERE id = ?",
            params![tx_hash, block_number, id.to_string()],
        )?;
        Ok(())
    }

    pub fn get_endorsement_count(&self, subject_id: &Uuid) -> Result<u32> {
        let count: u32 = self.conn.query_row(
            "SELECT COUNT(*) FROM endorsements WHERE subject_id = ? AND status != 'failed'",
            params![subject_id.to_string()],
            |row| row.get(0),
        )?;
        Ok(count)
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
