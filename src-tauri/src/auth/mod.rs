//! Secret storage + Google OAuth.
//!
//! SECURITY: the SQLite database never holds a password or token. Every secret
//! lives in the OS keychain under `SERVICE`, addressed by a `keyring_reference`
//! that the `accounts` table stores in its place.

use crate::error::{AppError, Result};
use crate::model::OAuthTokens;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};
use std::io::ErrorKind;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const SERVICE: &str = "com.emaildownloader.app";

const AUTH_ENDPOINT: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_ENDPOINT: &str = "https://oauth2.googleapis.com/token";
const SCOPES: &str = "https://www.googleapis.com/auth/gmail.readonly https://www.googleapis.com/auth/userinfo.email";

// ---- keychain ----

pub fn store_secret(reference: &str, secret: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, reference)?;
    entry.set_password(secret)?;
    Ok(())
}

pub fn get_secret(reference: &str) -> Result<String> {
    let entry = keyring::Entry::new(SERVICE, reference)?;
    Ok(entry.get_password()?)
}

pub fn delete_secret(reference: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, reference)?;
    // Missing entries are not an error on delete.
    match entry.delete_password() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

pub fn store_tokens(reference: &str, tokens: &OAuthTokens) -> Result<()> {
    store_secret(reference, &serde_json::to_string(tokens)?)
}

pub fn get_tokens(reference: &str) -> Result<OAuthTokens> {
    let raw = get_secret(reference)?;
    Ok(serde_json::from_str(&raw)?)
}

// ---- PKCE ----

fn random_token() -> String {
    // Two v4 UUIDs give 256 bits of entropy without a separate rand dep.
    let a = uuid::Uuid::new_v4();
    let b = uuid::Uuid::new_v4();
    let mut bytes = Vec::with_capacity(32);
    bytes.extend_from_slice(a.as_bytes());
    bytes.extend_from_slice(b.as_bytes());
    URL_SAFE_NO_PAD.encode(bytes)
}

fn code_challenge(verifier: &str) -> String {
    let mut h = Sha256::new();
    h.update(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(h.finalize())
}

// ---- OAuth flow ----

/// Run the full installed-app OAuth flow. `open_url` is invoked with the consent
/// URL (the caller opens it in the system browser). Returns tokens on success.
pub async fn run_google_oauth<F>(
    client_id: &str,
    client_secret: &str,
    open_url: F,
) -> Result<OAuthTokens>
where
    F: FnOnce(&str),
{
    if client_id.trim().is_empty() {
        return Err(AppError::Auth(
            "Google OAuth client id is not configured (Settings → Google client id)".into(),
        ));
    }

    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}");

    let state = random_token();
    let verifier = random_token();
    let challenge = code_challenge(&verifier);

    let auth_url = format!(
        "{AUTH_ENDPOINT}?response_type=code&client_id={cid}&redirect_uri={ru}\
         &scope={scope}&state={state}&code_challenge={chal}&code_challenge_method=S256\
         &access_type=offline&prompt=consent",
        cid = urlencode(client_id),
        ru = urlencode(&redirect_uri),
        scope = urlencode(SCOPES),
        state = state,
        chal = challenge,
    );

    open_url(&auth_url);

    let code = wait_for_code(listener, &state).await?;

    exchange_code(client_id, client_secret, &code, &verifier, &redirect_uri).await
}

/// Accept a single loopback request and pull the `code` out of the query string.
async fn wait_for_code(listener: TcpListener, expected_state: &str) -> Result<String> {
    let accept = async {
        loop {
            let (mut socket, _) = listener.accept().await?;
            let mut buf = vec![0u8; 8192];
            let n = socket.read(&mut buf).await?;
            let req = String::from_utf8_lossy(&buf[..n]);
            let first_line = req.lines().next().unwrap_or("");
            // GET /?code=...&state=... HTTP/1.1
            let path = first_line.split_whitespace().nth(1).unwrap_or("");
            let (code, state) = parse_query(path);

            let ok = code.is_some() && state.as_deref() == Some(expected_state);
            let body = if ok {
                "Authentication complete. You can close this window and return to Email Downloader."
            } else {
                "Authentication failed or was cancelled. You can close this window."
            };
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = socket.write_all(resp.as_bytes()).await;
            let _ = socket.flush().await;

            if ok {
                return Ok::<String, std::io::Error>(code.unwrap());
            }
            if code.is_some() {
                // state mismatch — treat as failure
                return Err(std::io::Error::new(ErrorKind::Other, "state mismatch"));
            }
            // otherwise keep waiting (favicon etc.)
        }
    };

    match tokio::time::timeout(Duration::from_secs(300), accept).await {
        Ok(Ok(code)) => Ok(code),
        Ok(Err(e)) => Err(AppError::Auth(format!("loopback: {e}"))),
        Err(_) => Err(AppError::Auth("OAuth timed out waiting for consent".into())),
    }
}

fn parse_query(path: &str) -> (Option<String>, Option<String>) {
    let q = path.split_once('?').map(|(_, q)| q).unwrap_or("");
    let mut code = None;
    let mut state = None;
    for pair in q.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            let val = urldecode(v);
            match k {
                "code" => code = Some(val),
                "state" => state = Some(val),
                _ => {}
            }
        }
    }
    (code, state)
}

#[derive(serde::Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

async fn exchange_code(
    client_id: &str,
    client_secret: &str,
    code: &str,
    verifier: &str,
    redirect_uri: &str,
) -> Result<OAuthTokens> {
    let client = reqwest::Client::new();
    let mut form = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("client_id", client_id),
        ("redirect_uri", redirect_uri),
        ("code_verifier", verifier),
    ];
    if !client_secret.trim().is_empty() {
        form.push(("client_secret", client_secret));
    }
    let resp = client.post(TOKEN_ENDPOINT).form(&form).send().await?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(AppError::Auth(format!("token exchange failed: {body}")));
    }
    let tr: TokenResponse = resp.json().await?;
    Ok(OAuthTokens {
        access_token: tr.access_token,
        refresh_token: tr.refresh_token.unwrap_or_default(),
        expires_at: now() + tr.expires_in.unwrap_or(3600),
    })
}

/// Refresh an expired access token. Returns updated tokens (the refresh token is
/// preserved since Google does not re-issue it).
pub async fn refresh_tokens(
    client_id: &str,
    client_secret: &str,
    tokens: &OAuthTokens,
) -> Result<OAuthTokens> {
    let client = reqwest::Client::new();
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", tokens.refresh_token.as_str()),
        ("client_id", client_id),
    ];
    if !client_secret.trim().is_empty() {
        form.push(("client_secret", client_secret));
    }
    let resp = client.post(TOKEN_ENDPOINT).form(&form).send().await?;
    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(AppError::Auth(format!("token refresh failed: {body}")));
    }
    let tr: TokenResponse = resp.json().await?;
    Ok(OAuthTokens {
        access_token: tr.access_token,
        refresh_token: if tr.refresh_token.as_deref().unwrap_or("").is_empty() {
            tokens.refresh_token.clone()
        } else {
            tr.refresh_token.unwrap()
        },
        expires_at: now() + tr.expires_in.unwrap_or(3600),
    })
}

/// Ensure the access token is valid, refreshing if it expires within 60s.
pub async fn valid_access_token(
    reference: &str,
    client_id: &str,
    client_secret: &str,
) -> Result<String> {
    let tokens = get_tokens(reference)?;
    if tokens.expires_at - 60 > now() {
        return Ok(tokens.access_token);
    }
    let refreshed = refresh_tokens(client_id, client_secret, &tokens).await?;
    store_tokens(reference, &refreshed)?;
    Ok(refreshed.access_token)
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

// ---- minimal url encode/decode (avoids pulling another dep) ----

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

fn urldecode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(v);
                    i += 3;
                    continue;
                }
                out.push(bytes[i]);
                i += 1;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
