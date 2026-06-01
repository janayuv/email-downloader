//! Export pipeline. Each format implements [`Exporter`] with a streaming
//! lifecycle (`add` per message, `finish`, then `verify`). `verify()` is NOT
//! optional — a write returning `Ok` never counts as success on its own; we
//! re-open / re-parse the output and (for MSG/PST) check structural validity.

pub mod eml;
pub mod mbox;
pub mod msg;
pub mod pst;

use crate::error::Result;
use crate::model::ExportFormat;
use crate::parser::ParsedMessage;
use std::path::{Path, PathBuf};

/// Streaming exporter. Implementations own their output files/handles.
pub trait Exporter: Send {
    fn format(&self) -> ExportFormat;

    /// Add one message: raw RFC822 bytes, the parsed view, and its sha256.
    fn add(&mut self, raw: &[u8], parsed: &ParsedMessage, sha: &str) -> Result<()>;

    /// Flush and close. Called once after the last `add`.
    fn finish(&mut self) -> Result<()>;

    /// Re-open / re-parse the produced output and assert it is valid.
    fn verify(&self) -> Result<()>;
}

/// Build one exporter per requested format, each rooted under `dest/<format>/`.
pub fn create_exporters(formats: &[ExportFormat], dest: &Path) -> Result<Vec<Box<dyn Exporter>>> {
    let mut out: Vec<Box<dyn Exporter>> = Vec::new();
    for f in formats {
        let dir = dest.join(f.as_str());
        std::fs::create_dir_all(&dir)?;
        let exporter: Box<dyn Exporter> = match f {
            ExportFormat::Eml => Box::new(eml::EmlExporter::new(dir)),
            ExportFormat::Mbox => Box::new(mbox::MboxExporter::new(dir)?),
            ExportFormat::Msg => Box::new(msg::MsgExporter::new(dir)),
            ExportFormat::Pst => Box::new(pst::PstExporter::new(dir)),
        };
        out.push(exporter);
    }
    Ok(out)
}

/// Shared helper: a safe-ish filename stem from a sha (always valid on disk).
pub fn stem(sha: &str) -> String {
    if sha.len() >= 16 {
        sha[..16].to_string()
    } else {
        sha.to_string()
    }
}

/// UTF-16LE little-endian bytes — used by the MSG writer.
pub fn utf16le(s: &str) -> Vec<u8> {
    let mut v = Vec::with_capacity(s.len() * 2 + 2);
    for u in s.encode_utf16() {
        v.extend_from_slice(&u.to_le_bytes());
    }
    v.extend_from_slice(&[0, 0]); // null terminator
    v
}

/// Track output roots so a job can report what was produced.
#[allow(dead_code)] // surfaced in the reporting UI in a later milestone
#[derive(Debug, Default, Clone)]
pub struct ExportOutputs {
    pub dirs: Vec<PathBuf>,
}
