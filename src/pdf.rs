use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};
use std::{env, fs, io};

/// A temporary PDF path that is deleted again when it goes out of scope.
pub struct TempFile(PathBuf);

impl TempFile {
    pub fn new(label: &str) -> Self {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        Self(env::temp_dir().join(format!("shoebox-{label}-{}-{n}.pdf", std::process::id())))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Extracts the text of a PDF. If it has no text layer (a scan), runs OCR on a
/// temporary copy and extracts the text from that. The original file is never changed.
pub fn extract_text(path: &Path, ocr_language: &str) -> crate::Res<String> {
    let text = read_text(path)?;
    if !text.trim().is_empty() {
        return Ok(text);
    }

    eprintln!(
        "{} has no text layer (probably a scan). Running OCR, this can take a while…",
        path.display()
    );
    let ocr_copy = TempFile::new("ocr");
    ocr(path, ocr_copy.path(), ocr_language, false)?;
    let text = read_text(ocr_copy.path())?;
    if text.trim().is_empty() {
        return Err(format!("OCR found no text in {}", path.display()).into());
    }
    Ok(text)
}

/// The text of each page, in page order.
pub fn text_by_pages(path: &Path) -> crate::Res<Vec<String>> {
    Ok(pdf_extract::extract_text_by_pages(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?)
}

fn read_text(path: &Path) -> crate::Res<String> {
    Ok(pdf_extract::extract_text(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?)
}

/// Runs `ocrmypdf` from `input` to `output`. Also straightens and rotates pages,
/// which fixes pages that were scanned upside down.
/// With `skip_text`, pages that already have text are left as they are.
pub fn ocr(input: &Path, output: &Path, language: &str, skip_text: bool) -> crate::Res<()> {
    let mut command = Command::new("ocrmypdf");
    command.args(["--quiet", "--rotate-pages", "--deskew", "-l", language]);
    if skip_text {
        command.arg("--skip-text");
    }
    // ocrmypdf's own messages are captured and shown only on failure; it reports
    // harmless errors for blank pages (nothing to recognise) even when it succeeds.
    let result = command.arg(input).arg(output).output().map_err(|e| {
        if e.kind() == io::ErrorKind::NotFound {
            "ocrmypdf is not installed. Install it with: brew install ocrmypdf tesseract-lang"
                .to_string()
        } else {
            format!("cannot run ocrmypdf: {e}")
        }
    })?;
    if !result.status.success() {
        let _ = fs::remove_file(output);
        return Err(format!(
            "ocrmypdf failed on {} ({}):\n{}",
            input.display(),
            result.status,
            String::from_utf8_lossy(&result.stderr).trim()
        )
        .into());
    }
    Ok(())
}
