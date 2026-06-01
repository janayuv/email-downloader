//! Filesystem layout and persisted app settings.

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Resolved on-disk locations, derived from the Tauri app-data directory.
#[derive(Debug, Clone)]
pub struct AppPaths {
    /// Base app-data directory.
    pub data_dir: PathBuf,
    /// SQLite database file.
    pub db_path: PathBuf,
    /// Hash-based blob store root (`storage/`).
    pub storage_dir: PathBuf,
    /// Rotating log files.
    pub logs_dir: PathBuf,
    /// Per-job `backup-report.json` history.
    pub reports_dir: PathBuf,
}

impl AppPaths {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            db_path: data_dir.join("email-downloader.db"),
            storage_dir: data_dir.join("storage"),
            logs_dir: data_dir.join("logs"),
            reports_dir: data_dir.join("reports"),
            data_dir,
        }
    }

    /// Create every directory the app writes to. Idempotent.
    pub fn ensure(&self) -> Result<()> {
        for d in [
            &self.data_dir,
            &self.storage_dir,
            &self.storage_dir.join("attachments"),
            &self.storage_dir.join("messages"),
            &self.logs_dir,
            &self.reports_dir,
        ] {
            std::fs::create_dir_all(d)?;
        }
        Ok(())
    }

    pub fn settings_path(&self) -> PathBuf {
        self.data_dir.join("settings.json")
    }
}

/// User-facing settings persisted as `settings.json` in the data dir.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    /// Default export destination shown in the backup form.
    #[serde(default)]
    pub default_destination: String,
    /// "light" | "dark" | "system".
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Days to keep rotating logs.
    #[serde(default = "default_log_retention")]
    pub log_retention_days: u32,
    /// Max number of `backup-report.json` files to keep.
    #[serde(default = "default_report_retention")]
    pub report_retention_count: u32,
    /// Google OAuth client id for the Gmail provider (desktop installed app).
    /// Left blank until the operator configures a Google Cloud project.
    #[serde(default)]
    pub google_client_id: String,
    #[serde(default)]
    pub google_client_secret: String,
}

fn default_theme() -> String {
    "system".into()
}
fn default_log_retention() -> u32 {
    14
}
fn default_report_retention() -> u32 {
    50
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            default_destination: String::new(),
            theme: default_theme(),
            log_retention_days: default_log_retention(),
            report_retention_count: default_report_retention(),
            google_client_id: String::new(),
            google_client_secret: String::new(),
        }
    }
}

impl AppSettings {
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}
