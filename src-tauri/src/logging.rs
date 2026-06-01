//! Structured logging via `tracing`, writing rotating daily files under
//! `logs/` plus a console layer. Initialized once at startup.

use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// Initialize tracing. The returned guard must be kept alive for the lifetime
/// of the app so the non-blocking writer flushes on shutdown.
pub fn init(logs_dir: &Path) -> WorkerGuard {
    let file_appender = tracing_appender::rolling::daily(logs_dir, "email-downloader.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let filter = EnvFilter::try_from_env("ED_LOG")
        .unwrap_or_else(|_| EnvFilter::new("info,email_downloader_lib=debug"));

    let file_layer = fmt::layer()
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_target(true);

    let console_layer = fmt::layer().with_target(false);

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(console_layer)
        .try_init();

    tracing::info!("logging initialized");
    guard
}
