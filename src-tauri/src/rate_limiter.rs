//! Quota protection for the Gmail API: a spacing-based rate limiter plus a
//! retry helper with exponential backoff + jitter on HTTP 429 and 5xx (and on
//! transient transport errors). Production Gmail backups WILL hit per-user rate
//! limits on large mailboxes, so every API call funnels through here.

use std::sync::Mutex;
use std::time::{Duration, Instant};

pub struct RateLimiter {
    interval: Duration,
    last: Mutex<Instant>,
}

impl RateLimiter {
    /// Allow at most `rps` requests per second (evenly spaced).
    pub fn per_second(rps: f64) -> Self {
        let interval = if rps > 0.0 {
            Duration::from_secs_f64(1.0 / rps)
        } else {
            Duration::ZERO
        };
        Self {
            interval,
            last: Mutex::new(Instant::now() - interval),
        }
    }

    /// Reserve the next slot and sleep until it is due.
    pub async fn acquire(&self) {
        let wait = {
            let mut last = self.last.lock().unwrap();
            let now = Instant::now();
            let earliest = *last + self.interval;
            let w = earliest.saturating_duration_since(now);
            *last = if earliest > now { earliest } else { now };
            w
        };
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
    }
}

/// Execute `f` with retries. `f` is called repeatedly; it should perform one
/// rate-limited request and return the `reqwest::Response`. The response is
/// retried when its status is 429 or 5xx; transport errors are also retried.
pub async fn with_retry<F, Fut>(max_retries: u32, mut f: F) -> reqwest::Result<reqwest::Response>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = reqwest::Result<reqwest::Response>>,
{
    let base = Duration::from_millis(500);
    let max_delay = Duration::from_secs(30);
    let mut attempt = 0u32;

    loop {
        match f().await {
            Ok(resp) => {
                let status = resp.status();
                let retryable = status.as_u16() == 429 || status.is_server_error();
                if retryable && attempt < max_retries {
                    let delay = backoff(base, attempt, max_delay);
                    tracing::warn!(%status, attempt, ?delay, "retrying after retryable status");
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                    continue;
                }
                return Ok(resp);
            }
            Err(e) => {
                if attempt < max_retries && (e.is_timeout() || e.is_connect() || e.is_request()) {
                    let delay = backoff(base, attempt, max_delay);
                    tracing::warn!(error=%e, attempt, ?delay, "retrying after transport error");
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                    continue;
                }
                return Err(e);
            }
        }
    }
}

fn backoff(base: Duration, attempt: u32, max_delay: Duration) -> Duration {
    let exp = base.saturating_mul(1u32 << attempt.min(6));
    let capped = exp.min(max_delay);
    // Deterministic-ish jitter from the system clock — avoids a rand dependency.
    let nanos = Instant::now().elapsed().subsec_nanos() as u64;
    let jitter = Duration::from_millis(nanos % 250);
    capped + jitter
}
