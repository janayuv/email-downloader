# Email Downloader

A premium, production-grade **email attachment downloader & Google Workspace backup**
desktop app for Windows. Built with **Tauri 2 + Vite + React + TypeScript + Tailwind +
shadcn/ui** on the front, and a **Rust** backend doing the heavy lifting (IMAP, Gmail
API, parsing, export, storage).

> Milestone 1 — single-account backup loop for **Gmail/Workspace (OAuth)** and **generic
> IMAP**, exporting to **EML / MBOX / MSG / PST**, with content-addressed storage,
> FTS5 search, streaming (memory-flat) processing, crash recovery, and retention.

## Features (Milestone 1)

- **Providers:** Gmail/Google Workspace via OAuth 2.0 (PKCE loopback) + Gmail API, and
  generic IMAP (app-password) for any server.
- **Export formats:** EML, MBOX (mboxrd), MSG (Outlook compound file), PST (via sidecar).
  Every exporter implements `verify()` — output is re-parsed/re-opened, never assumed OK.
- **Attachments:** streamed MIME extraction with an extension filter (PDF, XLSX, DOCX,
  PPTX, …), stored content-addressed (`storage/attachments/<ab>/<sha256>.ext`) for dedupe.
- **Search:** local **FTS5** index over subject/from/to/body (`Browse`), plus a separate
  Gmail `q` translator for server-side filtering.
- **Extract from archives:** pull attachments out of existing **EML / MBOX / MSG / PST**
  files with an extension filter (`Extract`). EML/MBOX/MSG parsed natively (MSG via the
  `cfb` compound-file reader); PST read through the sidecar. Output is per-source subfolders
  with original filenames (collision-suffixed).
- **Scale:** end-to-end **streaming** (`Stream<Message>`) — 500k-message mailboxes never
  load into memory.
- **Reliability:** SQLite job table with checkpoints + **crash recovery** (idempotent
  re-run via sha256 dedupe), `backup-report.json` per job, rotating `tracing` logs.
- **Security:** passwords & OAuth tokens live **only** in the OS keychain; SQLite stores a
  `keyring_reference`, never a secret.
- **Retention:** age-out logs, cap report history, GC orphaned blobs.

## Prerequisites

- Node.js 18+ and npm
- Rust (stable) + the Tauri 2 prerequisites for Windows (MSVC build tools, WebView2)

## Develop

```sh
npm install
npm run tauri dev
```

## Build

```sh
npm run tauri build
```

## Configuration

- **Gmail:** create a Google Cloud project with an **OAuth Desktop app** credential, then
  paste the client id (and secret, if any) in **Settings → Google Workspace**. Scopes
  used: `gmail.readonly` + `userinfo.email`.
- **PST:** PST has no open-source writer. Point `ED_PST_SIDECAR` at (or bundle next to the
  app) a `pst-export` tool — a .NET console app using Aspose.Email — exposing
  `export --input <emlDir> --output <pst>` and `verify --file <pst>`. Without it, EML/MBOX/
  MSG still complete and PST surfaces a clear "not configured" message.

## Architecture

```
src/                      React UI (features/{accounts,backup,jobs,browse,settings})
  lib/ipc.ts              typed wrappers over Tauri commands + job events
src-tauri/src/
  model.rs storage.rs     domain model + SQLite schema (FTS5, dedupe, jobs)
  blobstore.rs hashing.rs content-addressed storage + sha256
  providers/{imap,gmail}  MailProvider trait -> Stream<RawMessage>
  auth/                   keychain secrets + Google OAuth (PKCE loopback)
  rate_limiter.rs         Gmail throttle + backoff/retry (429/5xx)
  parser.rs               mail-parser wrapper
  export/{eml,mbox,msg,pst}  Exporter trait with write() + verify()
  search.rs gmail_query.rs    local FTS5 search vs. Gmail `q` (kept separate)
  jobs.rs                 streaming job runner + checkpoints + recovery
  report.rs retention.rs  backup-report.json + disk hygiene
```

## Roadmap (deferred)

Contacts & Calendar backup · extract attachments from existing `.eml` · incremental
backups · bulk user/domain backup (Workspace delegation) · audit logs · licensing/
activation · Microsoft 365 (Graph) provider. The schema and provider/exporter traits are
shaped to absorb these without redesign.
