//! SQLite storage + FTS5 search index.
//!
//! The connection is wrapped in a `Mutex` and shared via `Arc`. SQLite handles
//! our concurrency needs (a desktop app with one writer); WAL mode keeps reads
//! non-blocking. FTS5 is compiled into the bundled SQLite amalgamation.

use crate::error::{AppError, Result};
use crate::model::*;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;
use std::path::Path;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let db = Db {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.migrate()?;
        Ok(db)
    }

    /// Lock and borrow the underlying connection. Used by `search.rs` to build
    /// its own FTS5 queries without duplicating the schema knowledge here.
    pub(crate) fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap()
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS accounts (
                id TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                email TEXT NOT NULL,
                label TEXT NOT NULL,
                imap_host TEXT NOT NULL DEFAULT '',
                imap_port INTEGER NOT NULL DEFAULT 0,
                keyring_reference TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
                message_id TEXT NOT NULL DEFAULT '',
                provider_message_id TEXT NOT NULL DEFAULT '',
                last_uid INTEGER NOT NULL DEFAULT 0,
                internal_date INTEGER NOT NULL DEFAULT 0,
                sha256 TEXT NOT NULL,
                from_addr TEXT NOT NULL DEFAULT '',
                to_addr TEXT NOT NULL DEFAULT '',
                cc_addr TEXT NOT NULL DEFAULT '',
                subject TEXT NOT NULL DEFAULT '',
                has_attachments INTEGER NOT NULL DEFAULT 0,
                size INTEGER NOT NULL DEFAULT 0,
                raw_path TEXT NOT NULL DEFAULT '',
                UNIQUE(account_id, sha256)
            );
            CREATE INDEX IF NOT EXISTS idx_messages_account ON messages(account_id);
            CREATE INDEX IF NOT EXISTS idx_messages_provider_id ON messages(account_id, provider_message_id);
            CREATE INDEX IF NOT EXISTS idx_messages_internal_date ON messages(internal_date);

            CREATE TABLE IF NOT EXISTS attachments (
                id TEXT PRIMARY KEY,
                message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
                filename TEXT NOT NULL,
                ext TEXT NOT NULL DEFAULT '',
                size INTEGER NOT NULL DEFAULT 0,
                sha256 TEXT NOT NULL,
                blob_path TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_attachments_message ON attachments(message_id);
            CREATE INDEX IF NOT EXISTS idx_attachments_sha ON attachments(sha256);

            CREATE TABLE IF NOT EXISTS jobs (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL,
                status TEXT NOT NULL,
                started_at INTEGER NOT NULL,
                completed_at INTEGER,
                checkpoint TEXT,
                messages_done INTEGER NOT NULL DEFAULT 0,
                attachments_done INTEGER NOT NULL DEFAULT 0,
                failed INTEGER NOT NULL DEFAULT 0,
                config_json TEXT NOT NULL DEFAULT '{}'
            );
            CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status);

            CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
                message_id UNINDEXED,
                subject,
                from_addr,
                to_addr,
                body_preview
            );
            "#,
        )?;
        Ok(())
    }

    // ---- accounts ----

    pub fn insert_account(&self, a: &Account) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO accounts (id, provider, email, label, imap_host, imap_port, keyring_reference, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
            params![
                a.id, a.provider.as_str(), a.email, a.label, a.imap_host, a.imap_port,
                a.keyring_reference, a.created_at
            ],
        )?;
        Ok(())
    }

    pub fn list_accounts(&self) -> Result<Vec<Account>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, provider, email, label, imap_host, imap_port, keyring_reference, created_at
             FROM accounts ORDER BY created_at DESC",
        )?;
        let rows = stmt
            .query_map([], |r| Ok(row_to_account(r)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_account(&self, id: &str) -> Result<Account> {
        let conn = self.conn.lock().unwrap();
        let acc = conn
            .query_row(
                "SELECT id, provider, email, label, imap_host, imap_port, keyring_reference, created_at
                 FROM accounts WHERE id=?1",
                params![id],
                |r| Ok(row_to_account(r)),
            )
            .optional()?;
        acc.ok_or_else(|| AppError::NotFound(format!("account {id}")))
    }

    pub fn delete_account(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM accounts WHERE id=?1", params![id])?;
        Ok(())
    }

    // ---- messages ----

    /// Dedupe check used by the streaming job before re-downloading bodies.
    pub fn message_exists(&self, account_id: &str, sha: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE account_id=?1 AND sha256=?2",
            params![account_id, sha],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn upsert_message(&self, m: &MessageRecord, body_preview: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO messages
               (id, account_id, message_id, provider_message_id, last_uid, internal_date,
                sha256, from_addr, to_addr, cc_addr, subject, has_attachments, size, raw_path)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)
             ON CONFLICT(account_id, sha256) DO UPDATE SET
                provider_message_id=excluded.provider_message_id,
                last_uid=excluded.last_uid,
                raw_path=excluded.raw_path",
            params![
                m.id, m.account_id, m.message_id, m.provider_message_id, m.last_uid,
                m.internal_date, m.sha256, m.from_addr, m.to_addr, m.cc_addr, m.subject,
                m.has_attachments as i64, m.size, m.raw_path
            ],
        )?;
        conn.execute(
            "INSERT INTO messages_fts (message_id, subject, from_addr, to_addr, body_preview)
             VALUES (?1,?2,?3,?4,?5)",
            params![m.id, m.subject, m.from_addr, m.to_addr, body_preview],
        )?;
        Ok(())
    }

    pub fn insert_attachment(&self, a: &AttachmentRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO attachments (id, message_id, filename, ext, size, sha256, blob_path)
             VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![a.id, a.message_id, a.filename, a.ext, a.size, a.sha256, a.blob_path],
        )?;
        Ok(())
    }

    /// The highest IMAP UID stored for an account — the cursor incremental
    /// backup resumes from.
    pub fn max_uid(&self, account_id: &str) -> Result<u32> {
        let conn = self.conn.lock().unwrap();
        let v: i64 = conn.query_row(
            "SELECT COALESCE(MAX(last_uid),0) FROM messages WHERE account_id=?1",
            params![account_id],
            |r| r.get(0),
        )?;
        Ok(v as u32)
    }

    // ---- jobs ----

    pub fn create_job(&self, j: &Job) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO jobs (id, account_id, status, started_at, completed_at, checkpoint,
                messages_done, attachments_done, failed, config_json)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                j.id, j.account_id, j.status.as_str(), j.started_at, j.completed_at,
                j.checkpoint, j.messages_done, j.attachments_done, j.failed, j.config_json
            ],
        )?;
        Ok(())
    }

    pub fn update_job_progress(
        &self,
        id: &str,
        messages_done: i64,
        attachments_done: i64,
        failed: i64,
        checkpoint: Option<&str>,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE jobs SET messages_done=?2, attachments_done=?3, failed=?4, checkpoint=?5 WHERE id=?1",
            params![id, messages_done, attachments_done, failed, checkpoint],
        )?;
        Ok(())
    }

    pub fn set_job_status(&self, id: &str, status: JobStatus, completed_at: Option<i64>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE jobs SET status=?2, completed_at=?3 WHERE id=?1",
            params![id, status.as_str(), completed_at],
        )?;
        Ok(())
    }

    pub fn get_job(&self, id: &str) -> Result<Job> {
        let conn = self.conn.lock().unwrap();
        let j = conn
            .query_row(
                "SELECT id, account_id, status, started_at, completed_at, checkpoint,
                        messages_done, attachments_done, failed, config_json
                 FROM jobs WHERE id=?1",
                params![id],
                |r| Ok(row_to_job(r)),
            )
            .optional()?;
        j.ok_or_else(|| AppError::NotFound(format!("job {id}")))
    }

    pub fn list_jobs(&self, limit: i64) -> Result<Vec<Job>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, account_id, status, started_at, completed_at, checkpoint,
                    messages_done, attachments_done, failed, config_json
             FROM jobs ORDER BY started_at DESC LIMIT ?1",
        )?;
        let rows = stmt
            .query_map(params![limit], |r| Ok(row_to_job(r)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// Jobs left `running`/`pending` from a previous run — surfaced on startup
    /// so they can be resumed or marked failed.
    pub fn incomplete_jobs(&self) -> Result<Vec<Job>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, account_id, status, started_at, completed_at, checkpoint,
                    messages_done, attachments_done, failed, config_json
             FROM jobs WHERE status IN ('running','pending') ORDER BY started_at DESC",
        )?;
        let rows = stmt
            .query_map([], |r| Ok(row_to_job(r)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ---- retention ----

    /// Every attachment hash currently referenced — used to GC orphan blobs.
    pub fn all_attachment_shas(&self) -> Result<HashSet<String>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT DISTINCT sha256 FROM attachments")?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<HashSet<_>>>()?;
        Ok(rows)
    }
}

// ---- row mappers ----

fn row_to_account(r: &rusqlite::Row) -> Account {
    Account {
        id: r.get_unwrap(0),
        provider: Provider::from_str(&r.get_unwrap::<_, String>(1)).unwrap_or(Provider::Imap),
        email: r.get_unwrap(2),
        label: r.get_unwrap(3),
        imap_host: r.get_unwrap(4),
        imap_port: r.get_unwrap::<_, i64>(5) as u16,
        keyring_reference: r.get_unwrap(6),
        created_at: r.get_unwrap(7),
    }
}

fn row_to_job(r: &rusqlite::Row) -> Job {
    Job {
        id: r.get_unwrap(0),
        account_id: r.get_unwrap(1),
        status: JobStatus::from_str(&r.get_unwrap::<_, String>(2)).unwrap_or(JobStatus::Failed),
        started_at: r.get_unwrap(3),
        completed_at: r.get_unwrap(4),
        checkpoint: r.get_unwrap(5),
        messages_done: r.get_unwrap(6),
        attachments_done: r.get_unwrap(7),
        failed: r.get_unwrap(8),
        config_json: r.get_unwrap(9),
    }
}
