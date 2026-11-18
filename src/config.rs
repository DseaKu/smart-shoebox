use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize, Clone)]
pub struct Config {
    /// Ollama model used for all AI steps.
    pub model: String,
    /// Context window in tokens. Long documents are cut off silently if this is too small.
    #[serde(default = "default_context_size")]
    pub context_size: u64,
    /// Tesseract language(s) for OCR of scanned PDFs, e.g. "deu" or "deu+eng".
    /// "deu" and "eng" are built in (see `ocr`).
    #[serde(default = "default_ocr_language")]
    pub ocr_language: String,
    /// `add`: let a second AI call check the first proposal against the text.
    #[serde(default = "default_review")]
    pub review: bool,
    /// Ollama model that turns chunks of page text into vectors, for searching
    /// inside documents. A different model means embedding everything again.
    #[serde(default = "default_embed_model")]
    pub embed_model: String,
}

/// Multilingual (German text), no special prefix for questions needed.
fn default_embed_model() -> String {
    "bge-m3".to_string()
}

/// Off by default: with small models the reviewer made tags worse, and tripled
/// the time (see ROADMAP.md, step 3).
fn default_review() -> bool {
    false
}

fn default_context_size() -> u64 {
    16384
}

fn default_ocr_language() -> String {
    "deu".to_string()
}

/// Loads the box's `shoebox.yaml`.
pub fn load(path: &Path) -> crate::Res<Config> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let config: Config =
        serde_yaml_ng::from_str(&text).map_err(|e| format!("invalid {}: {e}", path.display()))?;
    for model in [&config.model, &config.embed_model] {
        check_local(model).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(config)
}

/// Refuses Ollama's cloud models (names like `gemma4:cloud`): they run on
/// Ollama's servers, so the documents would leave this computer.
pub fn check_local(model: &str) -> crate::Res<()> {
    let tag = model.rsplit_once(':').map_or("", |(_, tag)| tag);
    if tag == "cloud" || tag.ends_with("-cloud") {
        return Err(format!(
            "\"{model}\" is a cloud model: it would send the documents to Ollama's \
             servers. Choose a local model."
        )
        .into());
    }
    Ok(())
}

/// Sets `model:` in the box's `shoebox.yaml` and keeps everything else,
/// including the comments.
pub fn set_model(path: &Path, model: &str) -> crate::Res<()> {
    let model = model.trim();
    if model.is_empty() || model.contains(char::is_whitespace) {
        return Err(format!("\"{model}\" is not a model name").into());
    }
    check_local(model)?;
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let line = format!("model: {model}");
    let mut found = false;
    let mut lines: Vec<String> = text
        .lines()
        .map(|l| {
            if !found && l.starts_with("model:") {
                found = true;
                line.clone()
            } else {
                l.to_string()
            }
        })
        .collect();
    if !found {
        lines.insert(0, line);
    }
    let tmp = path.with_extension("yaml.part");
    std::fs::write(&tmp, lines.join("\n") + "\n")
        .map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_models_are_refused() {
        assert!(check_local("gemma4:cloud").is_err());
        assert!(check_local("gpt-oss:120b-cloud").is_err());
        assert!(check_local("qwen3:8b").is_ok());
        assert!(check_local("bge-m3").is_ok());
    }

    #[test]
    fn set_model_keeps_comments_and_other_settings() {
        let path = std::env::temp_dir().join(format!(
            "shoebox-test-{}-set-model.yaml",
            std::process::id()
        ));
        std::fs::write(&path, "# The model\nmodel: qwen3:8b\n\nocr_language: deu\n").unwrap();
        set_model(&path, "qwen3:4b").unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, "# The model\nmodel: qwen3:4b\n\nocr_language: deu\n");
        assert_eq!(load(&path).unwrap().model, "qwen3:4b");
        assert!(set_model(&path, "gemma4:cloud").is_err());
        assert!(set_model(&path, "two words").is_err());
        let _ = std::fs::remove_file(path);
    }
}
