use crate::{
    Res, config::Config, entry::Entry, export, hash, index, llm::Llm, pdf, pdfdoc, pipeline,
    prompt, search, store, trash, workspace::Workspace,
};
use std::path::{Path, PathBuf};

/// Hash → duplicate check → extract text (OCR if needed) → AI proposal and review
/// → my confirmation → copy into the box's `store/` → append to its `index.tsv`.
pub async fn add(config: &Config, ws: &Workspace, file: &Path) -> Res<()> {
    let hash = hash::sha256_file(file)?;
    let index = index::read(&ws.index_path())?;
    if index::contains(&index, &hash) {
        println!("Already in the index: {}", store::path(ws, &hash).display());
        return Ok(());
    }

    let text = pdf::extract_text(file, &config.ocr_language)?;

    eprintln!("Asking {} for date, tags and summary…", config.model);
    let llm = Llm::new(config);
    let entry = match pipeline::add(&llm, &text, &index::tags(&index), config.review).await {
        Ok(entry) => entry,
        Err(e) => {
            eprintln!("The model gave no usable answer ({e}); please fill in the fields.");
            Entry::default()
        }
    };

    let Some(entry) = prompt::confirm(entry)? else {
        println!("Not added.");
        return Ok(());
    };

    let original_name = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stored = store::copy(ws, file, &hash)?;
    index::append(&ws.index_path(), &hash, &original_name, &entry)?;
    println!("Added: {}", stored.display());
    Ok(())
}

/// Appends the PDFs, in the given order, into one new PDF. No AI, no OCR.
pub fn merge(inputs: &[PathBuf], out: Option<&Path>) -> Res<()> {
    for input in inputs {
        if !input.is_file() {
            return Err(format!("{} does not exist", input.display()).into());
        }
    }
    let out = pdfdoc::output_path(out, &inputs[0], "merged")?;
    pdfdoc::merge(inputs, &out)?;
    let pages = pdfdoc::page_count(&out)?;
    println!("Wrote {} ({} files, {pages} pages)", out.display(), inputs.len());

    // Only offer to delete the originals if every page made it into the result.
    let expected = inputs.iter().map(|f| pdfdoc::page_count(f)).sum::<Res<usize>>()?;
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

/// Asks the AI which index rows match the question, prints them and,
/// with `to`, copies them into that folder under readable names.
pub async fn find(config: &Config, ws: &Workspace, question: &str, to: Option<&Path>) -> Res<()> {
    let index = index::read(&ws.index_path())?;
    if index::is_empty(&index) {
        return Err("index.tsv is empty, nothing to search yet.".into());
    }
    let rows = index::parse(&index);
    let selection = search::select(&rows, question, config.context_size);
    if selection.left_out > 0 && selection.focused {
        eprintln!(
            "Searching the {} of {} documents that match your words best.",
            selection.rows.len(),
            rows.len()
        );
    } else if selection.left_out > 0 {
        eprintln!(
            "Searching {} of {} documents (the newest); the rest do not fit into the model's \
             context. Use words from the document, or raise context_size in shoebox.yaml.",
            selection.rows.len(),
            rows.len()
        );
    }

    let llm = Llm::new(config);
    let matches = pipeline::find(&llm, &selection, question).await?;
    if matches.is_empty() {
        println!("No matching document found.");
        return Ok(());
    }

    let mut failed = false;
    for row in matches {
        println!("{}  {}", row.date, row.summary);
        println!("  tags: {}", row.tags);
        println!("  {}", store::path(ws, &row.hash).display());
        if let Some(dir) = to {
            match export::copy_to(ws, row, dir) {
                Ok(export::Copied::New(dest)) => println!("  copied to {}", dest.display()),
                Ok(export::Copied::AlreadyThere(dest)) => {
                    println!("  already there: {}", dest.display())
                }
                Err(e) => {
                    eprintln!("  error: {e}");
                    failed = true;
                }
            }
        }
        println!();
    }
    if failed {
        return Err("some documents could not be copied".into());
    }
    Ok(())
}
