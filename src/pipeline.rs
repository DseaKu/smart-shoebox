use crate::{
    Res,
    entry::Entry,
    index::Row,
    llm::Model,
    roles,
    search::{self, Selection},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The extractor's and reviewer's answer.
#[derive(Deserialize, Serialize, JsonSchema)]
pub struct EntryAnswer {
    pub date: String,
    pub tags: Vec<String>,
    pub summary: String,
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

/// `add`: the extractor proposes an entry; with `review`, the reviewer checks it
/// against the text. If the review fails, the proposal is used. Fails only if
/// the extractor gives no usable answer, even after one retry.
pub async fn add(llm: &impl Model, text: &str, existing_tags: &[String], review: bool) -> Result<Entry, String> {
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
        .ask_for::<EntryAnswer>(roles::REVIEWER, &format!("{context}\n\nProposed entry:\n{proposal_json}"))
        .await
    {
        Ok(reviewed) => Ok(reviewed.into()),
        Err(e) => {
            eprintln!("The review gave no usable answer ({e}); using the first proposal.");
            Ok(proposal.into())
        }
    }
}

/// `find`: the finder picks matching rows from the index.
/// Retries once if the answer is unusable (see `Llm::ask_for`).
pub async fn find<'a>(llm: &impl Model, selection: &Selection<'a>, question: &str) -> Res<Vec<&'a Row>> {
    let message = format!("Index:\n{}\n\nQuestion: {question}", selection.prompt_lines());
    let answer: search::Answer = llm
        .ask_for(roles::FINDER, &message)
        .await
        .map_err(|e| format!("the model gave no usable answer ({e})"))?;
    Ok(selection.resolve(&answer))
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
