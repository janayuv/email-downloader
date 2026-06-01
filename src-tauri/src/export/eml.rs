//! EML exporter — writes the raw RFC822 of each message as `<stem>.eml`.
//! Native and lossless. `verify()` re-parses a sample to confirm validity.

use super::{stem, Exporter};
use crate::error::{AppError, Result};
use crate::model::ExportFormat;
use crate::parser::ParsedMessage;
use std::path::PathBuf;

pub struct EmlExporter {
    dir: PathBuf,
    written: Vec<PathBuf>,
}

impl EmlExporter {
    pub fn new(dir: PathBuf) -> Self {
        Self {
            dir,
            written: Vec::new(),
        }
    }
}

impl Exporter for EmlExporter {
    fn format(&self) -> ExportFormat {
        ExportFormat::Eml
    }

    fn add(&mut self, raw: &[u8], _parsed: &ParsedMessage, sha: &str) -> Result<()> {
        let path = self.dir.join(format!("{}.eml", stem(sha)));
        std::fs::write(&path, raw)?;
        self.written.push(path);
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        Ok(())
    }

    fn verify(&self) -> Result<()> {
        // Re-parse up to the first few files to confirm they are valid RFC822.
        for path in self.written.iter().take(5) {
            let bytes = std::fs::read(path)?;
            if mail_parser::MessageParser::default().parse(&bytes).is_none() {
                return Err(AppError::Export(format!(
                    "EML verify failed: {} did not re-parse",
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

    const FIXTURE_EML: &[u8] =
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.eml"));
    const FIXTURE_XLSX_EML: &[u8] =
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample_with_xlsx.eml"));
    // Expected decoded bytes for the attachments embedded in the fixture files.
    const GOLDEN_PDF_CONTENT: &[u8] = b"golden-pdf-content";
    const GOLDEN_XLSX_CONTENT: &[u8] = b"golden-xlsx-content";

    fn dummy_parsed() -> ParsedMessage {
        parser::parse(FIXTURE_EML).expect("fixture must parse")
    }

    // Golden fixture: verify it parses, has expected subject and one attachment.
    #[test]
    fn fixture_parses_correctly() {
        let p = parser::parse(FIXTURE_EML).expect("sample.eml must parse");
        assert_eq!(p.subject, "Golden EML fixture");
        assert_eq!(p.from_addr, "alice@fixtures.test");
        assert_eq!(p.attachments.len(), 1);
        assert_eq!(p.attachments[0].filename, "report.pdf");
        assert_eq!(p.attachments[0].ext, "pdf");
        assert_eq!(p.attachments[0].data, GOLDEN_PDF_CONTENT);
    }

    // Golden fixture: XLSX variant has the xlsx attachment.
    #[test]
    fn xlsx_fixture_parses_correctly() {
        let p = parser::parse(FIXTURE_XLSX_EML).expect("sample_with_xlsx.eml must parse");
        assert_eq!(p.attachments.len(), 1);
        assert_eq!(p.attachments[0].filename, "data.xlsx");
        assert_eq!(p.attachments[0].ext, "xlsx");
        assert_eq!(p.attachments[0].data, GOLDEN_XLSX_CONTENT);
    }

    // Write the golden fixture bytes through EmlExporter, then verify() passes
    // and the re-parsed file preserves the subject.
    #[test]
    fn write_and_verify_passes() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ex = EmlExporter::new(tmp.path().to_path_buf());
        let parsed = dummy_parsed();
        ex.add(FIXTURE_EML, &parsed, "aabbccdd00000001").unwrap();
        ex.finish().unwrap();
        ex.verify().expect("verify() must pass on valid EML");
        // Round-trip: re-parse the written file and check subject survived.
        let out = tmp.path().join("aabbccdd00000001.eml");
        let reparsed = parser::parse(&std::fs::read(out).unwrap()).unwrap();
        assert_eq!(reparsed.subject, "Golden EML fixture");
    }

    // verify() must fail when the output file has been removed after writing
    // (simulates a silent write failure or external deletion).
    #[test]
    fn verify_fails_on_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let mut ex = EmlExporter::new(tmp.path().to_path_buf());
        ex.add(FIXTURE_EML, &dummy_parsed(), "aabbccdd00000002").unwrap();
        // Remove the file to simulate missing output.
        std::fs::remove_file(tmp.path().join("aabbccdd00000002.eml")).unwrap();
        assert!(ex.verify().is_err(), "verify() must fail when output file is missing");
    }
}
