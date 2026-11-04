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
    #[serde(default = "default_ocr_language")]
    pub ocr_language: String,
    /// `add`: let a second AI call check the first proposal against the text.
    #[serde(default = "default_review")]
    pub review: bool,
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
    Ok(serde_yaml_ng::from_str(&text).map_err(|e| format!("invalid {}: {e}", path.display()))?)
}
