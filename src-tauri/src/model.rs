//! Domain model shared across the backend and serialized to the frontend.
//!
//! The message schema deliberately stores `provider_message_id`, `last_uid`,
//! `internal_date` and `sha256` from day one so incremental backup, dedupe and
//! integrity verification become queries later rather than schema migrations.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Imap,
    Gmail,
}

impl Provider {
    pub fn as_str(&self) -> &'static str {
        match self {
            Provider::Imap => "imap",
            Provider::Gmail => "gmail",
        }
    }
}

impl std::str::FromStr for Provider {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "imap" => Ok(Provider::Imap),
            "gmail" => Ok(Provider::Gmail),
            _ => Err(()),
        }
    }
}

/// An account row. Secrets are NEVER stored here — only a `keyring_reference`
/// that points at the OS keychain entry holding the password / OAuth tokens.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub provider: Provider,
    pub email: String,
    /// Display label, defaults to the email.
    pub label: String,
    /// IMAP only — host/port. Empty for Gmail.
    #[serde(default)]
    pub imap_host: String,
    #[serde(default)]
    pub imap_port: u16,
    /// Keychain entry key. The value lives in the OS keychain, not the DB.
    pub keyring_reference: String,
    pub created_at: i64,
}

/// Credentials are loaded transiently from the keychain and never persisted in
/// the database or serialized to the frontend.
#[allow(dead_code)] // used by provider-builder paths added in later milestones
#[derive(Debug, Clone)]
pub enum Credential {
    Password(String),
    OAuth(OAuthTokens),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: String,
    /// Unix seconds at which the access token expires.
    pub expires_at: i64,
}

/// Stored message metadata. The raw body and attachments live on disk; the DB
/// holds the index + integrity hash + the cursors incremental backup needs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageRecord {
    pub id: String,
    pub account_id: String,
    /// RFC822 Message-ID header.
    pub message_id: String,
    /// Gmail message id or the IMAP UID source — provider-native identity.
    pub provider_message_id: String,
    /// IMAP UID (0 for Gmail).
    pub last_uid: u32,
    /// Unix seconds — the server-side internal date.
    pub internal_date: i64,
    /// sha256 of the raw RFC822 bytes (lowercase hex).
    pub sha256: String,
    pub from_addr: String,
    pub to_addr: String,
    pub cc_addr: String,
    pub subject: String,
    pub has_attachments: bool,
    pub size: i64,
    /// Path to the persisted raw .eml on disk, if kept.
    #[serde(default)]
    pub raw_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentRecord {
    pub id: String,
    pub message_id: String,
    pub filename: String,
    pub ext: String,
    pub size: i64,
    pub sha256: String,
    pub blob_path: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            JobStatus::Pending => "pending",
            JobStatus::Running => "running",
            JobStatus::Completed => "completed",
            JobStatus::Failed => "failed",
            JobStatus::Cancelled => "cancelled",
        }
    }
}

impl std::str::FromStr for JobStatus {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(JobStatus::Pending),
            "running" => Ok(JobStatus::Running),
            "completed" => Ok(JobStatus::Completed),
            "failed" => Ok(JobStatus::Failed),
            "cancelled" => Ok(JobStatus::Cancelled),
            _ => Err(()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    pub account_id: String,
    pub status: JobStatus,
    pub started_at: i64,
    pub completed_at: Option<i64>,
    /// Resumable cursor (provider page token / last UID processed) as JSON.
    pub checkpoint: Option<String>,
    pub messages_done: i64,
    pub attachments_done: i64,
    pub failed: i64,
    /// Serialized BackupConfig used to launch the job (so recovery can resume).
    pub config_json: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Eml,
    Mbox,
    Msg,
    Pst,
}

impl ExportFormat {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExportFormat::Eml => "eml",
            ExportFormat::Mbox => "mbox",
            ExportFormat::Msg => "msg",
            ExportFormat::Pst => "pst",
        }
    }
}

/// Structured search/backup filter. Used by both the local FTS5 search
/// (`search.rs`) and the Gmail `q` translator (`gmail_query.rs`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Filter {
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub cc: Option<String>,
    #[serde(default)]
    pub subject: Option<String>,
    /// Free-text query (matched against subject/from/to/body in FTS5).
    #[serde(default)]
    pub text: Option<String>,
    /// Unix seconds (inclusive lower bound).
    #[serde(default)]
    pub since: Option<i64>,
    /// Unix seconds (exclusive upper bound).
    #[serde(default)]
    pub before: Option<i64>,
    #[serde(default)]
    pub has_attachment: Option<bool>,
    /// Lowercase extensions without the dot, e.g. ["pdf","docx"].
    #[serde(default)]
    pub extensions: Vec<String>,
    /// IMAP mailbox / Gmail label, optional.
    #[serde(default)]
    pub mailbox: Option<String>,
}

/// Everything needed to launch (or resume) a backup job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupConfig {
    pub account_id: String,
    pub filter: Filter,
    pub formats: Vec<ExportFormat>,
    /// Destination folder for exports + attachments.
    pub destination: String,
    /// Download attachments matching `filter.extensions` (empty = all).
    #[serde(default = "default_true")]
    pub download_attachments: bool,
    /// Persist the raw .eml of each message into the blob store.
    #[serde(default = "default_true")]
    pub keep_raw: bool,
}

fn default_true() -> bool {
    true
}

/// Progress event payload emitted on `job://progress`.
#[derive(Debug, Clone, Serialize)]
pub struct JobProgress {
    pub job_id: String,
    pub status: JobStatus,
    pub messages_done: i64,
    pub attachments_done: i64,
    pub failed: i64,
    /// Optional total when known (IMAP search count); None while streaming Gmail.
    pub total: Option<i64>,
    pub current: String,
}

/// The default set of attachment extensions surfaced in the UI.
pub const DEFAULT_EXTENSIONS: &[&str] = &[
    "pdf", "xlsx", "xls", "doc", "docx", "ppt", "pptx", "csv", "txt", "zip", "rar", "png", "jpg",
    "jpeg",
];
