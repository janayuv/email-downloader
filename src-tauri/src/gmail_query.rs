//! Translate the app's structured [`Filter`] into a Gmail `q` search string.
//!
//! This is intentionally separate from `search.rs`: the local FTS5 index and
//! Gmail's `q` language do not map 1:1 (Gmail has `in:`, `has:attachment`,
//! `filename:`, date granularity in days, implicit OR/AND precedence). Keeping
//! the provider quirks here stops them leaking into local search.

use crate::model::Filter;
use chrono::{TimeZone, Utc};

fn quote(term: &str) -> String {
    if term.contains(' ') {
        format!("\"{}\"", term.replace('"', ""))
    } else {
        term.to_string()
    }
}

/// Gmail's `after:`/`before:` take whole days; we pass unix seconds which Gmail
/// also accepts, but normalize to YYYY/MM/DD for readability and stability.
fn ymd(unix: i64) -> String {
    Utc.timestamp_opt(unix, 0)
        .single()
        .map(|d| d.format("%Y/%m/%d").to_string())
        .unwrap_or_default()
}

pub fn build_query(filter: &Filter) -> String {
    let mut parts: Vec<String> = Vec::new();

    if let Some(v) = filter.from.as_deref().filter(|s| !s.trim().is_empty()) {
        parts.push(format!("from:{}", quote(v.trim())));
    }
    if let Some(v) = filter.to.as_deref().filter(|s| !s.trim().is_empty()) {
        parts.push(format!("to:{}", quote(v.trim())));
    }
    if let Some(v) = filter.cc.as_deref().filter(|s| !s.trim().is_empty()) {
        parts.push(format!("cc:{}", quote(v.trim())));
    }
    if let Some(v) = filter.subject.as_deref().filter(|s| !s.trim().is_empty()) {
        parts.push(format!("subject:{}", quote(v.trim())));
    }
    if let Some(v) = filter.text.as_deref().filter(|s| !s.trim().is_empty()) {
        parts.push(quote(v.trim()));
    }
    if let Some(v) = filter.mailbox.as_deref().filter(|s| !s.trim().is_empty()) {
        parts.push(format!("in:{}", v.trim()));
    }
    if let Some(since) = filter.since {
        parts.push(format!("after:{}", ymd(since)));
    }
    if let Some(before) = filter.before {
        parts.push(format!("before:{}", ymd(before)));
    }
    if filter.has_attachment == Some(true) || !filter.extensions.is_empty() {
        parts.push("has:attachment".to_string());
    }
    if !filter.extensions.is_empty() {
        let ors = filter
            .extensions
            .iter()
            .map(|e| format!("filename:{}", e.to_lowercase()))
            .collect::<Vec<_>>()
            .join(" OR ");
        parts.push(format!("({ors})"));
    }

    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_fields() {
        let f = Filter {
            from: Some("alice@example.com".into()),
            subject: Some("quarterly report".into()),
            has_attachment: Some(true),
            extensions: vec!["pdf".into(), "xlsx".into()],
            ..Default::default()
        };
        let q = build_query(&f);
        assert!(q.contains("from:alice@example.com"));
        assert!(q.contains("subject:\"quarterly report\""));
        assert!(q.contains("has:attachment"));
        assert!(q.contains("filename:pdf OR filename:xlsx"));
    }
}
