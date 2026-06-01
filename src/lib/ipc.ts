// Typed wrappers over the Tauri command surface + job event subscriptions.

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  Account,
  AppSettings,
  BackupConfig,
  ExtractProgress,
  ExtractReport,
  Filter,
  Job,
  JobProgress,
  MessageHit,
  RetentionStats,
} from "./types";

export const api = {
  listAccounts: () => invoke<Account[]>("list_accounts"),

  addImapAccount: (email: string, host: string, port: number, password: string) =>
    invoke<Account>("add_imap_account", { email, host, port, password }),

  addGmailAccount: () => invoke<Account>("add_gmail_account"),

  deleteAccount: (id: string) => invoke<void>("delete_account", { id }),

  startBackup: (config: BackupConfig) => invoke<string>("start_backup", { config }),

  cancelJob: (jobId: string) => invoke<void>("cancel_job", { jobId }),

  listJobs: (limit?: number) => invoke<Job[]>("list_jobs", { limit }),

  getJob: (id: string) => invoke<Job>("get_job", { id }),

  searchMessages: (filter: Filter, accountId?: string, limit?: number) =>
    invoke<MessageHit[]>("search_messages", { accountId, filter, limit }),

  getSettings: () => invoke<AppSettings>("get_settings"),

  saveSettings: (settings: AppSettings) => invoke<void>("save_settings", { settings }),

  pickFolder: () => invoke<string | null>("pick_folder"),

  runRetention: () => invoke<RetentionStats>("run_retention"),

  defaultExtensions: () => invoke<string[]>("default_extensions"),

  pickArchiveFiles: () => invoke<string[]>("pick_archive_files"),

  extractAttachments: (sources: string[], destination: string, extensions: string[]) =>
    invoke<ExtractReport>("extract_attachments", { sources, destination, extensions }),
};

// ---- job events ----

export function onJobProgress(cb: (p: JobProgress) => void): Promise<UnlistenFn> {
  return listen<JobProgress>("job://progress", (e) => cb(e.payload));
}

export function onJobDone(
  cb: (p: { job_id: string; messages: number; attachments: number; failed: number; warnings: string[] }) => void
): Promise<UnlistenFn> {
  return listen("job://done", (e) => cb(e.payload as never));
}

export function onJobError(cb: (p: { job_id: string; error: string }) => void): Promise<UnlistenFn> {
  return listen("job://error", (e) => cb(e.payload as never));
}

export function onExtractProgress(cb: (p: ExtractProgress) => void): Promise<UnlistenFn> {
  return listen<ExtractProgress>("extract://progress", (e) => cb(e.payload));
}

export function onExtractDone(cb: (p: ExtractReport) => void): Promise<UnlistenFn> {
  return listen<ExtractReport>("extract://done", (e) => cb(e.payload));
}
