//! `add`: put a PDF into the box with a date, tags and a summary.

use crate::{
    Res, config::Config, entry::Entry, hash, index, llm::Model, pdf, prompt, roles, store,
    workspace::Workspace,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Hash → duplicate check → extract text (OCR if needed) → AI proposal (and
/// review, if on) → my confirmation → copy into `store/` → append to `index.tsv`.
pub async fn run(config: &Config, ws: &Workspace, llm: &impl Model, file: &Path) -> Res<()> {
    let hash = hash::sha256_file(file)?;
    let index = index::read(&ws.index_path())?;
    if index::contains(&index, &hash) {
        println!("Already in the index: {}", store::path(ws, &hash).display());
        return Ok(());
    }

    let text = pdf::extract_text(file, &config.ocr_language)?;

    eprintln!("Asking {} for date, tags and summary…", llm.name());
    let entry = match propose(llm, &text, &index::tags(&index), config.review).await {
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
