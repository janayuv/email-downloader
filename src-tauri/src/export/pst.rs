//! PST exporter. Microsoft PST has no mature open-source *writer*, so real PST
//! output is produced by a bundled sidecar (`pst-export`, a .NET tool using
//! Aspose.Email per the project decision). This module stages each message as
//! an `.eml` in a temp folder, then drives the sidecar on `finish()`.
//!
//! IMPORTANT: when the sidecar is absent we surface a clear, non-silent error
//! rather than pretending the export succeeded — the other three formats still
//! complete independently.

use super::{stem, Exporter};
use crate::error::{AppError, Result};
use crate::model::ExportFormat;
use crate::parser::ParsedMessage;
use std::path::PathBuf;
use std::process::Command;

/// Locate the PST sidecar binary: `ED_PST_SIDECAR` override, then next to the
/// executable. Shared by the PST exporter and the attachment extractor.
pub fn locate_sidecar() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("ED_PST_SIDECAR") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    let exe_dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    for name in ["pst-export.exe", "pst-export"] {
        let cand = exe_dir.join(name);
        if cand.exists() {
            return Some(cand);
        }
    }
    None
}

pub struct PstExporter {
    dir: PathBuf,
    eml_dir: PathBuf,
    pst_path: PathBuf,
    staged: u64,
    produced: bool,
}

impl PstExporter {
    pub fn new(dir: PathBuf) -> Self {
        let eml_dir = dir.join("_staged_eml");
        let pst_path = dir.join("backup.pst");
        Self {
            dir,
            eml_dir,
            pst_path,
            staged: 0,
            produced: false,
        }
    }
}

impl Exporter for PstExporter {
    fn format(&self) -> ExportFormat {
        ExportFormat::Pst
    }

    fn add(&mut self, raw: &[u8], _parsed: &ParsedMessage, sha: &str) -> Result<()> {
        std::fs::create_dir_all(&self.eml_dir)?;
        let path = self.eml_dir.join(format!("{}.eml", stem(sha)));
        std::fs::write(&path, raw)?;
        self.staged += 1;
        Ok(())
    }

    fn finish(&mut self) -> Result<()> {
        if self.staged == 0 {
            return Ok(());
        }
        let sidecar = locate_sidecar().ok_or_else(|| {
            AppError::Export(format!(
                "PST sidecar not configured. Staged {} message(s) as EML at {} — \
                 build/point ED_PST_SIDECAR at the Aspose.Email pst-export tool to produce {}.",
                self.staged,
                self.eml_dir.display(),
                self.pst_path.display()
            ))
        })?;

        let output = Command::new(&sidecar)
            .arg("export")
            .arg("--input")
            .arg(&self.eml_dir)
            .arg("--output")
            .arg(&self.pst_path)
            .output()
            .map_err(|e| AppError::Export(format!("pst sidecar spawn: {e}")))?;

        if !output.status.success() {
            return Err(AppError::Export(format!(
                "pst sidecar failed: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        self.produced = true;
        Ok(())
    }

    fn verify(&self) -> Result<()> {
        if self.staged == 0 {
            return Ok(());
        }
        if !self.produced || !self.pst_path.exists() {
            return Err(AppError::Export(format!(
                "PST verify failed: {} was not produced",
                self.pst_path.display()
            )));
        }
        // Optional deeper check: ask the sidecar to validate the file.
        if let Some(sidecar) = locate_sidecar() {
            let status = Command::new(&sidecar)
                .arg("verify")
                .arg("--file")
                .arg(&self.pst_path)
                .status();
            if let Ok(s) = status {
                if !s.success() {
                    return Err(AppError::Export("PST sidecar verify reported invalid".into()));
                }
            }
        }
        let _ = &self.dir;
        Ok(())
    }
}
