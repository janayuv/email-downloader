//! Tauri command surface — a thin layer that validates input and delegates to
//! the service modules. Secrets only ever touch the keychain (`auth`).

use crate::auth;
use crate::config::AppSettings;
use crate::error::{AppError, Result};
use crate::jobs;
use crate::model::*;
use crate::retention::{self, RetentionStats};
use crate::search::{self, MessageHit};
use crate::AppState;
use std::sync::Arc;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

type AppArc<'a> = State<'a, Arc<AppState>>;

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[tauri::command]
pub fn list_accounts(state: AppArc) -> Result<Vec<Account>> {
    state.db.list_accounts()
}

#[tauri::command]
pub async fn add_imap_account(
    state: AppArc<'_>,
    email: String,
    host: String,
    port: u16,
    password: String,
) -> Result<Account> {
    use crate::providers::{imap::ImapProvider, MailProvider};

    // Validate credentials before persisting anything.
    let provider = ImapProvider::new(host.clone(), port, email.clone(), password.clone());
    provider.test_connection().await?;

    let reference = format!("imap-{}", uuid::Uuid::new_v4());
    auth::store_secret(&reference, &password)?;

    let account = Account {
        id: uuid::Uuid::new_v4().to_string(),
        provider: Provider::Imap,
        email: email.clone(),
        label: email,
        imap_host: host,
        imap_port: port,
        keyring_reference: reference,
        created_at: now(),
    };
    state.db.insert_account(&account)?;
    Ok(account)
}

#[derive(serde::Deserialize)]
struct GmailProfile {
    #[serde(rename = "emailAddress")]
    email_address: String,
}

#[tauri::command]
pub async fn add_gmail_account(app: AppHandle, state: AppArc<'_>) -> Result<Account> {
    let (client_id, client_secret) = {
        let s = state.settings.lock().unwrap();
        (s.google_client_id.clone(), s.google_client_secret.clone())
    };

    let app_for_open = app.clone();
    let tokens = auth::run_google_oauth(&client_id, &client_secret, move |url| {
        let _ = app_for_open.opener().open_url(url.to_string(), None::<&str>);
    })
    .await?;

    // Resolve the account email from the Gmail profile.
    let client = reqwest::Client::new();
    let resp = client
        .get("https://gmail.googleapis.com/gmail/v1/users/me/profile")
        .bearer_auth(&tokens.access_token)
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(AppError::Auth(format!("profile lookup: {}", resp.status())));
    }
    let profile: GmailProfile = resp.json().await?;

    let reference = format!("gmail-{}", uuid::Uuid::new_v4());
    auth::store_tokens(&reference, &tokens)?;

    let account = Account {
        id: uuid::Uuid::new_v4().to_string(),
        provider: Provider::Gmail,
        email: profile.email_address.clone(),
        label: profile.email_address,
        imap_host: String::new(),
        imap_port: 0,
        keyring_reference: reference,
        created_at: now(),
    };
    state.db.insert_account(&account)?;
    Ok(account)
}

#[tauri::command]
pub fn delete_account(state: AppArc, id: String) -> Result<()> {
    let account = state.db.get_account(&id)?;
    let _ = auth::delete_secret(&account.keyring_reference);
    state.db.delete_account(&id)
}

#[tauri::command]
pub async fn start_backup(
    app: AppHandle,
    state: AppArc<'_>,
    config: BackupConfig,
) -> Result<String> {
    let state = state.inner().clone();
    jobs::start_backup(app, state, config).await
}

#[tauri::command]
pub fn cancel_job(state: AppArc, job_id: String) -> Result<()> {
    jobs::cancel_job(state.inner(), &job_id)
}

#[tauri::command]
pub fn list_jobs(state: AppArc, limit: Option<i64>) -> Result<Vec<Job>> {
    state.db.list_jobs(limit.unwrap_or(100))
}

#[tauri::command]
pub fn get_job(state: AppArc, id: String) -> Result<Job> {
    state.db.get_job(&id)
}

#[tauri::command]
pub fn search_messages(
    state: AppArc,
    account_id: Option<String>,
    filter: Filter,
    limit: Option<i64>,
) -> Result<Vec<MessageHit>> {
    search::search(&state.db, account_id.as_deref(), &filter, limit.unwrap_or(200))
}

#[tauri::command]
pub fn get_settings(state: AppArc) -> Result<AppSettings> {
    Ok(state.settings.lock().unwrap().clone())
}

#[tauri::command]
pub fn save_settings(state: AppArc, settings: AppSettings) -> Result<()> {
    settings.save(&state.paths.settings_path())?;
    *state.settings.lock().unwrap() = settings;
    Ok(())
}

#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Result<Option<String>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |f| {
        let _ = tx.send(f);
    });
    let picked = rx.await.map_err(|_| AppError::other("folder dialog closed"))?;
    Ok(picked.and_then(|p| p.into_path().ok()).map(|pb| pb.to_string_lossy().into_owned()))
}

#[tauri::command]
pub fn run_retention(state: AppArc) -> Result<RetentionStats> {
    let settings = state.settings.lock().unwrap().clone();
    retention::run(&state.paths, &state.db, &settings)
}

#[tauri::command]
pub fn default_extensions() -> Vec<String> {
    DEFAULT_EXTENSIONS.iter().map(|s| s.to_string()).collect()
}

#[tauri::command]
pub async fn pick_archive_files(app: AppHandle) -> Result<Vec<String>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("Email archives", &["eml", "mbox", "msg", "pst"])
        .pick_files(move |files| {
            let _ = tx.send(files);
        });
    let picked = rx.await.map_err(|_| AppError::other("file dialog closed"))?;
    Ok(picked
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| p.into_path().ok())
        .map(|pb| pb.to_string_lossy().into_owned())
        .collect())
}

#[tauri::command]
pub async fn extract_attachments(
    app: AppHandle,
    sources: Vec<String>,
    destination: String,
    extensions: Vec<String>,
) -> Result<crate::extract::ExtractReport> {
    crate::extract::run(app, sources, destination, extensions).await
}
