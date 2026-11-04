use crate::{Res, llm::Llm, roles};

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
pub async fn find(llm: &Llm, index: &str, question: &str) -> Res<String> {
    llm.ask(
        roles::FINDER,
        &format!("Index:\n{index}\n\nQuestion: {question}"),
    )
    .await
}
