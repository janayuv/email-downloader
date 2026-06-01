//! Email Downloader — Tauri backend entry point.
//!
//! Wires plugins, builds the shared [`AppState`] (database, blob store, paths,
//! settings, running-job registry), initializes structured logging, and resumes
//! any jobs interrupted by a crash before exposing the command surface.

mod auth;
mod blobstore;
mod commands;
mod config;
mod error;
mod export;
mod extract;
mod gmail_query;
mod hashing;
mod jobs;
mod logging;
mod model;
mod parser;
mod providers;
mod rate_limiter;
mod report;
mod retention;
mod search;
mod storage;

use blobstore::BlobStore;
use config::{AppPaths, AppSettings};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use storage::Db;
use tauri::Manager;

/// Shared application state, managed by Tauri as `Arc<AppState>` so background
/// jobs can hold a clone independent of any single command's lifetime.
pub struct AppState {
    pub db: Db,
    pub paths: AppPaths,
    pub blobs: BlobStore,
    pub settings: Mutex<AppSettings>,
    /// Cancellation flags for currently running jobs, keyed by job id.
    pub jobs: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// Keeps the non-blocking log writer alive for the app's lifetime.
    _log_guard: Mutex<tracing_appender::non_blocking::WorkerGuard>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let paths = AppPaths::new(data_dir);
            paths.ensure()?;

            let guard = logging::init(&paths.logs_dir);
            tracing::info!(data_dir = %paths.data_dir.display(), "starting Email Downloader");

            let db = Db::open(&paths.db_path)?;
            let blobs = BlobStore::new(&paths.storage_dir);
            let settings = AppSettings::load(&paths.settings_path());

            let state = Arc::new(AppState {
                db,
                paths,
                blobs,
                settings: Mutex::new(settings),
                jobs: Mutex::new(HashMap::new()),
                _log_guard: Mutex::new(guard),
            });
            app.manage(state.clone());

            // Resume anything left running by a previous (crashed) session.
            jobs::recover_incomplete_jobs(app.handle().clone(), state.clone());

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_accounts,
            commands::add_imap_account,
            commands::add_gmail_account,
            commands::delete_account,
            commands::start_backup,
            commands::cancel_job,
            commands::list_jobs,
            commands::get_job,
            commands::search_messages,
            commands::get_settings,
            commands::save_settings,
            commands::pick_folder,
            commands::run_retention,
            commands::default_extensions,
            commands::pick_archive_files,
            commands::extract_attachments,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
