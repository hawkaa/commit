use axum::http::StatusCode;
use rusqlite::{Connection, Result, params};
use uuid::Uuid;

use crate::models::{Subject, SubjectKind};

/// Map a rusqlite error to an HTTP status code.
/// Returns 409 Conflict for unique constraint violations, 500 otherwise.
pub fn map_db_error(e: rusqlite::Error) -> StatusCode {
    if let rusqlite::Error::SqliteFailure(err, _) = &e
        && err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
    {
        return StatusCode::CONFLICT;
    }
    StatusCode::INTERNAL_SERVER_ERROR
}

const CACHE_TTL_SECS: i64 = 3600; // 1 hour

pub struct Database {
    conn: Connection,
}

#[derive(Debug)]
pub struct AttestationRow {
    pub id: String,
    pub endorsement_id: String,
    pub tx_hash: Option<String>,
    pub chain: String,
    pub block_number: Option<i64>,
    pub attested_at: Option<String>,
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
                chain TEXT NOT NULL DEFAULT 'base_sepolia',
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
        // Deduplicate case-insensitive collisions: cascade-delete related rows first
        // to avoid FK violations, then remove duplicate subjects.
        self.conn.execute_batch(
            "DELETE FROM attestations WHERE endorsement_id IN (
                SELECT e.id FROM endorsements e
                JOIN subjects s ON e.subject_id = s.id
                WHERE s.rowid NOT IN (
                    SELECT MIN(rowid) FROM subjects GROUP BY kind, LOWER(identifier)
                )
            );
            DELETE FROM endorsements WHERE subject_id IN (
                SELECT id FROM subjects WHERE rowid NOT IN (
                    SELECT MIN(rowid) FROM subjects GROUP BY kind, LOWER(identifier)
                )
            );
            DELETE FROM subjects WHERE rowid NOT IN (
                SELECT MIN(rowid) FROM subjects GROUP BY kind, LOWER(identifier)
            );
            UPDATE subjects SET identifier = LOWER(identifier)
                WHERE identifier != LOWER(identifier);",
        )?;

        // Migration: add attestation_data column and unique proof_hash constraint.
        // Deduplicate existing proof_hash collisions before adding constraint.
        let has_attestation_col: bool = self
            .conn
            .prepare("SELECT attestation_data FROM endorsements LIMIT 0")
            .is_ok();
        if !has_attestation_col {
            self.conn
                .execute_batch("ALTER TABLE endorsements ADD COLUMN attestation_data BLOB;")?;
        }
        let has_unique_proof_hash: bool = self
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='index' AND name='idx_endorsements_unique_proof_hash'")
            .and_then(|mut s| s.query_row([], |_| Ok(true)))
            .unwrap_or(false);
        if !has_unique_proof_hash {
            // Cascade-delete attestations for duplicate endorsements, then dedup
            self.conn.execute_batch(
                "DELETE FROM attestations WHERE endorsement_id IN (
                    SELECT id FROM endorsements WHERE rowid NOT IN (
                        SELECT MIN(rowid) FROM endorsements GROUP BY proof_hash
                    )
                );
                DELETE FROM endorsements WHERE rowid NOT IN (
                    SELECT MIN(rowid) FROM endorsements GROUP BY proof_hash
                );
                CREATE UNIQUE INDEX idx_endorsements_unique_proof_hash ON endorsements(proof_hash);",
            )?;
        }

        // Migration: add endorser_key_hash column (revisit indicators, sentiment flips, sybil analysis).
        let has_endorser_key_hash: bool = self
            .conn
            .prepare("SELECT endorser_key_hash FROM endorsements LIMIT 0")
            .is_ok();
        if !has_endorser_key_hash {
            self.conn.execute_batch(
                "ALTER TABLE endorsements ADD COLUMN endorser_key_hash TEXT;
                 CREATE INDEX IF NOT EXISTS idx_endorsements_key_hash ON endorsements(endorser_key_hash);",
            )?;
        }

        // Migration: normalize chain='pending' to 'base_sepolia'
        self.conn.execute_batch(
            "UPDATE attestations SET chain = 'base_sepolia' WHERE chain = 'pending';",
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

    #[allow(clippy::too_many_arguments)]
    pub fn create_endorsement(
        &self,
        id: &Uuid,
        subject_id: &Uuid,
        category: &str,
        proof_hash: &[u8],
        proof_type: &str,
        attestation_data: Option<&[u8]>,
        endorser_key_hash: Option<&str>,
    ) -> Result<()> {
        self.conn.execute(
            "INSERT INTO endorsements (id, subject_id, category, proof_hash, proof_type, attestation_data, endorser_key_hash)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
            params![
                id.to_string(),
                subject_id.to_string(),
                category,
                proof_hash,
                proof_type,
                attestation_data,
                endorser_key_hash,
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

    /// Mark an attestation as skipped (e.g. already attested on-chain).
    /// Sets `chain = 'already_attested'` but leaves `tx_hash = NULL` so that
    /// `get_attestation_for_endorsement` correctly reports `on_chain: false`
    /// and `get_pending_attestations` (which filters `tx_hash IS NULL AND chain = 'base_sepolia'`)
    /// excludes it from future batches.
    pub fn mark_attestation_skipped(&self, id: &Uuid) -> Result<()> {
        self.conn.execute(
            "UPDATE attestations SET chain = 'already_attested' WHERE id = ?",
            params![id.to_string()],
        )?;
        Ok(())
    }

    /// Batch-fetch attestations for multiple endorsement IDs in a single query.
    /// Returns a map from endorsement_id to `AttestationRow`.
    pub fn get_attestations_for_endorsements(
        &self,
        endorsement_ids: &[&str],
    ) -> Result<std::collections::HashMap<String, AttestationRow>> {
        if endorsement_ids.is_empty() {
            return Ok(std::collections::HashMap::new());
        }
        let placeholders: Vec<&str> = endorsement_ids.iter().map(|_| "?").collect();
        let sql = format!(
            "SELECT id, endorsement_id, tx_hash, chain, block_number, attested_at \
             FROM attestations WHERE endorsement_id IN ({})",
            placeholders.join(", ")
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> = endorsement_ids
            .iter()
            .map(|id| id as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt.query_map(params.as_slice(), |row| {
            Ok(AttestationRow {
                id: row.get(0)?,
                endorsement_id: row.get(1)?,
                tx_hash: row.get(2)?,
                chain: row.get(3)?,
                block_number: row.get(4)?,
                attested_at: row.get(5)?,
            })
        })?;
        let mut map = std::collections::HashMap::new();
        for row in rows {
            let att = row?;
            map.insert(att.endorsement_id.clone(), att);
        }
        Ok(map)
    }

    pub fn count_recent_endorsements(&self, subject_id: &Uuid, window_minutes: i64) -> Result<u32> {
        let count: u32 = self.conn.query_row(
            "SELECT COUNT(*) FROM endorsements WHERE subject_id = ? AND created_at > datetime('now', '-' || ? || ' minutes')",
            params![subject_id.to_string(), window_minutes],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    pub fn get_endorsement_count(&self, subject_id: &Uuid) -> Result<u32> {
        let count: u32 = self.conn.query_row(
            "SELECT COUNT(*) FROM endorsements WHERE subject_id = ? AND status != 'failed'",
            params![subject_id.to_string()],
            |row| row.get(0),
        )?;
        Ok(count)
    }

    /// Returns `(verified_count, pending_count)` for non-failed endorsements.
    pub fn get_endorsement_counts_by_status(&self, subject_id: &Uuid) -> Result<(u32, u32)> {
        let mut stmt = self.conn.prepare(
            "SELECT status, COUNT(*) FROM endorsements \
             WHERE subject_id = ? AND status IN ('verified', 'pending_attestation') \
             GROUP BY status",
        )?;
        let mut verified: u32 = 0;
        let mut pending: u32 = 0;
        let rows = stmt.query_map(params![subject_id.to_string()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
        })?;
        for row in rows {
            let (status, count) = row?;
            match status.as_str() {
                "verified" => verified = count,
                "pending_attestation" => pending = count,
                _ => {}
            }
        }
        Ok((verified, pending))
    }

    pub fn get_recent_endorsements(
        &self,
        subject_id: &Uuid,
        limit: u32,
    ) -> Result<Vec<EndorsementRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, subject_id, category, proof_hash, proof_type, status, created_at
             FROM endorsements WHERE subject_id = ? ORDER BY created_at DESC LIMIT ?",
        )?;
        let rows = stmt.query_map(params![subject_id.to_string(), limit], |row| {
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

    /// Returns the average age (in months) of non-failed endorsements for a subject.
    /// Uses `julianday()` to compute months since each endorsement was created.
    /// Returns 0.0 if the subject has no endorsements.
    pub fn get_endorsement_tenure_months(&self, subject_id: &Uuid) -> Result<f64> {
        let avg: f64 = self.conn.query_row(
            "SELECT COALESCE(AVG((julianday('now') - julianday(created_at)) / 30.44), 0.0) \
             FROM endorsements WHERE subject_id = ? AND status != 'failed'",
            params![subject_id.to_string()],
            |row| row.get(0),
        )?;
        Ok(avg)
    }

    /// Deletes the cached signal data for a subject, forcing recomputation on the next request.
    pub fn invalidate_signal_cache(&self, subject_id: &Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM signal_cache WHERE subject_id = ?",
            params![subject_id.to_string()],
        )?;
        Ok(())
    }

    /// Returns pending attestations (no tx_hash) joined with their endorsement proof_hash.
    /// Limited to `limit` rows, ordered oldest first.
    pub fn get_pending_attestations(
        &self,
        limit: u32,
    ) -> Result<Vec<crate::services::l2::PendingAttestation>> {
        let mut stmt = self.conn.prepare(
            "SELECT a.id, a.endorsement_id, e.proof_hash
             FROM attestations a
             JOIN endorsements e ON a.endorsement_id = e.id
             WHERE a.tx_hash IS NULL AND a.chain = 'base_sepolia'
             ORDER BY a.created_at ASC
             LIMIT ?",
        )?;
        let rows = stmt.query_map(params![limit], |row| {
            Ok(crate::services::l2::PendingAttestation {
                id: row.get(0)?,
                endorsement_id: row.get(1)?,
                endorsement_proof_hash: row.get(2)?,
            })
        })?;
        rows.collect()
    }

    pub fn get_attestation_for_endorsement(
        &self,
        endorsement_id: &str,
    ) -> Result<Option<AttestationRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, endorsement_id, tx_hash, chain, block_number, attested_at
             FROM attestations WHERE endorsement_id = ?",
        )?;
        let result = stmt.query_row(params![endorsement_id], |row| {
            Ok(AttestationRow {
                id: row.get(0)?,
                endorsement_id: row.get(1)?,
                tx_hash: row.get(2)?,
                chain: row.get(3)?,
                block_number: row.get(4)?,
                attested_at: row.get(5)?,
            })
        });
        match result {
            Ok(row) => Ok(Some(row)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
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
