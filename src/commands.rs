use crate::{
    Res, config::Config, entry::Entry, export, hash, index, llm::Llm, pdf, pdfdoc, pipeline,
    prompt, store, trash, workspace::Workspace,
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
    let (proposal, reviewed) = pipeline::add(&llm, &text, &index::tags(&index)).await?;
    let entry = Entry::parse(&reviewed)
        .or_else(|| Entry::parse(&proposal))
        .unwrap_or_else(|| {
            eprintln!("The model's answer could not be parsed, please fill in the fields.");
            Entry::default()
        });

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

    let llm = Llm::new(config);
    let answer = pipeline::find(&llm, &index, question).await?;
    let matches = select(&rows, &answer);
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

/// Picks the rows whose hash the model named, in the model's order.
/// Accepts a unique hash prefix of at least 8 characters, because small models
/// sometimes shorten hashes. Anything that is not a known hash is ignored.
fn select<'a>(rows: &'a [index::Row], answer: &str) -> Vec<&'a index::Row> {
    let mut selected: Vec<&index::Row> = Vec::new();
    let tokens = answer
        .split(|c: char| !c.is_ascii_hexdigit())
        .filter(|t| t.len() >= 8)
        .map(str::to_lowercase);
    for token in tokens {
        let mut hits = rows.iter().filter(|r| r.hash.starts_with(&token));
        if let (Some(row), None) = (hits.next(), hits.next())
            && !selected.iter().any(|s| s.hash == row.hash)
        {
            selected.push(row);
        }
    }
    selected
}
