//! Reading a scan page by page, with OCR where needed.
//! Shared by `sort`, `check` and `counterpart`.

use crate::{
    Res, ocr,
    pages::{self, Page},
    pdf,
    pdf::TempFile,
    pdfdoc,
};
use std::path::{Path, PathBuf};

pub struct Scan {
    pub pages: Vec<Page>,
    input: PathBuf,
    /// A copy with the pages turned upright, if OCR found pages that are not.
    /// Deleted when the scan is dropped.
    upright_copy: Option<TempFile>,
}

impl Scan {
    /// A scan made of given pages, for tests that need no PDF.
    #[cfg(test)]
    pub fn from_pages(pages: Vec<Page>) -> Self {
        Self {
            pages,
            input: PathBuf::new(),
            upright_copy: None,
        }
    }

    /// Extracts the text of every page. Pages without text are read with OCR;
    /// pages that already have text are left as they are.
    pub fn read(input: &Path, ocr_language: &str) -> Res<Self> {
        if !input.is_file() {
            return Err(format!("{} does not exist", input.display()).into());
        }
        let mut texts = pdf::text_by_pages(input)?;
        let turns = ocr::fill_blank_pages(input, ocr_language, &mut texts)?;
        let mut upright_copy = None;
        if !turns.is_empty() {
            let copy = TempFile::new("scan-upright");
            pdfdoc::turn_pages(input, &turns, copy.path())?;
            upright_copy = Some(copy);
        }
        Ok(Self {
            pages: pages::analyze(texts),
            input: input.to_path_buf(),
            upright_copy,
        })
    }

    /// The version to build results from: the copy with the pages turned
    /// upright if there is one, else the input.
    pub fn source(&self) -> &Path {
        self.upright_copy
            .as_ref()
            .map_or(&self.input, TempFile::path)
    }
}
