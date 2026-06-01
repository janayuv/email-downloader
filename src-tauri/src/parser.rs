//! Thin wrapper over `mail-parser` that extracts exactly the fields the index,
//! exporters and attachment pipeline need from a raw RFC822 message.

use crate::error::{AppError, Result};
use mail_parser::{Address, MessageParser, MimeHeaders};

#[derive(Debug, Clone)]
pub struct ParsedAttachment {
    pub filename: String,
    pub ext: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct ParsedMessage {
    pub message_id: String,
    pub from_addr: String,
    pub to_addr: String,
    pub cc_addr: String,
    pub subject: String,
    /// Unix seconds; 0 when the message has no Date header.
    pub date: i64,
    pub body_preview: String,
    pub attachments: Vec<ParsedAttachment>,
}

/// Flatten an address header (lists and groups) into a comma-separated string
/// of email addresses.
fn join_addresses(addr: Option<&Address>) -> String {
    let mut out: Vec<String> = Vec::new();
    if let Some(a) = addr {
        for ad in a.iter() {
            if let Some(email) = ad.address.as_deref() {
                out.push(email.to_string());
            }
        }
    }
    out.join(", ")
}

fn ext_of(filename: &str) -> String {
    filename
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .unwrap_or_default()
}

pub fn parse(raw: &[u8]) -> Result<ParsedMessage> {
    let msg = MessageParser::default()
        .parse(raw)
        .ok_or_else(|| AppError::Parse("no headers found".into()))?;

    let body_preview = msg
        .body_text(0)
        .map(|c| {
            let s: String = c.chars().take(2000).collect();
            s
        })
        .unwrap_or_default();

    let mut attachments = Vec::new();
    for part in msg.attachments() {
        // Skip nested rfc822 messages — only real file attachments.
        if part.message().is_some() {
            continue;
        }
        let filename = part.attachment_name().unwrap_or("attachment").to_string();
        let ext = ext_of(&filename);
        attachments.push(ParsedAttachment {
            ext,
            filename,
            data: part.contents().to_vec(),
        });
    }

    Ok(ParsedMessage {
        message_id: msg.message_id().unwrap_or_default().to_string(),
        from_addr: join_addresses(msg.from()),
        to_addr: join_addresses(msg.to()),
        cc_addr: join_addresses(msg.cc()),
        subject: msg.subject().unwrap_or_default().to_string(),
        date: msg.date().map(|d| d.to_timestamp()).unwrap_or(0),
        body_preview,
        attachments,
    })
}
