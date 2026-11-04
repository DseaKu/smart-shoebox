//! `name`: renames PDFs in place to a readable name,
//! e.g. `IMG_20261103090559.pdf` → `2026-03-14_steuerbescheid-2025-finanzamt-muenchen.pdf`.

use crate::{
    Res, config::Config, hash, index, llm::Llm, naming, pdf, prompt, roles, workspace::Workspace,
};
use schemars::JsonSchema;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};

/// The model only needs the start of a document to name it.
const MAX_TEXT_CHARS: usize = 4000;

pub async fn run(config: &Config, ws: &Workspace, files: &[PathBuf]) -> Res<()> {
    let rows = index::parse(&index::read(&ws.index_path())?);
    let llm = Llm::new(config);
    let mut failed = 0;
    for file in files {
        if let Err(e) = name_one(config, &llm, &rows, file, None).await {
            eprintln!("error: {}: {e}", file.display());
            failed += 1;
        }
    }
    if failed > 0 {
        return Err(format!("{failed} of {} files could not be processed", files.len()).into());
    }
    Ok(())
}

/// Names a single file, e.g. for `check`.
/// `text` is the document's text if the caller already has it (e.g. `check`
/// after its own OCR), so it is not read and OCR'd a second time.
pub async fn name_file(config: &Config, ws: &Workspace, file: &Path, text: Option<&str>) -> Res<()> {
    let rows = index::parse(&index::read(&ws.index_path())?);
    name_one(config, &Llm::new(config), &rows, file, text).await
}

async fn name_one(
    config: &Config,
    llm: &Llm,
    rows: &[index::Row],
    file: &Path,
    known_text: Option<&str>,
) -> Res<()> {
    if !file.is_file() {
        return Err("does not exist".into());
    }
    let current = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    // Documents already in the box are named from their index row, without asking the AI.
    let hash = hash::sha256_file(file)?;
    let (date, title) = match rows.iter().find(|row| row.hash == hash) {
        Some(row) => (row.date.clone(), row.summary.clone()),
        None => {
            let text = match known_text {
                Some(text) => text.to_string(),
                None => pdf::extract_text(file, &config.ocr_language)?,
            };
            let start: String = text.chars().take(MAX_TEXT_CHARS).collect();
            eprintln!("Asking {} for a name for {}…", config.model, file.display());
            match llm.ask_for::<NameAnswer>(roles::NAMER, &start).await {
                Ok(answer) => (answer.date, answer.title),
                Err(e) => {
                    eprintln!("The model gave no usable answer ({e}); type a name yourself with [e]dit.");
                    (String::new(), String::new())
                }
            }
        }
    };

    let suggestion = naming::stem(&date, &[&title]).unwrap_or_else(|| current.clone());
    if suggestion == current {
        println!("{} already has a good name.", file.display());
        return Ok(());
    }
    let Some(stem) = prompt::confirm_name(file, &suggestion)? else {
        println!("Not renamed.");
        return Ok(());
    };
    if stem == current {
        return Ok(());
    }

    let dir = file.parent().unwrap_or(Path::new(""));
    let dest = naming::free_path(dir, &stem);
    fs::rename(file, &dest).map_err(|e| format!("cannot rename to {}: {e}", dest.display()))?;
    println!("Renamed to {}", dest.display());
    Ok(())
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
