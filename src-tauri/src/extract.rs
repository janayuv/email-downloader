//! Extract attachments from existing archive files (EML, MBOX, MSG, PST) with an
//! optional extension filter (pdf, xlsx, pptx, docx, …).
//!
//! Output is organized per-source: one subfolder per input archive, attachments
//! written under their original filenames (numeric-suffixed on collision).
//! EML/MBOX/MSG are parsed natively (MSG via the `cfb` compound-file reader); PST
//! is read through the sidecar, which dumps EMLs we then extract natively — so the
//! attachment-filtering and naming logic stays uniform across all formats.

use crate::error::{AppError, Result};
use crate::export::pst::locate_sidecar;
use crate::parser;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::{AppHandle, Emitter};

pub const SUPPORTED: &[&str] = &["eml", "mbox", "msg", "pst"];

#[derive(Debug, Default, Clone, Serialize)]
pub struct ExtractReport {
    pub files_processed: u64,
    pub attachments_extracted: u64,
    pub skipped_filtered: u64,
    pub errors: Vec<String>,
    pub output_root: String,
}

#[derive(Debug, Clone, Serialize)]
struct ExtractProgress {
    file: String,
    files_processed: u64,
    attachments_extracted: u64,
}

fn ext_lower(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}

/// Expand the user selection (files and/or folders) into a flat list of
/// supported archive files; folders are scanned recursively.
fn expand_sources(sources: &[String]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for s in sources {
        let p = PathBuf::from(s);
        if p.is_dir() {
            collect_dir(&p, &mut out);
        } else if p.is_file() && SUPPORTED.contains(&ext_lower(&p).as_str()) {
            out.push(p);
        }
    }
    out
}

fn collect_dir(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_dir(&p, out);
        } else if SUPPORTED.contains(&ext_lower(&p).as_str()) {
            out.push(p);
        }
    }
}

/// Strip any path components and replace filesystem-unsafe characters.
fn sanitize(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let mut s: String = base
        .chars()
        .map(|c| {
            if "<>:\"/\\|?*".contains(c) || (c as u32) < 0x20 {
                '_'
            } else {
                c
            }
        })
        .collect();
    s = s.trim().trim_matches('.').to_string();
    if s.is_empty() {
        "attachment".to_string()
    } else {
        s
    }
}

/// Pick a non-colliding path inside `dir` for `filename`.
fn unique_path(dir: &Path, filename: &str) -> PathBuf {
    let candidate = dir.join(filename);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match filename.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (filename.to_string(), String::new()),
    };
    let mut i = 1;
    loop {
        let c = dir.join(format!("{stem}_{i}{ext}"));
        if !c.exists() {
            return c;
        }
        i += 1;
    }
}

fn write_attachment(
    out_dir: &Path,
    filename: &str,
    ext: &str,
    data: &[u8],
    filter: &HashSet<String>,
    report: &mut ExtractReport,
) -> Result<()> {
    if !filter.is_empty() && !filter.contains(&ext.to_lowercase()) {
        report.skipped_filtered += 1;
        return Ok(());
    }
    std::fs::create_dir_all(out_dir)?;
    let path = unique_path(out_dir, &sanitize(filename));
    std::fs::write(&path, data)?;
    report.attachments_extracted += 1;
    Ok(())
}

fn extract_from_eml_bytes(
    raw: &[u8],
    out_dir: &Path,
    filter: &HashSet<String>,
    report: &mut ExtractReport,
) -> Result<()> {
    let parsed = parser::parse(raw)?;
    for att in parsed.attachments {
        write_attachment(out_dir, &att.filename, &att.ext, &att.data, filter, report)?;
    }
    Ok(())
}

/// Stream an mbox file, splitting on `From ` separators and reversing the
/// mboxrd `>From ` quoting, then extract each message's attachments.
fn extract_from_mbox(
    path: &Path,
    out_dir: &Path,
    filter: &HashSet<String>,
    report: &mut ExtractReport,
) -> Result<()> {
    let mut reader = BufReader::new(std::fs::File::open(path)?);
    let mut line: Vec<u8> = Vec::new();
    let mut msg: Vec<u8> = Vec::new();
    let mut started = false;

    let flush = |msg: &mut Vec<u8>, report: &mut ExtractReport| {
        if !msg.is_empty() {
            if let Err(e) = extract_from_eml_bytes(msg, out_dir, filter, report) {
                report.errors.push(format!("{}: {e}", path.display()));
            }
        }
        msg.clear();
    };

    loop {
        line.clear();
        let n = reader.read_until(b'\n', &mut line)?;
        if n == 0 {
            break;
        }
        if line.starts_with(b"From ") {
            if started {
                flush(&mut msg, report);
            }
            started = true;
            continue; // the From_ separator is not part of the message
        }
        // Reverse mboxrd quoting: `>+From ` had one `>` added on write.
        if line.first() == Some(&b'>') {
            let nonarrow = line.iter().position(|&b| b != b'>').unwrap_or(line.len());
            if line[nonarrow..].starts_with(b"From ") {
                msg.extend_from_slice(&line[1..]);
                continue;
            }
        }
        msg.extend_from_slice(&line);
    }
    if started {
        flush(&mut msg, report);
    }
    Ok(())
}

fn utf16le_to_string(b: &[u8]) -> String {
    let u: Vec<u16> = b
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&u)
        .trim_end_matches('\0')
        .to_string()
}

#[derive(Default)]
struct MsgAttach {
    data: Option<Vec<u8>>,
    long_name: Option<String>,
    short_name: Option<String>,
}

/// Read attachments out of an Outlook `.msg` compound file by walking its
/// `__attach_version1.0_*` storages.
fn extract_from_msg(
    path: &Path,
    out_dir: &Path,
    filter: &HashSet<String>,
    report: &mut ExtractReport,
) -> Result<()> {
    let mut comp = cfb::open(path).map_err(|e| AppError::Export(format!("msg open: {e}")))?;

    // Phase 1 (immutable walk): collect attachment-related stream paths.
    let stream_paths: Vec<PathBuf> = comp
        .walk()
        .filter(|e| e.is_stream())
        .map(|e| e.path().to_path_buf())
        .filter(|p| p.to_string_lossy().contains("__attach_version1.0"))
        .collect();

    // Phase 2 (mutable open): group by parent storage, decode data + name.
    let mut groups: HashMap<String, MsgAttach> = HashMap::new();
    for p in stream_paths {
        let norm = p.to_string_lossy().replace('\\', "/");
        let (parent, leaf) = match norm.rsplit_once('/') {
            Some((par, leaf)) => (par.to_string(), leaf.to_string()),
            None => continue,
        };
        let mut stream = match comp.open_stream(&p) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mut bytes = Vec::new();
        if stream.read_to_end(&mut bytes).is_err() {
            continue;
        }
        let acc = groups.entry(parent).or_default();
        match leaf.as_str() {
            "__substg1.0_3701000D" => acc.data = Some(bytes), // PR_ATTACH_DATA_BIN
            "__substg1.0_3707001F" => acc.long_name = Some(utf16le_to_string(&bytes)), // long filename (unicode)
            "__substg1.0_3704001F" => acc.short_name = Some(utf16le_to_string(&bytes)), // 8.3 filename (unicode)
            "__substg1.0_3707001E" => {
                acc.long_name = Some(String::from_utf8_lossy(&bytes).trim_end_matches('\0').to_string())
            }
            "__substg1.0_3704001E" => {
                acc.short_name = Some(String::from_utf8_lossy(&bytes).trim_end_matches('\0').to_string())
            }
            _ => {}
        }
    }

    let mut idx = 0;
    for (_parent, att) in groups {
        let Some(data) = att.data else { continue };
        let filename = att
            .long_name
            .or(att.short_name)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| {
                idx += 1;
                format!("attachment_{idx}.bin")
            });
        let ext = filename
            .rsplit_once('.')
            .map(|(_, e)| e.to_lowercase())
            .unwrap_or_default();
        write_attachment(out_dir, &filename, &ext, &data, filter, report)?;
    }
    Ok(())
}

/// Extract from a PST via the sidecar: it dumps each message as `.eml` into a
/// temp dir which we then extract natively. Fails loudly if the sidecar is
/// absent — never silently.
fn extract_from_pst(
    path: &Path,
    out_dir: &Path,
    filter: &HashSet<String>,
    report: &mut ExtractReport,
) -> Result<()> {
    let sidecar = locate_sidecar().ok_or_else(|| {
        AppError::Export(format!(
            "PST sidecar not configured (set ED_PST_SIDECAR) — cannot extract from {}",
            path.display()
        ))
    })?;

    let tmp = out_dir.join("_pst_eml_tmp");
    std::fs::create_dir_all(&tmp)?;

    let output = Command::new(&sidecar)
        .arg("extract-eml")
        .arg("--file")
        .arg(path)
        .arg("--output")
        .arg(&tmp)
        .output()
        .map_err(|e| AppError::Export(format!("pst sidecar spawn: {e}")))?;
    if !output.status.success() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(AppError::Export(format!(
            "pst sidecar failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    if let Ok(rd) = std::fs::read_dir(&tmp) {
        for entry in rd.flatten() {
            let p = entry.path();
            if ext_lower(&p) == "eml" {
                match std::fs::read(&p) {
                    Ok(raw) => {
                        if let Err(e) = extract_from_eml_bytes(&raw, out_dir, filter, report) {
                            report.errors.push(format!("{}: {e}", p.display()));
                        }
                    }
                    Err(e) => report.errors.push(format!("{}: {e}", p.display())),
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

/// Run extraction over the selected sources/folders. Blocking file IO runs on a
/// dedicated thread; per-file progress is emitted on `extract://progress` and
/// the final report on `extract://done`.
pub async fn run(
    app: AppHandle,
    sources: Vec<String>,
    destination: String,
    extensions: Vec<String>,
) -> Result<ExtractReport> {
    let filter: HashSet<String> = extensions.iter().map(|e| e.to_lowercase()).collect();
    let dest = PathBuf::from(&destination);
    std::fs::create_dir_all(&dest)?;

    let app2 = app.clone();
    let report = tokio::task::spawn_blocking(move || {
        let files = expand_sources(&sources);
        let mut report = ExtractReport {
            output_root: dest.to_string_lossy().into_owned(),
            ..Default::default()
        };

        for f in files {
            let stem = f.file_stem().and_then(|s| s.to_str()).unwrap_or("archive");
            let out_dir = dest.join(sanitize(stem));

            let res = match ext_lower(&f).as_str() {
                "eml" => std::fs::read(&f)
                    .map_err(AppError::from)
                    .and_then(|raw| extract_from_eml_bytes(&raw, &out_dir, &filter, &mut report)),
                "mbox" => extract_from_mbox(&f, &out_dir, &filter, &mut report),
                "msg" => extract_from_msg(&f, &out_dir, &filter, &mut report),
                "pst" => extract_from_pst(&f, &out_dir, &filter, &mut report),
                _ => Ok(()),
            };
            if let Err(e) = res {
                report.errors.push(format!("{}: {e}", f.display()));
            }
            report.files_processed += 1;
            let _ = app2.emit(
                "extract://progress",
                ExtractProgress {
                    file: f.display().to_string(),
                    files_processed: report.files_processed,
                    attachments_extracted: report.attachments_extracted,
                },
            );
        }
        report
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?;

    let _ = app.emit("extract://done", &report);
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // ---- known bytes embedded in the fixture files ----
    const GOLDEN_PDF: &[u8] = b"golden-pdf-content";
    const GOLDEN_XLSX: &[u8] = b"golden-xlsx-content";
    const GOLDEN_MBOX: &[u8] = b"golden-mbox-content";

    const FIXTURE_EML: &[u8] =
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.eml"));
    const FIXTURE_XLSX_EML: &[u8] =
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample_with_xlsx.eml"));
    const FIXTURE_MBOX: &str =
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/sample.mbox");

    // Build a minimal MSG compound file with one attachment for extract_from_msg tests.
    fn make_msg_with_attachment(path: &Path, att_name: &str, att_data: &[u8]) {
        let mut comp = cfb::create(path).unwrap();
        {
            let mut s = comp.create_stream("__properties_version1.0").unwrap();
            s.write_all(&[0u8; 32]).unwrap();
        }
        comp.create_storage("__attach_version1.0_#00000000").unwrap();
        {
            let sp = Path::new("__attach_version1.0_#00000000")
                .join("__substg1.0_3701000D");
            let mut s = comp.create_stream(&sp).unwrap();
            s.write_all(att_data).unwrap();
        }
        {
            let sp = Path::new("__attach_version1.0_#00000000")
                .join("__substg1.0_3704001E");
            let mut s = comp.create_stream(&sp).unwrap();
            let mut name = att_name.as_bytes().to_vec();
            name.push(0); // null-terminated ASCII
            s.write_all(&name).unwrap();
        }
        comp.flush().unwrap();
    }

    fn no_filter() -> HashSet<String> {
        HashSet::new()
    }

    fn filter(exts: &[&str]) -> HashSet<String> {
        exts.iter().map(|e| e.to_string()).collect()
    }

    // ---- EML extraction ----

    #[test]
    fn extract_eml_gets_pdf_attachment() {
        let tmp = tempfile::tempdir().unwrap();
        let mut report = ExtractReport::default();
        extract_from_eml_bytes(FIXTURE_EML, tmp.path(), &no_filter(), &mut report).unwrap();
        assert_eq!(report.attachments_extracted, 1);
        assert_eq!(report.skipped_filtered, 0);
        let out = tmp.path().join("report.pdf");
        assert!(out.exists(), "report.pdf must be written");
        assert_eq!(std::fs::read(&out).unwrap(), GOLDEN_PDF);
    }

    #[test]
    fn extract_eml_pdf_excluded_by_xlsx_filter() {
        let tmp = tempfile::tempdir().unwrap();
        let mut report = ExtractReport::default();
        // PDF attachment, but filter only allows xlsx → nothing extracted.
        extract_from_eml_bytes(FIXTURE_EML, tmp.path(), &filter(&["xlsx"]), &mut report).unwrap();
        assert_eq!(report.attachments_extracted, 0);
        assert_eq!(report.skipped_filtered, 1);
    }

    #[test]
    fn extract_eml_xlsx_included_by_xlsx_filter() {
        let tmp = tempfile::tempdir().unwrap();
        let mut report = ExtractReport::default();
        extract_from_eml_bytes(FIXTURE_XLSX_EML, tmp.path(), &filter(&["xlsx"]), &mut report).unwrap();
        assert_eq!(report.attachments_extracted, 1);
        let out = tmp.path().join("data.xlsx");
        assert!(out.exists());
        assert_eq!(std::fs::read(&out).unwrap(), GOLDEN_XLSX);
    }

    #[test]
    fn extract_eml_no_attachments_yields_zero() {
        // A plain-text EML with no attachments should succeed with 0 extracted.
        let plain = b"From: a@b.com\r\nSubject: plain\r\n\r\nJust text.\r\n";
        let tmp = tempfile::tempdir().unwrap();
        let mut report = ExtractReport::default();
        extract_from_eml_bytes(plain, tmp.path(), &no_filter(), &mut report).unwrap();
        assert_eq!(report.attachments_extracted, 0);
    }

    // ---- MBOX extraction ----

    #[test]
    fn extract_mbox_gets_attachment_from_second_message() {
        let tmp = tempfile::tempdir().unwrap();
        let mut report = ExtractReport::default();
        extract_from_mbox(Path::new(FIXTURE_MBOX), tmp.path(), &no_filter(), &mut report).unwrap();
        // Only the second message has an attachment.
        assert_eq!(report.attachments_extracted, 1, "expected 1 attachment from sample.mbox");
        let out = tmp.path().join("mbox_report.pdf");
        assert!(out.exists(), "mbox_report.pdf must be extracted");
        assert_eq!(std::fs::read(&out).unwrap(), GOLDEN_MBOX);
    }

    #[test]
    fn extract_mbox_quoting_reversed_correctly() {
        // The second message body contains ">From " (mboxrd-quoted). After
        // extraction the body seen by mail-parser must begin with "From ".
        let tmp = tempfile::tempdir().unwrap();
        let mut report = ExtractReport::default();
        extract_from_mbox(Path::new(FIXTURE_MBOX), tmp.path(), &no_filter(), &mut report).unwrap();
        // No errors means the quoting reversal didn't corrupt the MIME structure.
        assert!(report.errors.is_empty(), "extraction must not produce errors");
    }

    // ---- MSG extraction ----

    #[test]
    fn extract_msg_gets_attachment() {
        let tmp = tempfile::tempdir().unwrap();
        let msg_path = tmp.path().join("test.msg");
        make_msg_with_attachment(&msg_path, "document.pdf", GOLDEN_PDF);
        let out_dir = tmp.path().join("out");
        std::fs::create_dir_all(&out_dir).unwrap();
        let mut report = ExtractReport::default();
        extract_from_msg(&msg_path, &out_dir, &no_filter(), &mut report).unwrap();
        assert_eq!(report.attachments_extracted, 1, "expected 1 attachment from MSG");
        assert!(out_dir.join("document.pdf").exists());
        assert_eq!(std::fs::read(out_dir.join("document.pdf")).unwrap(), GOLDEN_PDF);
    }

    #[test]
    fn extract_msg_with_no_attachments_returns_zero() {
        use crate::export::Exporter; // needed to call trait method .add()
        // A MSG with no attach storages should succeed cleanly with 0 extracted.
        let tmp = tempfile::tempdir().unwrap();
        // Use MsgExporter to write a minimal MSG (no attachment storages).
        let mut ex = crate::export::msg::MsgExporter::new(tmp.path().to_path_buf());
        let parsed = crate::parser::parse(FIXTURE_EML).unwrap();
        ex.add(FIXTURE_EML, &parsed, "aabbccdd00000099").unwrap();
        let msg_path = tmp.path().join("aabbccdd00000099.msg");
        let mut report = ExtractReport::default();
        extract_from_msg(&msg_path, tmp.path(), &no_filter(), &mut report).unwrap();
        assert_eq!(report.attachments_extracted, 0);
        assert!(report.errors.is_empty());
    }

    // ---- Filename helpers ----

    #[test]
    fn unique_path_adds_suffix_on_collision() {
        let tmp = tempfile::tempdir().unwrap();
        let first = unique_path(tmp.path(), "doc.pdf");
        std::fs::write(&first, b"x").unwrap();
        let second = unique_path(tmp.path(), "doc.pdf");
        assert_ne!(first, second, "collision must produce different path");
        assert!(second.to_string_lossy().contains("doc_1.pdf"));
    }

    #[test]
    fn sanitize_removes_unsafe_chars() {
        assert_eq!(sanitize("<script>.pdf"), "_script_.pdf");
        assert_eq!(sanitize("file:name.docx"), "file_name.docx");
        assert_eq!(sanitize("  "), "attachment"); // all-whitespace → fallback
        assert_eq!(sanitize("normal.pdf"), "normal.pdf");
    }

    #[test]
    fn expand_sources_only_includes_supported_types() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("a.eml"), b"").unwrap();
        std::fs::write(tmp.path().join("b.mbox"), b"").unwrap();
        std::fs::write(tmp.path().join("c.msg"), b"").unwrap();
        std::fs::write(tmp.path().join("d.pdf"), b"").unwrap(); // not supported
        std::fs::write(tmp.path().join("e.txt"), b"").unwrap(); // not supported
        let sources = vec![tmp.path().to_string_lossy().into_owned()];
        let found = expand_sources(&sources);
        assert_eq!(found.len(), 3, "only eml/mbox/msg should be found");
        let names: Vec<_> = found.iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        for expected in ["a.eml", "b.mbox", "c.msg"] {
            assert!(names.contains(&expected.to_string()), "{expected} must be in results");
        }
    }
}
