// TypeScript mirrors of the Rust serde model (serialized lowercase).

export type Provider = "imap" | "gmail";
export type JobStatus =
  | "pending"
  | "running"
  | "completed"
  | "failed"
  | "cancelled";
export type ExportFormat = "eml" | "mbox" | "msg" | "pst";

export interface Account {
  id: string;
  provider: Provider;
  email: string;
  label: string;
  imap_host: string;
  imap_port: number;
  keyring_reference: string;
  created_at: number;
}

export interface Filter {
  from?: string | null;
  to?: string | null;
  cc?: string | null;
  subject?: string | null;
  text?: string | null;
  since?: number | null;
  before?: number | null;
  has_attachment?: boolean | null;
  extensions: string[];
  mailbox?: string | null;
}

export interface BackupConfig {
  account_id: string;
  filter: Filter;
  formats: ExportFormat[];
  destination: string;
  download_attachments: boolean;
  keep_raw: boolean;
}

export interface Job {
  id: string;
  account_id: string;
  status: JobStatus;
  started_at: number;
  completed_at: number | null;
  checkpoint: string | null;
  messages_done: number;
  attachments_done: number;
  failed: number;
  config_json: string;
}

export interface JobProgress {
  job_id: string;
  status: JobStatus;
  messages_done: number;
  attachments_done: number;
  failed: number;
  total: number | null;
  current: string;
}

export interface MessageHit {
  id: string;
  account_id: string;
  subject: string;
  from_addr: string;
  to_addr: string;
  internal_date: number;
  has_attachments: boolean;
  size: number;
}

export interface AppSettings {
  default_destination: string;
  theme: string;
  log_retention_days: number;
  report_retention_count: number;
  google_client_id: string;
  google_client_secret: string;
}

export interface RetentionStats {
  logs_deleted: number;
  reports_deleted: number;
  blobs_deleted: number;
  bytes_reclaimed: number;
}

export interface ExtractReport {
  files_processed: number;
  attachments_extracted: number;
  skipped_filtered: number;
  errors: string[];
  output_root: string;
}

export interface ExtractProgress {
  file: string;
  files_processed: number;
  attachments_extracted: number;
}
