//! Per-job `backup-report.json` — written to the reports dir and copied into the
//! export destination. Invaluable for support/QA: it records exactly what was
//! produced, how long it took, and how many messages failed.

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupReport {
    pub job_id: String,
    pub account_email: String,
    pub status: String,
    pub started_at: i64,
    pub completed_at: i64,
    pub duration_seconds: i64,
    pub messages: i64,
    pub attachments: i64,
    pub failed: i64,
    pub exports: Vec<String>,
    /// Non-fatal export issues (e.g. PST sidecar missing) surfaced for the user.
    #[serde(default)]
    pub warnings: Vec<String>,
}

impl BackupReport {
    /// Write `backup-report-<job>.json` into `dir` and return its path.
    pub fn write_to(&self, dir: &Path) -> Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("backup-report-{}.json", self.job_id));
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(path)
    }
}
