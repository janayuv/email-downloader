//! Streaming backup job runner.
//!
//! A job pulls a `Stream<RawMessage>` from a provider and processes each message
//! incrementally — hash, dedupe, index (+FTS5), download attachments, feed every
//! exporter — so memory stays flat regardless of mailbox size. Progress is
//! checkpointed and emitted as Tauri events. Because dedupe is keyed on the raw
//! sha256, **re-running a job is idempotent**, which is exactly how crash
//! recovery resumes an interrupted backup.

use crate::auth;
use crate::error::{AppError, Result};
use crate::export::{self, Exporter};
use crate::model::*;
use crate::parser;
use crate::providers::{gmail::GmailProvider, imap::ImapProvider, MailProvider};
use crate::report::BackupReport;
use crate::{hashing, AppState};
use futures::StreamExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

const PROGRESS_EVERY: i64 = 10;

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Build the right provider for an account, loading credentials transiently from
/// the OS keychain (never from the DB).
async fn build_provider(state: &AppState, account: &Account) -> Result<Box<dyn MailProvider>> {
    match account.provider {
        Provider::Imap => {
            let password = auth::get_secret(&account.keyring_reference)?;
            Ok(Box::new(ImapProvider::new(
                account.imap_host.clone(),
                account.imap_port,
                account.email.clone(),
                password,
            )))
        }
        Provider::Gmail => {
            let (cid, secret) = {
                let s = state.settings.lock().unwrap();
                (s.google_client_id.clone(), s.google_client_secret.clone())
            };
            Ok(Box::new(GmailProvider::new(
                account.keyring_reference.clone(),
                cid,
                secret,
            )))
        }
    }
}

/// Launch a backup. Creates the job row, registers a cancel flag, and spawns the
/// runner. Returns the job id immediately.
pub async fn start_backup(
    app: AppHandle,
    state: Arc<AppState>,
    config: BackupConfig,
) -> Result<String> {
    let job_id = uuid::Uuid::new_v4().to_string();
    let job = Job {
        id: job_id.clone(),
        account_id: config.account_id.clone(),
        status: JobStatus::Running,
        started_at: now(),
        completed_at: None,
        checkpoint: None,
        messages_done: 0,
        attachments_done: 0,
        failed: 0,
        config_json: serde_json::to_string(&config)?,
    };
    state.db.create_job(&job)?;

    let cancel = Arc::new(AtomicBool::new(false));
    state
        .jobs
        .lock()
        .unwrap()
        .insert(job_id.clone(), cancel.clone());

    let jid = job_id.clone();
    tokio::spawn(async move {
        if let Err(e) = run_job(app.clone(), state.clone(), jid.clone(), config, cancel).await {
            tracing::error!(job = %jid, error = %e, "job failed");
            let _ = state.db.set_job_status(&jid, JobStatus::Failed, Some(now()));
            let _ = app.emit(
                "job://error",
                serde_json::json!({ "job_id": jid, "error": e.to_string() }),
            );
        }
        state.jobs.lock().unwrap().remove(&jid);
    });

    Ok(job_id)
}

#[allow(clippy::too_many_lines)]
async fn run_job(
    app: AppHandle,
    state: Arc<AppState>,
    job_id: String,
    config: BackupConfig,
    cancel: Arc<AtomicBool>,
) -> Result<()> {
    let account = state.db.get_account(&config.account_id)?;
    let provider = build_provider(&state, &account).await?;
    let dest = std::path::PathBuf::from(&config.destination);
    std::fs::create_dir_all(&dest)?;

    let mut exporters = export::create_exporters(&config.formats, &dest)?;
    let ext_filter: Vec<String> = config
        .filter
        .extensions
        .iter()
        .map(|e| e.to_lowercase())
        .collect();

    let started = now();
    let mut messages_done: i64 = 0;
    let mut attachments_done: i64 = 0;
    let mut failed: i64 = 0;

    let mut stream = provider.fetch(&config.filter).await?;

    while let Some(item) = stream.next().await {
        if cancel.load(Ordering::Relaxed) {
            state
                .db
                .set_job_status(&job_id, JobStatus::Cancelled, Some(now()))?;
            emit_progress(&app, &job_id, JobStatus::Cancelled, messages_done, attachments_done, failed, "cancelled");
            tracing::info!(job = %job_id, "job cancelled");
            return Ok(());
        }

        let raw = match item {
            Ok(m) => m,
            Err(e) => {
                failed += 1;
                tracing::warn!(job = %job_id, error = %e, "stream item error");
                continue;
            }
        };

        if let Err(e) = process_message(&state, &account, &config, &ext_filter, &mut exporters, &raw, &mut attachments_done) {
            failed += 1;
            tracing::warn!(job = %job_id, error = %e, "message processing error");
            continue;
        }

        messages_done += 1;
        if messages_done % PROGRESS_EVERY == 0 {
            let checkpoint = serde_json::json!({
                "last_provider_id": raw.provider_message_id,
                "last_uid": raw.uid,
                "last_internal_date": raw.internal_date,
            })
            .to_string();
            state.db.update_job_progress(
                &job_id,
                messages_done,
                attachments_done,
                failed,
                Some(&checkpoint),
            )?;
            emit_progress(&app, &job_id, JobStatus::Running, messages_done, attachments_done, failed, &raw.provider_message_id);
        }
    }

    // Finalize + verify every exporter; export problems are warnings, not job
    // failures (e.g. PST sidecar missing) — the other formats still succeed.
    let mut warnings: Vec<String> = Vec::new();
    for ex in exporters.iter_mut() {
        let fmt = ex.format();
        if let Err(e) = ex.finish() {
            warnings.push(format!("{}: {e}", fmt.as_str()));
            continue;
        }
        if let Err(e) = ex.verify() {
            warnings.push(format!("{} verify: {e}", fmt.as_str()));
        }
    }

    let completed = now();
    state
        .db
        .update_job_progress(&job_id, messages_done, attachments_done, failed, None)?;
    state
        .db
        .set_job_status(&job_id, JobStatus::Completed, Some(completed))?;

    let report = BackupReport {
        job_id: job_id.clone(),
        account_email: account.email.clone(),
        status: JobStatus::Completed.as_str().to_string(),
        started_at: started,
        completed_at: completed,
        duration_seconds: completed - started,
        messages: messages_done,
        attachments: attachments_done,
        failed,
        exports: config.formats.iter().map(|f| f.as_str().to_string()).collect(),
        warnings: warnings.clone(),
    };
    let _ = report.write_to(&state.paths.reports_dir);
    let _ = report.write_to(&dest);

    emit_progress(&app, &job_id, JobStatus::Completed, messages_done, attachments_done, failed, "done");
    let _ = app.emit(
        "job://done",
        serde_json::json!({
            "job_id": job_id,
            "messages": messages_done,
            "attachments": attachments_done,
            "failed": failed,
            "warnings": warnings,
        }),
    );
    tracing::info!(job = %job_id, messages_done, attachments_done, failed, "job complete");
    Ok(())
}

/// Process a single raw message: hash → dedupe → parse → index → attachments →
/// feed exporters.
fn process_message(
    state: &AppState,
    account: &Account,
    config: &BackupConfig,
    ext_filter: &[String],
    exporters: &mut [Box<dyn Exporter>],
    raw: &crate::providers::RawMessage,
    attachments_done: &mut i64,
) -> Result<()> {
    let sha = hashing::sha256_hex(&raw.raw);

    // Dedupe: a message already stored for this account is skipped entirely.
    if state.db.message_exists(&account.id, &sha)? {
        return Ok(());
    }

    let parsed = parser::parse(&raw.raw)?;

    let raw_path = if config.keep_raw {
        state
            .blobs
            .put_message(&sha, &raw.raw)?
            .to_string_lossy()
            .into_owned()
    } else {
        String::new()
    };

    let internal_date = if raw.internal_date > 0 {
        raw.internal_date
    } else {
        parsed.date
    };

    let msg_id = uuid::Uuid::new_v4().to_string();
    let record = MessageRecord {
        id: msg_id.clone(),
        account_id: account.id.clone(),
        message_id: parsed.message_id.clone(),
        provider_message_id: raw.provider_message_id.clone(),
        last_uid: raw.uid,
        internal_date,
        sha256: sha.clone(),
        from_addr: parsed.from_addr.clone(),
        to_addr: parsed.to_addr.clone(),
        cc_addr: parsed.cc_addr.clone(),
        subject: parsed.subject.clone(),
        has_attachments: !parsed.attachments.is_empty(),
        size: raw.raw.len() as i64,
        raw_path,
    };
    state.db.upsert_message(&record, &parsed.body_preview)?;

    // Attachments (extension filter; empty filter = all).
    if config.download_attachments {
        for att in &parsed.attachments {
            if !ext_filter.is_empty() && !ext_filter.contains(&att.ext) {
                continue;
            }
            let (asha, path) = state.blobs.put_attachment(&att.ext, &att.data)?;
            let rec = AttachmentRecord {
                id: uuid::Uuid::new_v4().to_string(),
                message_id: msg_id.clone(),
                filename: att.filename.clone(),
                ext: att.ext.clone(),
                size: att.data.len() as i64,
                sha256: asha,
                blob_path: path.to_string_lossy().into_owned(),
            };
            state.db.insert_attachment(&rec)?;
            *attachments_done += 1;
        }
    }

    // Feed exporters.
    for ex in exporters.iter_mut() {
        ex.add(&raw.raw, &parsed, &sha)?;
    }

    Ok(())
}

fn emit_progress(
    app: &AppHandle,
    job_id: &str,
    status: JobStatus,
    messages_done: i64,
    attachments_done: i64,
    failed: i64,
    current: &str,
) {
    let payload = JobProgress {
        job_id: job_id.to_string(),
        status,
        messages_done,
        attachments_done,
        failed,
        total: None,
        current: current.to_string(),
    };
    let _ = app.emit("job://progress", payload);
}

/// On startup, resume jobs left `running`/`pending`. Because processing is
/// idempotent (sha dedupe), we simply relaunch each with its saved config; any
/// already-stored message is skipped.
pub fn recover_incomplete_jobs(app: AppHandle, state: Arc<AppState>) {
    let jobs = match state.db.incomplete_jobs() {
        Ok(j) => j,
        Err(e) => {
            tracing::error!(error = %e, "could not query incomplete jobs");
            return;
        }
    };
    for job in jobs {
        let config: BackupConfig = match serde_json::from_str(&job.config_json) {
            Ok(c) => c,
            Err(_) => {
                let _ = state.db.set_job_status(&job.id, JobStatus::Failed, Some(now()));
                continue;
            }
        };
        tracing::info!(job = %job.id, "recovering incomplete job");
        let cancel = Arc::new(AtomicBool::new(false));
        state.jobs.lock().unwrap().insert(job.id.clone(), cancel.clone());
        let app = app.clone();
        let state2 = state.clone();
        let jid = job.id.clone();
        tokio::spawn(async move {
            if let Err(e) = run_job(app.clone(), state2.clone(), jid.clone(), config, cancel).await {
                tracing::error!(job = %jid, error = %e, "recovered job failed");
                let _ = state2.db.set_job_status(&jid, JobStatus::Failed, Some(now()));
            }
            state2.jobs.lock().unwrap().remove(&jid);
        });
    }
}

/// Signal a running job to stop at the next message boundary.
pub fn cancel_job(state: &AppState, job_id: &str) -> Result<()> {
    if let Some(flag) = state.jobs.lock().unwrap().get(job_id) {
        flag.store(true, Ordering::Relaxed);
        Ok(())
    } else {
        Err(AppError::NotFound(format!("running job {job_id}")))
    }
}
