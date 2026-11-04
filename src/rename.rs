//! `name`: renames PDFs in place to a readable name,
//! e.g. `IMG_20261103090559.pdf` → `2026-03-14_steuerbescheid-2025-finanzamt-muenchen.pdf`.

use crate::{
    Res, config::Config, hash, index, llm::Llm, naming, pdf, prompt, roles, workspace::Workspace,
};
use std::fs;
use std::path::{Path, PathBuf};

/// The model only needs the start of a document to name it.
const MAX_TEXT_CHARS: usize = 4000;

pub async fn run(config: &Config, ws: &Workspace, files: &[PathBuf]) -> Res<()> {
    let rows = index::parse(&index::read(&ws.index_path())?);
    let llm = Llm::new(config);
    let mut failed = 0;
    for file in files {
        if let Err(e) = name_one(config, &llm, &rows, file).await {
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
pub async fn name_file(config: &Config, ws: &Workspace, file: &Path) -> Res<()> {
    let rows = index::parse(&index::read(&ws.index_path())?);
    name_one(config, &Llm::new(config), &rows, file).await
}

async fn name_one(config: &Config, llm: &Llm, rows: &[index::Row], file: &Path) -> Res<()> {
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
            let text = pdf::extract_text(file, &config.ocr_language)?;
            let start: String = text.chars().take(MAX_TEXT_CHARS).collect();
            eprintln!("Asking {} for a name for {}…", config.model, file.display());
            parse_answer(&llm.ask(roles::NAMER, &start).await?)
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

/// Reads `date: …` and `title: …` from the model's answer; missing lines stay empty.
fn parse_answer(answer: &str) -> (String, String) {
    let (mut date, mut title) = (String::new(), String::new());
    for line in answer.lines() {
        let line = line.trim().trim_start_matches(['-', '*', ' ']);
        let Some((key, value)) = line.split_once(':') else { continue };
        let value = value.trim().trim_matches(['*', '"']).trim().to_string();
        match key.trim().trim_matches('*').to_lowercase().as_str() {
            "date" => date = value,
            "title" => title = value,
            _ => {}
        }
    }
    (date, title)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_models_answer() {
        let (date, title) = parse_answer("date: 2026-03-14\ntitle: \"Steuerbescheid 2025 Finanzamt München\"");
        assert_eq!(date, "2026-03-14");
        assert_eq!(title, "Steuerbescheid 2025 Finanzamt München");
        assert_eq!(parse_answer("no idea"), (String::new(), String::new()));
    }
}
