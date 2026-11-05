//! Reading a scan page by page, with OCR where needed.
//! Shared by `sort` and `check`.

use crate::{
    Res,
    pages::{self, Page},
    pdf::{self, TempFile},
};
use std::path::{Path, PathBuf};

pub struct Scan {
    pub pages: Vec<Page>,
    input: PathBuf,
    /// The OCR'd copy, if OCR was needed. Deleted when the scan is dropped.
    ocr_copy: Option<TempFile>,
}

impl Scan {
    /// A scan made of given pages, for tests that need no PDF.
    #[cfg(test)]
    pub fn from_pages(pages: Vec<Page>) -> Self {
        Self {
            pages,
            input: PathBuf::new(),
            ocr_copy: None,
        }
    }

    /// Extracts the text of every page. If any page has no text, runs OCR on a
    /// temporary copy (pages that already have text are left as they are).
    pub fn read(input: &Path, ocr_language: &str) -> Res<Self> {
        if !input.is_file() {
            return Err(format!("{} does not exist", input.display()).into());
        }
        let mut texts = pdf::text_by_pages(input)?;
        let mut ocr_copy = None;
        if texts.iter().any(|t| pages::is_blank(t)) {
            let copy = TempFile::new("scan-ocr");
            pdf::ocr(input, copy.path(), ocr_language, true)?;
            texts = pdf::text_by_pages(copy.path())?;
            ocr_copy = Some(copy);
        }
        Ok(Self {
            pages: pages::analyze(texts),
            input: input.to_path_buf(),
            ocr_copy,
        })
    }

    /// The searchable, straightened version: the OCR'd copy if OCR ran, else the input.
    /// Results are built from this, so they need no second OCR.
    pub fn source(&self) -> &Path {
        self.ocr_copy.as_ref().map_or(&self.input, TempFile::path)
    }
}
