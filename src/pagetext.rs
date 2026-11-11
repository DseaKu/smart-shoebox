//! The text of each page of a stored document, kept next to the PDF as
//! `store/<hash>.pages.json`. It is the source for searching *inside* documents
//! (ROADMAP step 7), so the PDF is read and OCR'd only once, when it is added.
//!
//! `shoebox text` fills it in for documents added before this file existed.

use crate::{Res, config::Config, index, pages, pdf, pdf::TempFile, store, workspace::Workspace};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// The file's content. A struct (not a bare list), so fields can be added later.
#[derive(Serialize, Deserialize)]
struct PageFile {
    /// Page texts in page order; page 1 is `pages[0]`. Blank pages stay as
    /// (nearly) empty strings, so the positions match the PDF's pages.
    pages: Vec<String>,
}

/// `<box>/store/<hash>.pages.json`.
pub fn path(ws: &Workspace, hash: &str) -> PathBuf {
    ws.store_dir().join(format!("{hash}.pages.json"))
}

/// Writes the page texts. Writes a temporary file first, so an interrupted
/// write never looks like a finished one.
pub fn save(ws: &Workspace, hash: &str, page_texts: &[String]) -> Res<()> {
    let dest = path(ws, hash);
    let json = serde_json::to_string_pretty(&PageFile {
        pages: page_texts.to_vec(),
    })?;
    let tmp = dest.with_extension("json.part");
    fs::write(&tmp, json).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    fs::rename(&tmp, &dest)?;
    Ok(())
}

/// The page texts of a stored document, or `None` if they were never saved.
pub fn load(ws: &Workspace, hash: &str) -> Res<Option<Vec<String>>> {
    let file = path(ws, hash);
    if !file.is_file() {
        return Ok(None);
    }
    let json =
        fs::read_to_string(&file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    let content: PageFile =
        serde_json::from_str(&json).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    Ok(Some(content.pages))
}

/// Reads the text of each page. Pages without text (scanned pages, also in a
/// PDF that is partly digital) are OCR'd on a temporary copy; the file itself is
/// never changed. If OCR fails but other pages have text, those are kept with a
/// warning, so a document with one blank page can still be added without OCR.
pub fn read_pdf(file: &Path, ocr_language: &str) -> Res<Vec<String>> {
    let texts = pdf::text_by_pages(file)?;
    if !texts.iter().any(|t| pages::is_blank(t)) {
        return Ok(texts);
    }
    let has_text = texts.iter().any(|t| !pages::is_blank(t));
    if !has_text {
        eprintln!(
            "{} has no text layer (probably a scan). Running OCR, this can take a while…",
            file.display()
        );
    }
    let copy = TempFile::new("pages-ocr");
    match pdf::ocr(file, copy.path(), ocr_language, true)
        .and_then(|()| pdf::text_by_pages(copy.path()))
    {
        Ok(ocr_texts) if ocr_texts.iter().any(|t| !pages::is_blank(t)) => Ok(ocr_texts),
        Ok(_) if !has_text => Err(format!("OCR found no text in {}", file.display()).into()),
        Err(e) if !has_text => Err(e),
        Ok(_) => Ok(texts),
        Err(e) => {
            eprintln!(
                "warning: OCR of the empty pages of {} failed, keeping the other pages' text: {e}",
                file.display()
            );
            Ok(texts)
        }
    }
}

/// The page texts as one text, as the AI gets it.
pub fn joined(page_texts: &[String]) -> String {
    page_texts.join("\n")
}

/// `shoebox text`: saves the page text of every stored document that has none
/// yet (with `all`: of every document). Continues after a broken file.
pub fn backfill(config: &Config, ws: &Workspace, all: bool) -> Res<()> {
    let rows = index::parse(&index::read(&ws.index_path())?);
    let todo: Vec<&index::Row> = rows
        .iter()
        .filter(|row| all || !path(ws, &row.hash).is_file())
        .collect();
    if todo.is_empty() {
        println!("All {} documents already have their page text.", rows.len());
        return Ok(());
    }
    let mut failed = 0;
    for (i, row) in todo.iter().enumerate() {
        let pdf = store::path(ws, &row.hash);
        eprintln!(
            "[{}/{}] {} ({})",
            i + 1,
            todo.len(),
            row.summary,
            row.original_name
        );
        let result = if pdf.is_file() {
            read_pdf(&pdf, &config.ocr_language).and_then(|texts| save(ws, &row.hash, &texts))
        } else {
            Err(format!("{} is missing", pdf.display()).into())
        };
        if let Err(e) = result {
            eprintln!("error: {e}");
            failed += 1;
        }
    }
    println!(
        "Saved the page text of {} of {} documents.",
        todo.len() - failed,
        todo.len()
    );
    if failed > 0 {
        return Err(format!("{failed} documents could not be read").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{TestBox, pdf};

    #[test]
    fn saved_pages_load_again() {
        let t = TestBox::new("pagetext-roundtrip");
        assert!(load(&t.ws, "abc").unwrap().is_none());
        fs::create_dir_all(t.ws.store_dir()).unwrap();
        let texts = vec!["Seite 1 von 2\nMiete".to_string(), String::new()];
        save(&t.ws, "abc", &texts).unwrap();
        assert_eq!(load(&t.ws, "abc").unwrap(), Some(texts));
        assert!(!path(&t.ws, "abc").with_extension("json.part").exists());
    }

    #[test]
    fn reads_one_text_per_page() {
        let t = TestBox::new("pagetext-read");
        let file = t.dir.join("two.pdf");
        pdf(
            &file,
            &[
                "Seite 1 von 2 Mietvertrag",
                "Seite 2 von 2 Kaution 2340 EUR",
            ],
        );
        let texts = read_pdf(&file, "deu").unwrap();
        assert_eq!(texts.len(), 2);
        assert!(texts[0].contains("Mietvertrag"));
        assert!(texts[1].contains("Kaution"));
    }
}
