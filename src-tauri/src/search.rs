//! Local search over the FTS5 index. This module owns ONLY the local index
//! query logic. Provider-side query translation (Gmail `q`) lives separately in
//! `gmail_query.rs` because the two query languages do not map 1:1.

use crate::error::Result;
use crate::model::Filter;
use crate::storage::Db;
use rusqlite::types::ToSql;

/// A single search result row (a subset of the message record plus rank).
#[derive(Debug, Clone, serde::Serialize)]
pub struct MessageHit {
    pub id: String,
    pub account_id: String,
    pub subject: String,
    pub from_addr: String,
    pub to_addr: String,
    pub internal_date: i64,
    pub has_attachments: bool,
    pub size: i64,
}

/// Escape a user token for an FTS5 MATCH expression by wrapping it in double
/// quotes (FTS5 treats a quoted string as a literal phrase). Internal double
/// quotes are doubled per FTS5 rules.
fn fts_phrase(token: &str) -> String {
    format!("\"{}\"", token.replace('"', "\"\""))
}

/// Build an FTS5 MATCH string from the structured filter, scoping field-specific
/// terms with column filters (`subject:`, `from_addr:` ...). Returns `None` when
/// there is nothing to match on (the caller then runs a plain metadata query).
fn build_match(filter: &Filter) -> Option<String> {
    let mut clauses: Vec<String> = Vec::new();
    if let Some(t) = filter.text.as_deref().filter(|s| !s.trim().is_empty()) {
        clauses.push(fts_phrase(t.trim()));
    }
    if let Some(s) = filter.subject.as_deref().filter(|s| !s.trim().is_empty()) {
        clauses.push(format!("subject:{}", fts_phrase(s.trim())));
    }
    if let Some(f) = filter.from.as_deref().filter(|s| !s.trim().is_empty()) {
        clauses.push(format!("from_addr:{}", fts_phrase(f.trim())));
    }
    if let Some(t) = filter.to.as_deref().filter(|s| !s.trim().is_empty()) {
        clauses.push(format!("to_addr:{}", fts_phrase(t.trim())));
    }
    if clauses.is_empty() {
        None
    } else {
        Some(clauses.join(" AND "))
    }
}

/// Run a search. When the filter has free-text/subject/from/to terms it uses the
/// FTS5 index joined back to `messages`; otherwise it filters `messages` on
/// metadata (date range, has-attachment, extension) directly.
pub fn search(db: &Db, account_id: Option<&str>, filter: &Filter, limit: i64) -> Result<Vec<MessageHit>> {
    let conn = db.conn();

    let mut sql = String::from(
        "SELECT m.id, m.account_id, m.subject, m.from_addr, m.to_addr, m.internal_date,
                m.has_attachments, m.size
         FROM messages m ",
    );
    let mut wheres: Vec<String> = Vec::new();
    let mut binds: Vec<Box<dyn ToSql>> = Vec::new();

    if let Some(m) = build_match(filter) {
        sql.push_str("JOIN messages_fts f ON f.message_id = m.id ");
        wheres.push("messages_fts MATCH ?".into());
        binds.push(Box::new(m));
    }

    if let Some(acc) = account_id {
        wheres.push("m.account_id = ?".into());
        binds.push(Box::new(acc.to_string()));
    }
    if let Some(since) = filter.since {
        wheres.push("m.internal_date >= ?".into());
        binds.push(Box::new(since));
    }
    if let Some(before) = filter.before {
        wheres.push("m.internal_date < ?".into());
        binds.push(Box::new(before));
    }
    if let Some(true) = filter.has_attachment {
        wheres.push("m.has_attachments = 1".into());
    }
    if !filter.extensions.is_empty() {
        let placeholders = filter
            .extensions
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");
        wheres.push(format!(
            "m.id IN (SELECT message_id FROM attachments WHERE lower(ext) IN ({placeholders}))"
        ));
        for e in &filter.extensions {
            binds.push(Box::new(e.to_lowercase()));
        }
    }

    if !wheres.is_empty() {
        sql.push_str("WHERE ");
        sql.push_str(&wheres.join(" AND "));
        sql.push(' ');
    }
    sql.push_str("ORDER BY m.internal_date DESC LIMIT ?");
    binds.push(Box::new(limit));

    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn ToSql> = binds.iter().map(|b| b.as_ref()).collect();
    let rows = stmt
        .query_map(rusqlite::params_from_iter(param_refs), |r| {
            Ok(MessageHit {
                id: r.get_unwrap(0),
                account_id: r.get_unwrap(1),
                subject: r.get_unwrap(2),
                from_addr: r.get_unwrap(3),
                to_addr: r.get_unwrap(4),
                internal_date: r.get_unwrap(5),
                has_attachments: r.get_unwrap::<_, i64>(6) != 0,
                size: r.get_unwrap(7),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}
