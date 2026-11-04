use crate::{
    Res,
    index::Row,
    llm::Llm,
    roles,
    search::{self, Selection},
};

/// `add`: the extractor proposes an entry, the reviewer checks it against the text.
/// Returns (proposal, reviewed).
pub async fn add(llm: &Llm, text: &str, existing_tags: &[String]) -> Res<(String, String)> {
    let tag_list = if existing_tags.is_empty() {
        "(none yet)".to_string()
    } else {
        existing_tags.join(", ")
    };
    let context = format!("Existing tags: {tag_list}\n\nDocument text:\n{text}");

    let proposal = llm.ask(roles::EXTRACTOR, &context).await?;
    let reviewed = llm
        .ask(
            roles::REVIEWER,
            &format!("{context}\n\nProposed entry:\n{proposal}"),
        )
        .await?;
    Ok((proposal, reviewed))
}

/// `find`: the finder picks matching rows from the index.
/// Retries once if the answer is unusable (e.g. the model aborted or wrote no JSON).
pub async fn find<'a>(llm: &Llm, selection: &Selection<'a>, question: &str) -> Res<Vec<&'a Row>> {
    let message = format!("Index:\n{}\n\nQuestion: {question}", selection.prompt_lines());
    let mut error = String::new();
    for attempt in 0..2 {
        let prompt = if attempt == 0 {
            message.clone()
        } else {
            format!("{message}\n\nYour previous answer failed: {error}\nAnswer again with valid JSON.")
        };
        let result = llm
            .ask_structured::<search::Answer>(roles::FINDER, &prompt)
            .await
            .map_err(|e| e.to_string())
            .and_then(|answer| search::parse_answer(&answer));
        match result {
            Ok(answer) => return Ok(selection.resolve(&answer)),
            Err(e) => error = e,
        }
    }
    Err(format!("the model gave no usable answer ({error})").into())
}
