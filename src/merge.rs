//! `merge`: append PDFs into one. No AI, no OCR.

use crate::{Res, pdfdoc, trash};
use std::path::{Path, PathBuf};

/// Appends the PDFs, in the given order, into one new PDF, then offers to move
/// the originals to the Trash (only if every page made it into the result).
pub fn run(inputs: &[PathBuf], out: Option<&Path>) -> Res<()> {
    let merged = merge(inputs, out)?;
    println!(
        "Wrote {} ({} files, {} pages)",
        merged.out.display(),
        inputs.len(),
        merged.pages
    );
    if !merged.complete {
        eprintln!(
            "warning: expected {} pages, so the original files are kept.",
            merged.expected
        );
        return Ok(());
    }
    trash::offer(&originals(inputs))
}

pub struct Merged {
    pub out: PathBuf,
    pub pages: usize,
    pub expected: usize,
    /// Every page of every input made it into the result.
    pub complete: bool,
}

/// Writes the merged PDF (default `<first>_merged.pdf`) and checks the page count.
pub fn merge(inputs: &[PathBuf], out: Option<&Path>) -> Res<Merged> {
    if inputs.len() < 2 {
        return Err("merging needs at least two PDFs".into());
    }
    for input in inputs {
        if !input.is_file() {
            return Err(format!("{} does not exist", input.display()).into());
        }
    }
    let out = pdfdoc::output_path(out, &inputs[0], "merged")?;
    pdfdoc::merge(inputs, &out)?;
    let pages = pdfdoc::page_count(&out)?;
    let expected = inputs
        .iter()
        .map(|f| pdfdoc::page_count(f))
        .sum::<Res<usize>>()?;
    Ok(Merged {
        out,
        pages,
        expected,
        complete: pages == expected,
    })
}

/// The inputs without repeats (the same file may be merged twice).
pub fn originals(inputs: &[PathBuf]) -> Vec<PathBuf> {
    let mut originals: Vec<PathBuf> = Vec::new();
    for input in inputs {
        if !originals.contains(input) {
            originals.push(input.clone());
        }
    }
    originals
}
