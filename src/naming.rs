//! Readable file names like `2026-03-14_steuerbescheid-2025-finanzamt-muenchen.pdf`,
//! shared by `find --to` and `name`.

use std::path::{Path, PathBuf};

const MAX_SLUG_LEN: usize = 60;

/// Common German and English filler words left out of file names.
const FILLER_WORDS: &[&str] = &[
    "von", "vom", "mit", "fuer", "und", "der", "die", "das", "den", "dem", "des", "zur", "zum",
    "im", "in", "am", "an", "auf", "bei", "ein", "eine", "einer", "einem", "einen", "ueber",
    "the", "of", "and", "for", "with", "to", "a",
];

/// `<date>_<slug>` from the first description that gives a non-empty slug.
/// The date is only used if it is a real `YYYY-MM-DD`. `None` if no description works.
pub fn stem(date: &str, descriptions: &[&str]) -> Option<String> {
    let name = descriptions
        .iter()
        .map(|d| without_filler(&slugify(d)))
        .find(|slug| !slug.is_empty())?;
    Some(if is_iso_date(date.trim()) {
        format!("{}_{name}", date.trim())
    } else {
        name
    })
}

/// `dir/<stem>.pdf`, or `<stem>-2.pdf`, `-3`, … if that name is taken.
pub fn free_path(dir: &Path, stem: &str) -> PathBuf {
    let mut path = dir.join(format!("{stem}.pdf"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}-{n}.pdf"));
        n += 1;
    }
    path
}

fn is_iso_date(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
}

/// Drops filler words from a slug, then cuts it to length at a word boundary.
fn without_filler(slug: &str) -> String {
    let words: Vec<&str> = slug.split('-').filter(|w| !FILLER_WORDS.contains(w)).collect();
    truncate(&words.join("-"))
}

/// Lowercase ASCII with hyphens: umlauts become ae/oe/ue/ss, everything else
/// that is not a letter or digit becomes a hyphen. Cut at a word boundary.
pub fn slugify(text: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_readable_names() {
        assert_eq!(
            stem("2026-03-14", &["Steuerbescheid 2025 vom Finanzamt München"]).unwrap(),
            "2026-03-14_steuerbescheid-2025-finanzamt-muenchen"
        );
        // An invalid date is left out instead of ending up in the name.
        assert_eq!(stem("unknown", &["Mietvertrag LEG"]).unwrap(), "mietvertrag-leg");
        // Falls back to the next description, e.g. the tags.
        assert_eq!(stem("", &["€ !", "steuer finanzamt"]).unwrap(), "steuer-finanzamt");
        assert_eq!(stem("2026-03-14", &["", "--"]), None);
    }

    #[test]
    fn keeps_names_short_at_word_boundaries() {
        let long = "Beitragsanpassung Krankenversicherung Allianz Versicherungs AG zum ersten Januar";
        let s = stem("", &[long]).unwrap();
        assert!(s.len() <= MAX_SLUG_LEN && !s.ends_with('-'), "{s}");
    }
}
