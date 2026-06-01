//! MBOX exporter (mboxrd variant). All messages are concatenated into a single
//! `backup.mbox` separated by `From ` lines, with `>From ` quoting applied to
//! body lines that begin with `From `. `verify()` re-opens the file and checks
//! at least one separator is present and the file parses back.

use super::Exporter;
use crate::error::{AppError, Result};
use crate::model::ExportFormat;
use crate::parser::ParsedMessage;
use chrono::{TimeZone, Utc};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::PathBuf;

pub struct MboxExporter {
    path: PathBuf,
    writer: BufWriter<File>,
    count: u64,
}

impl MboxExporter {
    pub fn new(dir: PathBuf) -> Result<Self> {
        let path = dir.join("backup.mbox");
        let writer = BufWriter::new(File::create(&path)?);
        Ok(Self {
            path,
            writer,
            count: 0,
        })
    }
}

/// Quote any line beginning with `From ` (and `>From `, `>>From ` ...) by
/// prefixing an additional `>` — the mboxrd reversible convention.
fn escape_body(raw: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(raw);
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.trim_start_matches('>').starts_with("From ") {
            out.push('>');
        }
        out.push_str(line);
    }
    out.into_bytes()
}

impl Exporter for MboxExporter {
    fn format(&self) -> ExportFormat {
        ExportFormat::Mbox
    }

    fn add(&mut self, raw: &[u8], parsed: &ParsedMessage, _sha: &str) -> Result<()> {
        let sender = parsed
            .from_addr
            .split(',')
            .next()
            .unwrap_or("MAILER-DAEMON")
            .trim();
        let sender = if sender.is_empty() {
            "MAILER-DAEMON"
        } else {
            sender
        };
        let date = Utc
            .timestamp_opt(if parsed.date > 0 { parsed.date } else { 0 }, 0)
            .single()
            .map(|d| d.format("%a %b %e %H:%M:%S %Y").to_string())
            .unwrap_or_else(|| "Thu Jan  1 00:00:00 1970".to_string());

        writeln!(self.writer, "From {sender} {date}")?;
        self.writer.write_all(&escape_body(raw))?;
        // Ensure a trailing blank line between messages.
        if !raw.ends_with(b"\n") {
            self.writer.write_all(b"\n")?;
        }
        self.writer.write_all(b"\n")?;
        self.count += 1;
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        self.writer.flush()?;
        Ok(())
    }

    fn verify(&self) -> Result<()> {
        let bytes = std::fs::read(&self.path)?;
        if self.count > 0 && !bytes.windows(5).any(|w| w == b"From ") {
            return Err(AppError::Export(
                "MBOX verify failed: no From separator found".into(),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    const FIXTURE_EML: &[u8] =
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.eml"));

    fn dummy_parsed() -> ParsedMessage {
        parser::parse(FIXTURE_EML).unwrap()
    }

    // Writing two messages produces a file with exactly two "From " separators.
    #[test]
    fn two_messages_produce_two_separators() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ex = MboxExporter::new(tmp.path().to_path_buf()).unwrap();
        let p = dummy_parsed();
        ex.add(FIXTURE_EML, &p, "a").unwrap();
        ex.add(FIXTURE_EML, &p, "b").unwrap();
        ex.finish().unwrap();
        let mbox = std::fs::read(tmp.path().join("backup.mbox")).unwrap();
        let count = mbox.windows(5).filter(|w| *w == b"From ").count();
        assert_eq!(count, 2, "expected 2 From_ separators");
    }

    // verify() passes on a valid MBOX with at least one message.
    #[test]
    fn write_and_verify_passes() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ex = MboxExporter::new(tmp.path().to_path_buf()).unwrap();
        ex.add(FIXTURE_EML, &dummy_parsed(), "a").unwrap();
        ex.finish().unwrap();
        ex.verify().expect("verify() must pass");
    }

    // A body line starting with "From " must be written as ">From " (mboxrd quoting).
    #[test]
    fn from_line_in_body_gets_quoted() {
        let raw = b"From: sender@test.com\r\nSubject: t\r\n\r\nFrom this must be quoted\r\n";
        let escaped = escape_body(raw);
        let text = String::from_utf8_lossy(&escaped);
        assert!(
            text.contains("\n>From this must be quoted"),
            "body 'From ' must be escaped to '>From '"
        );
        // The From: header must NOT be quoted (it has a colon, not a space).
        assert!(text.contains("From: sender@test.com"));
    }

    // A line starting with ">From " must become ">>From " (idempotent quoting).
    #[test]
    fn existing_gt_from_gets_double_quoted() {
        let raw = b"Subject: t\r\n\r\n>From already quoted\r\n";
        let escaped = escape_body(raw);
        let text = String::from_utf8_lossy(&escaped);
        assert!(text.contains(">>From already quoted"));
    }
}
