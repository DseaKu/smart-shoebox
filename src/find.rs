//! `find`: ask a question, get the matching documents (and copies, with `--to`).

use crate::{
    Res,
    config::Config,
    export, index,
    index::Row,
    llm::Model,
    roles,
    search::{self, Selection},
    store,
    workspace::Workspace,
};
use std::path::Path;

/// Picks the rows to send (see `search`), asks the AI which ones match, prints
/// them and, with `to`, copies them into that folder under readable names.
pub async fn run(
    config: &Config,
    ws: &Workspace,
    llm: &impl Model,
    question: &str,
    to: Option<&Path>,
) -> Res<()> {
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

    let matches = ask(llm, &selection, question).await?;
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

/// The finder picks matching rows from the selection (with one retry, see
/// `Model::ask_for`). Numbers it was not given are ignored.
async fn ask<'a>(llm: &impl Model, selection: &Selection<'a>, question: &str) -> Res<Vec<&'a Row>> {
    let message = format!(
        "Index:\n{}\n\nQuestion: {question}",
        selection.prompt_lines()
    );
    let answer: search::Answer = llm
        .ask_for(roles::FINDER, &message)
        .await
        .map_err(|e| format!("the model gave no usable answer ({e})"))?;
    Ok(selection.resolve(&answer))
}
