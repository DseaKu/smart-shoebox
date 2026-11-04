//! `add`: put a PDF into the box with a date, tags and a summary.

use crate::{
    Res, config::Config, entry::Entry, hash, index, llm::Model, pdf, prompt, roles, store,
    workspace::Workspace,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Hash → duplicate check → extract text (OCR if needed) → AI proposal (and
/// review, if on) → my confirmation → copy into `store/` → append to `index.tsv`.
pub async fn run(config: &Config, ws: &Workspace, llm: &impl Model, file: &Path) -> Res<()> {
    let proposal = match prepare(config, ws, llm, file).await? {
        Prepared::AlreadyThere(stored) => {
            println!("Already in the index: {}", stored.display());
            return Ok(());
        }
        Prepared::New(proposal) => proposal,
    };
    if let Some(e) = &proposal.model_error {
        eprintln!("The model gave no usable answer ({e}); please fill in the fields.");
    }
    let Some(entry) = prompt::confirm(proposal.entry.clone())? else {
        println!("Not added.");
        return Ok(());
    };
    let stored = save(ws, &proposal, &entry)?;
    println!("Added: {}", stored.display());
    Ok(())
}

/// What `prepare` found out about a file.
pub enum Prepared {
    /// The identical file is already in the box (here).
    AlreadyThere(PathBuf),
    New(Proposal),
}

/// A file ready to be added, with the AI's proposal for its entry.
#[derive(Clone)]
pub struct Proposal {
    pub file: PathBuf,
    pub hash: String,
    pub entry: Entry,
    /// Set if the model gave no usable answer; `entry` is then empty.
    pub model_error: Option<String>,
}

/// Everything before my confirmation: hash, duplicate check, text, AI proposal.
/// Shared by the command line and the GUI.
pub async fn prepare(
    config: &Config,
    ws: &Workspace,
    llm: &impl Model,
    file: &Path,
) -> Res<Prepared> {
    let hash = hash::sha256_file(file)?;
    let index = index::read(&ws.index_path())?;
    if index::contains(&index, &hash) {
        return Ok(Prepared::AlreadyThere(store::path(ws, &hash)));
    }

    let text = pdf::extract_text(file, &config.ocr_language)?;

    eprintln!("Asking {} for date, tags and summary…", llm.name());
    let (entry, model_error) = match propose(llm, &text, &index::tags(&index), config.review).await
    {
        Ok(entry) => (entry, None),
        Err(e) => (Entry::default(), Some(e)),
    };
    Ok(Prepared::New(Proposal {
        file: file.to_path_buf(),
        hash,
        entry,
        model_error,
    }))
}

/// After my confirmation: copy into `store/` and append the (cleaned) entry.
pub fn save(ws: &Workspace, proposal: &Proposal, entry: &Entry) -> Res<PathBuf> {
    let entry = entry.clone().normalized();
    let original_name = proposal
        .file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stored = store::copy(ws, &proposal.file, &proposal.hash)?;
    index::append(&ws.index_path(), &proposal.hash, &original_name, &entry)?;
    Ok(stored)
}

/// The extractor's and reviewer's answer.
#[derive(Deserialize, Serialize, JsonSchema)]
struct EntryAnswer {
    date: String,
    tags: Vec<String>,
    summary: String,
}

impl From<EntryAnswer> for Entry {
    fn from(answer: EntryAnswer) -> Self {
        Entry {
            date: answer.date,
            tags: answer.tags.join(","),
            summary: answer.summary,
        }
        .normalized()
    }
}

/// The extractor proposes an entry; with `review`, the reviewer checks it against
/// the text. If the review fails, the proposal is used. Fails only if the
/// extractor gives no usable answer, even after one retry.
async fn propose(
    llm: &impl Model,
    text: &str,
    existing_tags: &[String],
    review: bool,
) -> Result<Entry, String> {
    let tag_list = if existing_tags.is_empty() {
        "(none yet)".to_string()
    } else {
        existing_tags.join(", ")
    };
    let context = format!("Existing tags: {tag_list}\n\nDocument text:\n{text}");

    let proposal: EntryAnswer = llm.ask_for(roles::EXTRACTOR, &context).await?;
    if !review {
        return Ok(proposal.into());
    }
    let proposal_json = serde_json::to_string(&proposal).map_err(|e| e.to_string())?;
    match llm
        .ask_for::<EntryAnswer>(
            roles::REVIEWER,
            &format!("{context}\n\nProposed entry:\n{proposal_json}"),
        )
        .await
    {
        Ok(reviewed) => Ok(reviewed.into()),
        Err(e) => {
            eprintln!("The review gave no usable answer ({e}); using the first proposal.");
            Ok(proposal.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answer_becomes_a_clean_entry() {
        let answer: EntryAnswer = crate::llm::parse_json(
            r#"{"date": "2026-03-14", "tags": ["Steuer", " finanzamt ", ""], "summary": "Steuerbescheid\t2025"}"#,
        )
        .unwrap();
        let entry: Entry = answer.into();
        assert_eq!(entry.date, "2026-03-14");
        assert_eq!(entry.tags, "steuer,finanzamt");
        assert_eq!(entry.summary, "Steuerbescheid 2025");
    }
}
