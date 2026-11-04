use std::cell::Cell;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Once;
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
    guarded(path, || pdf_extract::extract_text_by_pages(path))
}

fn read_text(path: &Path) -> crate::Res<String> {
    guarded(path, || pdf_extract::extract_text(path))
}

thread_local! {
    /// True while a guarded PDF library call runs on this thread.
    static IN_GUARD: Cell<bool> = const { Cell::new(false) };
}

/// Runs a PDF library call and turns a panic into a normal error for this file.
///
/// PDF libraries (`pdf-extract` in about 100 places) panic on PDFs they do not
/// understand, e.g. a page using a font that does not exist. Without this, one
/// odd file would end a whole batch run. The panic's own message is silenced only
/// while a guarded call runs; panics anywhere else are reported as usual.
pub fn guarded<T, E: std::fmt::Display>(path: &Path, call: impl FnOnce() -> Result<T, E>) -> crate::Res<T> {
    static QUIET_HOOK: Once = Once::new();
    QUIET_HOOK.call_once(|| {
        let default = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            if !IN_GUARD.with(Cell::get) {
                default(info);
            }
        }));
    });

    IN_GUARD.with(|g| g.set(true));
    let result = panic::catch_unwind(AssertUnwindSafe(call));
    IN_GUARD.with(|g| g.set(false));

    match result {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(e)) => Err(format!("cannot read {}: {e}", path.display()).into()),
        Err(payload) => {
            let reason = payload
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown reason".to_string());
            Err(format!(
                "cannot read {}: the PDF reader crashed on this file ({reason})",
                path.display()
            )
            .into())
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::content::{Content, Operation};
    use lopdf::{Dictionary, Document, Object, Stream, dictionary};

    fn temp(name: &str) -> PathBuf {
        env::temp_dir().join(format!("shoebox-test-{}-{name}.pdf", std::process::id()))
    }

    /// A one-page PDF that writes "Hi" with font `font` and the given fonts.
    /// `pdf-extract` panics on both variants used below.
    fn broken_pdf(path: &Path, fonts: Dictionary, font: &str) {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let ops = vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec![font.into(), 12.into()]),
            Operation::new("Tj", vec![Object::string_literal("Hi")]),
            Operation::new("ET", vec![]),
        ];
        let content_id =
            doc.add_object(Stream::new(dictionary! {}, Content { operations: ops }.encode().unwrap()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
            "Resources" => dictionary! { "Font" => fonts },
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page_id.into()], "Count" => 1 }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        doc.save(path).unwrap();
    }

    #[test]
    fn crashing_pdfs_become_errors() {
        let missing_font = temp("missing-font");
        broken_pdf(&missing_font, dictionary! {}, "F9");
        let bad_font = temp("bad-font");
        let type0 = dictionary! { "Type" => "Font", "Subtype" => "Type0", "BaseFont" => "X" };
        broken_pdf(&bad_font, dictionary! { "F1" => type0 }, "F1");

        for path in [&missing_font, &bad_font] {
            let err = text_by_pages(path).unwrap_err().to_string();
            assert!(err.contains("crashed"), "{err}");
            let err = read_text(path).unwrap_err().to_string();
            assert!(err.contains("crashed"), "{err}");
            let _ = fs::remove_file(path);
        }
        // The guard is off again afterwards, so real bugs elsewhere still report.
        assert!(!IN_GUARD.with(Cell::get));
    }

    #[test]
    fn damaged_file_is_a_normal_error() {
        let path = temp("damaged");
        fs::write(&path, b"%PDF-1.5\nthis is not really a pdf").unwrap();
        let err = text_by_pages(&path).unwrap_err().to_string();
        assert!(err.starts_with("cannot read"), "{err}");
        let _ = fs::remove_file(&path);
    }
}
