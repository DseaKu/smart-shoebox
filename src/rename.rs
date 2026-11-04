//! `name`: renames PDFs in place to a readable name,
//! e.g. `IMG_20261103090559.pdf` → `2026-03-14_steuerbescheid-2025-finanzamt-muenchen.pdf`.

use crate::{
    Res, config::Config, hash, index, llm::Model, naming, pdf, prompt, roles, workspace::Workspace,
};
use schemars::JsonSchema;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

/// The model only needs the start of a document to name it.
const MAX_TEXT_CHARS: usize = 4000;

pub async fn run(config: &Config, ws: &Workspace, llm: &impl Model, files: &[PathBuf]) -> Res<()> {
    let mut failed = 0;
    for file in files {
        if let Err(e) = name_file(config, ws, llm, file, None).await {
            eprintln!("error: {}: {e}", file.display());
            failed += 1;
        }
    }
    if failed > 0 {
        return Err(format!("{failed} of {} files could not be processed", files.len()).into());
    }
    Ok(())
}

/// Names a single file, asking me first.
/// `text` is the document's text if the caller already has it (e.g. `check`
/// after its own OCR), so it is not read and OCR'd a second time.
pub async fn name_file(
    config: &Config,
    ws: &Workspace,
    llm: &impl Model,
    file: &Path,
    text: Option<&str>,
) -> Res<()> {
    let proposal = propose(config, ws, llm, file, text).await?;
    if let Some(e) = &proposal.model_error {
        eprintln!("The model gave no usable answer ({e}); type a name yourself with [e]dit.");
    }
    if proposal.suggestion == proposal.current {
        println!("{} already has a good name.", file.display());
        return Ok(());
    }
    let Some(stem) = prompt::confirm_name(file, &proposal.suggestion)? else {
        println!("Not renamed.");
        return Ok(());
    };
    if stem == proposal.current {
        return Ok(());
    }
    let dest = apply(file, &stem)?;
    println!("Renamed to {}", dest.display());
    Ok(())
}

/// A proposed new name for a file.
pub struct NameProposal {
    pub file: PathBuf,
    /// The current name, without `.pdf`.
    pub current: String,
    /// The proposed name, without `.pdf` (the current one if nothing better was found).
    pub suggestion: String,
    /// Set if the model gave no usable answer.
    pub model_error: Option<String>,
}

/// Proposes a readable name. Documents already in the box are named from their
/// index row, without asking the AI.
pub async fn propose(
    config: &Config,
    ws: &Workspace,
    llm: &impl Model,
    file: &Path,
    known_text: Option<&str>,
) -> Res<NameProposal> {
    if !file.is_file() {
        return Err("does not exist".into());
    }
    let current = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let rows = index::parse(&index::read(&ws.index_path())?);
    let hash = hash::sha256_file(file)?;
    let mut model_error = None;
    let (date, title) = match rows.iter().find(|row| row.hash == hash) {
        Some(row) => (row.date.clone(), row.summary.clone()),
        None => {
            let text = match known_text {
                Some(text) => text.to_string(),
                None => pdf::extract_text(file, &config.ocr_language)?,
            };
            let start: String = text.chars().take(MAX_TEXT_CHARS).collect();
            eprintln!("Asking {} for a name for {}…", llm.name(), file.display());
            match llm.ask_for::<NameAnswer>(roles::NAMER, &start).await {
                Ok(answer) => (answer.date, answer.title),
                Err(e) => {
                    model_error = Some(e);
                    (String::new(), String::new())
                }
            }
        }
    };
    let suggestion = naming::stem(&date, &[&title]).unwrap_or_else(|| current.clone());
    Ok(NameProposal {
        file: file.to_path_buf(),
        current,
        suggestion,
        model_error,
    })
}

/// Renames the file in its folder to `<stem>.pdf` (or `<stem>-2.pdf`, … if taken).
pub fn apply(file: &Path, stem: &str) -> Res<PathBuf> {
    let dir = file.parent().unwrap_or(Path::new(""));
    let dest = naming::free_path(dir, stem);
    fs::rename(file, &dest).map_err(|e| format!("cannot rename to {}: {e}", dest.display()))?;
    Ok(dest)
}

/// The namer's answer.
#[derive(Deserialize, JsonSchema)]
struct NameAnswer {
    date: String,
    title: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_models_answer() {
        let answer: NameAnswer = crate::llm::parse_json(
            "```json\n{\"date\": \"2026-03-14\", \"title\": \"Steuerbescheid 2025 Finanzamt München\"}\n```",
        )
        .unwrap();
        assert_eq!(answer.date, "2026-03-14");
        assert_eq!(answer.title, "Steuerbescheid 2025 Finanzamt München");
        assert!(crate::llm::parse_json::<NameAnswer>("date: 2026-03-14").is_err());
    }
}
