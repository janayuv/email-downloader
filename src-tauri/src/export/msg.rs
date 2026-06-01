//! MSG exporter — writes each message as an Outlook `.msg` (OLE/CFB compound
//! file) via the `cfb` crate. We populate the core MAPI property streams
//! (subject, body, sender, recipients, transport headers) plus the top-level
//! `__properties_version1.0` stream. This is a pragmatic subset of full MAPI
//! fidelity (attachments-as-storages is a later refinement) but produces files
//! Outlook can open. `verify()` re-opens the compound file and asserts the
//! property stream is present.

use super::{stem, utf16le, Exporter};
use crate::error::{AppError, Result};
use crate::model::ExportFormat;
use crate::parser::ParsedMessage;
use std::io::Write;
use std::path::PathBuf;

const PT_UNICODE: u16 = 0x001F;
const PROPS_STREAM: &str = "__properties_version1.0";

pub struct MsgExporter {
    dir: PathBuf,
    written: Vec<PathBuf>,
}

impl MsgExporter {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            written: Vec::new(),
        }
    }
}

/// Extract the raw header block (everything up to the first blank line).
fn raw_headers(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    if let Some(idx) = text.find("\r\n\r\n").or_else(|| text.find("\n\n")) {
        text[..idx].to_string()
    } else {
        text.into_owned()
    }
}

fn substg_name(prop_id: u16, prop_type: u16) -> String {
    format!("__substg1.0_{prop_id:04X}{prop_type:04X}")
}

/// A 16-byte property entry for a variable-length (unicode) property.
fn unicode_entry(prop_id: u16, byte_len: usize) -> [u8; 16] {
    let mut e = [0u8; 16];
    let tag = ((prop_id as u32) << 16) | PT_UNICODE as u32;
    e[0..4].copy_from_slice(&tag.to_le_bytes());
    e[4..8].copy_from_slice(&0x0000_0006u32.to_le_bytes()); // readable | writable
    e[8..12].copy_from_slice(&(byte_len as u32).to_le_bytes());
    e // last 4 bytes reserved (0)
}

impl Exporter for MsgExporter {
    fn format(&self) -> ExportFormat {
        ExportFormat::Msg
    }

    fn add(&mut self, raw: &[u8], parsed: &ParsedMessage, sha: &str) -> Result<()> {
        let path = self.dir.join(format!("{}.msg", stem(sha)));

        // (property_id, value) for the unicode string properties we emit.
        let props: Vec<(u16, String)> = vec![
            (0x0037, parsed.subject.clone()),               // PR_SUBJECT
            (0x1000, parsed.body_preview.clone()),          // PR_BODY
            (0x0C1A, parsed.from_addr.clone()),             // PR_SENDER_NAME
            (0x0C1F, parsed.from_addr.clone()),             // PR_SENDER_EMAIL_ADDRESS
            (0x0E04, parsed.to_addr.clone()),               // PR_DISPLAY_TO
            (0x0E03, parsed.cc_addr.clone()),               // PR_DISPLAY_CC
            (0x007D, raw_headers(raw)),                     // PR_TRANSPORT_MESSAGE_HEADERS
        ];

        let mut comp =
            cfb::create(&path).map_err(|e| AppError::Export(format!("cfb create: {e}")))?;

        // Top-level property stream header: 8 reserved + 8 id/count dwords.
        let mut props_stream = Vec::new();
        props_stream.extend_from_slice(&[0u8; 32]);

        for (id, value) in &props {
            let data = utf16le(value);
            props_stream.extend_from_slice(&unicode_entry(*id, data.len()));

            let name = substg_name(*id, PT_UNICODE);
            let mut s = comp
                .create_stream(&name)
                .map_err(|e| AppError::Export(format!("cfb stream {name}: {e}")))?;
            s.write_all(&data)
                .map_err(|e| AppError::Export(format!("cfb write {name}: {e}")))?;
            s.flush().ok();
        }

        {
            let mut ps = comp
                .create_stream(PROPS_STREAM)
                .map_err(|e| AppError::Export(format!("cfb props: {e}")))?;
            ps.write_all(&props_stream)
                .map_err(|e| AppError::Export(format!("cfb props write: {e}")))?;
            ps.flush().ok();
        }

        comp.flush()
            .map_err(|e| AppError::Export(format!("cfb flush: {e}")))?;
        self.written.push(path);
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        Ok(())
    }

    fn verify(&self) -> Result<()> {
        for path in self.written.iter().take(5) {
            let comp =
                cfb::open(path).map_err(|e| AppError::Export(format!("MSG verify open: {e}")))?;
            if !comp.exists(PROPS_STREAM) {
                return Err(AppError::Export(format!(
                    "MSG verify failed: {} missing property stream",
                    path.display()
                )));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;
    use std::io::Read;

    const FIXTURE_EML: &[u8] =
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.eml"));

    fn decode_utf16le(b: &[u8]) -> String {
        let u16s: Vec<u16> = b
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&u16s)
            .trim_end_matches('\0')
            .to_string()
    }

    // write() + verify() must succeed for valid input.
    #[test]
    fn write_and_verify_passes() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ex = MsgExporter::new(tmp.path().to_path_buf());
        let parsed = parser::parse(FIXTURE_EML).unwrap();
        ex.add(FIXTURE_EML, &parsed, "aabbccdd00000001").unwrap();
        ex.finish().unwrap();
        ex.verify().expect("verify() must pass on a freshly written MSG");
    }

    // The __properties_version1.0 stream must be present in the CFB.
    #[test]
    fn properties_stream_present() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ex = MsgExporter::new(tmp.path().to_path_buf());
        let parsed = parser::parse(FIXTURE_EML).unwrap();
        ex.add(FIXTURE_EML, &parsed, "aabbccdd00000002").unwrap();
        let path = tmp.path().join("aabbccdd00000002.msg");
        let comp = cfb::open(&path).unwrap();
        assert!(comp.exists(PROPS_STREAM), "__properties_version1.0 must exist");
    }

    // The subject written as PR_SUBJECT must round-trip through UTF-16LE correctly.
    #[test]
    fn subject_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ex = MsgExporter::new(tmp.path().to_path_buf());
        let parsed = parser::parse(FIXTURE_EML).unwrap();
        assert_eq!(parsed.subject, "Golden EML fixture");
        ex.add(FIXTURE_EML, &parsed, "aabbccdd00000003").unwrap();

        let path = tmp.path().join("aabbccdd00000003.msg");
        let mut comp = cfb::open(&path).unwrap();
        // PR_SUBJECT = 0x0037, PT_UNICODE = 0x001F  →  __substg1.0_0037001F
        let stream_name = "__substg1.0_0037001F";
        assert!(comp.exists(stream_name), "subject stream must exist");
        let mut stream = comp.open_stream(stream_name).unwrap();
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        assert_eq!(decode_utf16le(&bytes), "Golden EML fixture");
    }
}
