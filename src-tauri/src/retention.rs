//! Retention / cleanup so the app never silently fills the disk:
//! - rotate + age-out log files,
//! - cap the number of retained `backup-report.json` files,
//! - garbage-collect orphaned blobs (sha256 files no longer referenced by any
//!   `attachments` row).

use crate::config::{AppPaths, AppSettings};
use crate::error::Result;
use crate::storage::Db;
use std::time::{Duration, SystemTime};

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct RetentionStats {
    pub logs_deleted: u64,
    pub reports_deleted: u64,
    pub blobs_deleted: u64,
    pub bytes_reclaimed: u64,
}

pub fn run(paths: &AppPaths, db: &Db, settings: &AppSettings) -> Result<RetentionStats> {
    let mut stats = RetentionStats::default();
    prune_logs(paths, settings, &mut stats)?;
    prune_reports(paths, settings, &mut stats)?;
    gc_blobs(paths, db, &mut stats)?;
    tracing::info!(?stats, "retention pass complete");
    Ok(stats)
}

fn file_age(path: &std::path::Path) -> Option<Duration> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = meta.modified().ok()?;
    SystemTime::now().duration_since(modified).ok()
}

fn prune_logs(paths: &AppPaths, settings: &AppSettings, stats: &mut RetentionStats) -> Result<()> {
    let max_age = Duration::from_secs(settings.log_retention_days as u64 * 86_400);
    if !paths.logs_dir.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(&paths.logs_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(age) = file_age(&path) {
                if age > max_age {
                    let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    if std::fs::remove_file(&path).is_ok() {
                        stats.logs_deleted += 1;
                        stats.bytes_reclaimed += size;
                    }
                }
            }
        }
    }
    Ok(())
}

fn prune_reports(
    paths: &AppPaths,
    settings: &AppSettings,
    stats: &mut RetentionStats,
) -> Result<()> {
    if !paths.reports_dir.exists() {
        return Ok(());
    }
    let mut reports: Vec<(std::path::PathBuf, SystemTime)> = std::fs::read_dir(&paths.reports_dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "json").unwrap_or(false))
        .filter_map(|e| {
            let m = e.metadata().ok()?.modified().ok()?;
            Some((e.path(), m))
        })
        .collect();
    // Newest first; delete everything past the retention count.
    reports.sort_by(|a, b| b.1.cmp(&a.1));
    for (path, _) in reports.into_iter().skip(settings.report_retention_count as usize) {
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        if std::fs::remove_file(&path).is_ok() {
            stats.reports_deleted += 1;
            stats.bytes_reclaimed += size;
        }
    }
    Ok(())
}

fn gc_blobs(paths: &AppPaths, db: &Db, stats: &mut RetentionStats) -> Result<()> {
    let referenced = db.all_attachment_shas()?;
    let root = paths.storage_dir.join("attachments");
    if !root.exists() {
        return Ok(());
    }
    // Two-level shard directories: attachments/<ab>/<sha>.<ext>
    for shard in std::fs::read_dir(&root)? {
        let shard = shard?;
        if !shard.path().is_dir() {
            continue;
        }
        for blob in std::fs::read_dir(shard.path())? {
            let blob = blob?;
            let path = blob.path();
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            if !stem.is_empty() && !referenced.contains(&stem) {
                let size = blob.metadata().map(|m| m.len()).unwrap_or(0);
                if std::fs::remove_file(&path).is_ok() {
                    stats.blobs_deleted += 1;
                    stats.bytes_reclaimed += size;
                }
            }
        }
    }
    Ok(())
}
