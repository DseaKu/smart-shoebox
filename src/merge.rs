//! `merge`: append PDFs into one. No AI, no OCR.

use crate::{Res, pdfdoc, trash};
use std::path::{Path, PathBuf};

/// Appends the PDFs, in the given order, into one new PDF, then offers to move
/// the originals to the Trash (only if every page made it into the result).
pub fn run(inputs: &[PathBuf], out: Option<&Path>) -> Res<()> {
    for input in inputs {
        if !input.is_file() {
            return Err(format!("{} does not exist", input.display()).into());
        }
    }
    let out = pdfdoc::output_path(out, &inputs[0], "merged")?;
    pdfdoc::merge(inputs, &out)?;
    let pages = pdfdoc::page_count(&out)?;
    println!(
        "Wrote {} ({} files, {pages} pages)",
        out.display(),
        inputs.len()
    );

    let expected = inputs
        .iter()
        .map(|f| pdfdoc::page_count(f))
        .sum::<Res<usize>>()?;
    if pages != expected {
        eprintln!("warning: expected {expected} pages, so the original files are kept.");
        return Ok(());
    }
    let mut originals: Vec<PathBuf> = Vec::new();
    for input in inputs {
        if !originals.contains(input) {
            originals.push(input.clone());
        }
    }
    trash::offer(&originals)
}
