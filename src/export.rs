use crate::{Res, hash, index::Row, store, workspace::Workspace};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_SLUG_LEN: usize = 60;

/// Common German and English filler words left out of file names.
const FILLER_WORDS: &[&str] = &[
    "von", "vom", "mit", "fuer", "und", "der", "die", "das", "den", "dem", "des", "zur", "zum",
    "im", "in", "am", "an", "auf", "bei", "ein", "eine", "einer", "einem", "einen", "ueber",
    "the", "of", "and", "for", "with", "to", "a", "an",
];

pub enum Copied {
    New(PathBuf),
    AlreadyThere(PathBuf),
}

/// Copies the stored document into `dir` under a readable name, e.g.
/// `2026-03-14_steuerbescheid-finanzamt-muenchen-nachzahlung.pdf`.
/// Never overwrites: if the name is taken by a different file, `-2`, `-3`, … is
/// appended; if the identical file is already there, nothing is copied.
pub fn copy_to(ws: &Workspace, row: &Row, dir: &Path) -> Res<Copied> {
    let src = store::path(ws, &row.hash);
    if !src.exists() {
        return Err(format!("{} is in the index but missing from the store", src.display()).into());
    }
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;

    let stem = file_stem(row);
    let mut dest = dir.join(format!("{stem}.pdf"));
    let mut n = 2;
    while dest.exists() {
        if hash::sha256_file(&dest)? == row.hash {
            return Ok(Copied::AlreadyThere(dest));
        }
        dest = dir.join(format!("{stem}-{n}.pdf"));
        n += 1;
    }
    fs::copy(&src, &dest).map_err(|e| format!("cannot copy to {}: {e}", dest.display()))?;
    Ok(Copied::New(dest))
}

/// `<date>_<summary-slug>`, falling back to tags and then to the hash.
fn file_stem(row: &Row) -> String {
    let date = slugify(&row.date);
    let mut name = without_filler(&slugify(&row.summary));
    if name.is_empty() {
        name = slugify(&row.tags.replace(',', " "));
    }
    if name.is_empty() {
        name = row.hash.chars().take(12).collect();
    }
    if date.is_empty() { name } else { format!("{date}_{name}") }
}

/// Drops filler words from a slug, then cuts it to length at a word boundary.
fn without_filler(slug: &str) -> String {
    let words: Vec<&str> = slug.split('-').filter(|w| !FILLER_WORDS.contains(w)).collect();
    truncate(&words.join("-"))
}

/// Lowercase ASCII with hyphens: umlauts become ae/oe/ue/ss, everything else
/// that is not a letter or digit becomes a hyphen. Cut at a word boundary.
fn slugify(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            'ä' => out.push_str("ae"),
            'ö' => out.push_str("oe"),
            'ü' => out.push_str("ue"),
            'ß' => out.push_str("ss"),
            c if c.is_ascii_alphanumeric() => out.push(c),
            _ if !out.is_empty() && !out.ends_with('-') => out.push('-'),
            _ => {}
        }
    }
    truncate(out.trim_end_matches('-'))
}

/// Cuts a slug to `MAX_SLUG_LEN` at a hyphen.
fn truncate(slug: &str) -> String {
    if slug.len() <= MAX_SLUG_LEN {
        return slug.to_string();
    }
    // Slug is pure ASCII, so byte slicing is safe.
    let cut = &slug[..MAX_SLUG_LEN];
    match cut.rfind('-') {
        Some(i) if i > 0 => cut[..i].to_string(),
        _ => cut.to_string(),
    }
}
