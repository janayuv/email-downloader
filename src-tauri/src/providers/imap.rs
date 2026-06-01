//! Generic IMAP provider using `async-imap` — fully native Tokio async.
//! Works with Gmail app-passwords, Outlook, Yahoo, and custom IMAP servers.

use super::{MailProvider, MessageStream, RawMessage};
use crate::error::{AppError, Result};
use crate::model::Filter;
use async_trait::async_trait;
use chrono::TimeZone;
use futures::StreamExt;
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};

pub struct ImapProvider {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
}

impl ImapProvider {
    pub fn new(host: String, port: u16, user: String, password: String) -> Self {
        Self { host, port, user, password }
    }

    /// Connect and authenticate. The TLS stream is wrapped with `tokio-util`'s
    /// compat shim to bridge `tokio::io` traits to the `futures` traits that
    /// `async-imap` requires.
    async fn connect(
        &self,
    ) -> Result<async_imap::Session<Compat<tokio_native_tls::TlsStream<TcpStream>>>> {
        let tcp = TcpStream::connect((self.host.as_str(), self.port))
            .await
            .map_err(|e| AppError::Imap(format!("connect {}: {e}", self.host)))?;

        let tls_cx = native_tls::TlsConnector::builder()
            .build()
            .map_err(AppError::Tls)?;
        let tls_stream = tokio_native_tls::TlsConnector::from(tls_cx)
            .connect(&self.host, tcp)
            .await
            .map_err(|e| AppError::Imap(format!("tls handshake: {e}")))?;

        // .compat() bridges tokio::io::{AsyncRead,AsyncWrite} → futures::{AsyncRead,AsyncWrite}
        let client = async_imap::Client::new(tls_stream.compat());
        client
            .login(&self.user, &self.password)
            .await
            .map_err(|(e, _)| AppError::Imap(format!("login failed: {e}")))
    }
}

/// Render unix seconds as the IMAP `DD-Mon-YYYY` date form.
fn imap_date(unix: i64) -> String {
    chrono::Utc
        .timestamp_opt(unix, 0)
        .single()
        .map(|d| d.format("%d-%b-%Y").to_string())
        .unwrap_or_default()
}

fn build_search(filter: &Filter) -> String {
    let mut crit: Vec<String> = Vec::new();
    if let Some(s) = filter.since {
        crit.push(format!("SINCE {}", imap_date(s)));
    }
    if let Some(b) = filter.before {
        crit.push(format!("BEFORE {}", imap_date(b)));
    }
    if let Some(f) = filter.from.as_deref().filter(|s| !s.trim().is_empty()) {
        crit.push(format!("FROM \"{}\"", f.replace('"', "")));
    }
    if let Some(t) = filter.to.as_deref().filter(|s| !s.trim().is_empty()) {
        crit.push(format!("TO \"{}\"", t.replace('"', "")));
    }
    if let Some(c) = filter.cc.as_deref().filter(|s| !s.trim().is_empty()) {
        crit.push(format!("CC \"{}\"", c.replace('"', "")));
    }
    if let Some(s) = filter.subject.as_deref().filter(|s| !s.trim().is_empty()) {
        crit.push(format!("SUBJECT \"{}\"", s.replace('"', "")));
    }
    if let Some(t) = filter.text.as_deref().filter(|s| !s.trim().is_empty()) {
        crit.push(format!("TEXT \"{}\"", t.replace('"', "")));
    }
    if crit.is_empty() { "ALL".to_string() } else { crit.join(" ") }
}

#[async_trait]
impl MailProvider for ImapProvider {
    async fn test_connection(&self) -> Result<()> {
        let mut session = self.connect().await?;
        let _ = session.logout().await;
        Ok(())
    }

    async fn fetch(&self, filter: &Filter) -> Result<MessageStream> {
        let mut session = self.connect().await?;
        let mailbox = filter.mailbox.clone().unwrap_or_else(|| "INBOX".to_string());

        session
            .select(&mailbox)
            .await
            .map_err(|e| AppError::Imap(e.to_string()))?;

        let query = build_search(filter);
        let uid_set = session
            .uid_search(&query)
            .await
            .map_err(|e| AppError::Imap(e.to_string()))?;

        let mut uids: Vec<u32> = uid_set.into_iter().collect();
        uids.sort_unstable();

        let (tx, rx) = tokio::sync::mpsc::channel::<Result<RawMessage>>(64);

        tokio::spawn(async move {
            let result: Result<()> = async {
                for chunk in uids.chunks(200) {
                    let set = chunk.iter().map(|u| u.to_string()).collect::<Vec<_>>().join(",");

                    let fetch_stream = session
                        .uid_fetch(&set, "(UID INTERNALDATE BODY[])")
                        .await
                        .map_err(|e| AppError::Imap(e.to_string()))?;

                    // Collect the chunk so the borrow on session is released before
                    // the next uid_fetch call in the next iteration.
                    let fetches: Vec<_> = fetch_stream.collect().await;

                    for fetch_result in fetches {
                        let f = fetch_result.map_err(|e| AppError::Imap(e.to_string()))?;
                        let uid = f.uid.unwrap_or(0);
                        let internal_date =
                            f.internal_date().map(|d| d.timestamp()).unwrap_or(0);
                        let raw = f.body().unwrap_or_default().to_vec();
                        let msg = RawMessage {
                            provider_message_id: uid.to_string(),
                            uid,
                            internal_date,
                            raw,
                        };
                        if tx.send(Ok(msg)).await.is_err() {
                            // Receiver dropped (job cancelled) — stop early.
                            let _ = session.logout().await;
                            return Ok(());
                        }
                    }
                }
                let _ = session.logout().await;
                Ok(())
            }
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
