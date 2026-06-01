//! Gmail / Google Workspace provider using the Gmail REST API.
//!
//! `messages.get?format=raw` returns the original RFC822, which we decode to
//! bytes and treat exactly like an IMAP body — so the rest of the pipeline
//! (parse, hash, export) is provider-agnostic. Every call is rate-limited and
//! retried (429/5xx) via `rate_limiter`, and the access token is refreshed
//! transparently so multi-hour backups survive token expiry.

use super::{MailProvider, MessageStream, RawMessage};
use crate::auth;
use crate::error::{AppError, Result};
use crate::gmail_query;
use crate::model::Filter;
use crate::rate_limiter::{with_retry, RateLimiter};
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use futures::stream::StreamExt;
use serde::Deserialize;
use std::sync::Arc;

const API: &str = "https://gmail.googleapis.com/gmail/v1/users/me";

#[derive(Clone)]
pub struct GmailProvider {
    reference: String,
    client_id: String,
    client_secret: String,
    client: reqwest::Client,
    limiter: Arc<RateLimiter>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListResp {
    #[serde(default)]
    messages: Vec<MsgRef>,
    #[serde(default)]
    next_page_token: Option<String>,
}

#[derive(Deserialize)]
struct MsgRef {
    id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawResp {
    #[serde(default)]
    internal_date: Option<String>,
    #[serde(default)]
    raw: Option<String>,
}

fn decode_raw(s: &str) -> Result<Vec<u8>> {
    URL_SAFE
        .decode(s)
        .or_else(|_| URL_SAFE_NO_PAD.decode(s.trim_end_matches('=')))
        .map_err(|e| AppError::Parse(format!("base64: {e}")))
}

impl GmailProvider {
    pub fn new(reference: String, client_id: String, client_secret: String) -> Self {
        Self {
            reference,
            client_id,
            client_secret,
            client: reqwest::Client::new(),
            // Gmail per-user quota is generous; ~10 req/s is safe and well under it.
            limiter: Arc::new(RateLimiter::per_second(10.0)),
        }
    }

    async fn token(&self) -> Result<String> {
        auth::valid_access_token(&self.reference, &self.client_id, &self.client_secret).await
    }

    async fn get(&self, url: &str, token: &str) -> Result<reqwest::Response> {
        self.limiter.acquire().await;
        let client = self.client.clone();
        let url = url.to_string();
        let token = token.to_string();
        let resp = with_retry(5, || {
            let client = client.clone();
            let url = url.clone();
            let token = token.clone();
            async move { client.get(&url).bearer_auth(&token).send().await }
        })
        .await?;
        Ok(resp)
    }
}

#[async_trait::async_trait]
impl MailProvider for GmailProvider {
    async fn test_connection(&self) -> Result<()> {
        let token = self.token().await?;
        let resp = self.get(&format!("{API}/profile"), &token).await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(AppError::Provider(format!(
                "gmail profile {}: {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            )))
        }
    }

    async fn fetch(&self, filter: &Filter) -> Result<MessageStream> {
        let q = gmail_query::build_query(filter);
        let this = self.clone();
        let (tx, rx) = tokio::sync::mpsc::channel::<Result<RawMessage>>(64);

        tokio::spawn(async move {
            let result = (|| async {
                let token = this.token().await?;
                let mut page_token: Option<String> = None;

                loop {
                    let mut url = format!(
                        "{API}/messages?maxResults=500&q={}",
                        urlencode(&q)
                    );
                    if let Some(pt) = &page_token {
                        url.push_str(&format!("&pageToken={pt}"));
                    }
                    let resp = this.get(&url, &token).await?;
                    if !resp.status().is_success() {
                        return Err(AppError::Provider(format!(
                            "list {}: {}",
                            resp.status(),
                            resp.text().await.unwrap_or_default()
                        )));
                    }
                    let list: ListResp = resp.json().await?;

                    for m in list.messages {
                        // Refresh token opportunistically for long runs.
                        let token = this.token().await?;
                        let url = format!("{API}/messages/{}?format=raw", m.id);
                        let resp = this.get(&url, &token).await?;
                        if !resp.status().is_success() {
                            return Err(AppError::Provider(format!(
                                "get {} {}",
                                m.id,
                                resp.status()
                            )));
                        }
                        let raw_resp: RawResp = resp.json().await?;
                        let raw = match raw_resp.raw {
                            Some(r) => decode_raw(&r)?,
                            None => continue,
                        };
                        let internal_date = raw_resp
                            .internal_date
                            .and_then(|s| s.parse::<i64>().ok())
                            .map(|ms| ms / 1000)
                            .unwrap_or(0);
                        let msg = RawMessage {
                            provider_message_id: m.id.clone(),
                            uid: 0,
                            internal_date,
                            raw,
                        };
                        if tx.send(Ok(msg)).await.is_err() {
                            return Ok(()); // cancelled
                        }
                    }

                    match list.next_page_token {
                        Some(pt) => page_token = Some(pt),
                        None => break,
                    }
                }
                Ok(())
            })()
            .await;

            if let Err(e) = result {
                let _ = tx.send(Err(e)).await;
            }
        });

        let stream = futures::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|item| (item, rx))
        })
        .boxed();
        Ok(stream)
    }
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
