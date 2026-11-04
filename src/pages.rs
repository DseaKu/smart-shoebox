//! Per-page analysis done in code, before the AI sees anything:
//! blank pages, printed page numbers and duplicate scans.

use regex::Regex;
use std::collections::HashSet;
use std::fmt;
use std::sync::LazyLock;

/// Pages with fewer visible characters than this count as blank.
const BLANK_THRESHOLD: usize = 20;
/// Two pages with the same printed number and at least this share of common words
/// are the same page scanned twice (OCR noise makes exact equality unlikely).
const DUPLICATE_SIMILARITY: f64 = 0.9;
/// Excerpts longer than this are shortened to their start and end.
const EXCERPT_HEAD: usize = 450;
const EXCERPT_TAIL: usize = 200;

/// A page number printed on the page, e.g. "Seite 2 von 3".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrintedNumber {
    pub n: u32,
    pub total: Option<u32>,
}

impl fmt::Display for PrintedNumber {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.total {
            Some(total) => write!(f, "{} of {total}", self.n),
            None => write!(f, "{}", self.n),
        }
    }
}

pub struct Page {
    /// 1-based position in the scan.
    pub number: u32,
    pub text: String,
    pub blank: bool,
    pub printed_number: Option<PrintedNumber>,
    /// Set if this page is a second scan of an earlier page.
    pub duplicate_of: Option<u32>,
}

impl Page {
    /// Neither blank nor a duplicate: a page that has to end up in the result.
    pub fn is_content(&self) -> bool {
        !self.blank && self.duplicate_of.is_none()
    }
}

pub fn analyze(texts: Vec<String>) -> Vec<Page> {
    let mut pages: Vec<Page> = texts
        .into_iter()
        .zip(1..)
        .map(|(text, number)| Page {
            number,
            blank: is_blank(&text),
            printed_number: printed_page_number(&text),
            duplicate_of: None,
            text,
        })
        .collect();
    mark_duplicates(&mut pages);
    pages
}

pub fn is_blank(text: &str) -> bool {
    text.chars().filter(|c| !c.is_whitespace()).count() < BLANK_THRESHOLD
}

/// Marks each page that repeats an earlier page: same printed number (or none on
/// both) and nearly the same words. The first scan of a page is kept.
fn mark_duplicates(pages: &mut [Page]) {
    let words: Vec<HashSet<String>> = pages.iter().map(|p| word_set(&p.text)).collect();
    for j in 0..pages.len() {
        if pages[j].blank {
            continue;
        }
        let original = (0..j).find(|&i| {
            pages[i].is_content()
                && pages[i].printed_number.map(|p| p.n) == pages[j].printed_number.map(|p| p.n)
                && similarity(&words[i], &words[j]) >= DUPLICATE_SIMILARITY
        });
        pages[j].duplicate_of = original.map(|i| pages[i].number);
    }
}

fn word_set(text: &str) -> HashSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3)
        .map(str::to_lowercase)
        .collect()
}

/// Share of words two pages have in common (Jaccard index).
fn similarity(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f64 / union as f64
}

/// "N of M" patterns first, since they are the most reliable; then a bare "N".
/// The bool marks patterns that can also appear as a reference in running text.
static PATTERNS: LazyLock<Vec<(Regex, bool)>> = LazyLock::new(|| {
    [
        (r"(?i)\b(?:seite|page|blatt)\s*(\d{1,3})\s*(?:von|of|aus|/)\s*(\d{1,3})\b", false),
        (r"(?m)^\s*(\d{1,3})\s*/\s*(\d{1,3})\s*$", false),
        (r"(?m)^\s*-\s*(\d{1,3})\s*-\s*$", false),
        (r"(?i)\b(?:seite|page)\s*(\d{1,3})\b", true),
    ]
    .into_iter()
    .map(|(p, may_be_reference)| (Regex::new(p).expect("valid regex"), may_be_reference))
    .collect()
});

/// Words that turn "Seite 3" into a reference to another page ("auf der Seite 3",
/// "siehe Seite 3") instead of this page's own number.
const REFERENCE_WORDS: &[&str] = &[
    "der", "die", "den", "auf", "siehe", "vgl", "s", "unter", "ab", "bis", "nach", "zur",
    "folgenden", "nächsten", "naechsten", "letzten", "vorherigen", "see", "on",
];

pub fn printed_page_number(text: &str) -> Option<PrintedNumber> {
    for (re, may_be_reference) in PATTERNS.iter() {
        for caps in re.captures_iter(text) {
            let whole = caps.get(0).expect("match");
            if *may_be_reference && preceded_by_reference_word(text, whole.start()) {
                continue;
            }
            let Ok(n) = caps[1].parse::<u32>() else { continue };
            let total = caps.get(2).and_then(|t| t.as_str().parse::<u32>().ok());
            match total {
                Some(total) if n >= 1 && n <= total => {
                    return Some(PrintedNumber { n, total: Some(total) });
                }
                Some(_) => continue, // e.g. a date like 14/3, not a page number
                None if n >= 1 => return Some(PrintedNumber { n, total: None }),
                None => continue,
            }
        }
    }
    None
}

fn preceded_by_reference_word(text: &str, start: usize) -> bool {
    let word = text[..start]
        .split_whitespace()
        .next_back()
        .unwrap_or("")
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase();
    REFERENCE_WORDS.contains(&word.as_str())
}

/// The page text on one line: all of it if short, otherwise start … end,
/// where letterheads, dates, page numbers and signatures usually are.
pub fn excerpt(text: &str) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let chars: Vec<char> = flat.chars().collect();
    if chars.len() <= EXCERPT_HEAD + EXCERPT_TAIL {
        return flat;
    }
    let head: String = chars[..EXCERPT_HEAD].iter().collect();
    let tail: String = chars[chars.len() - EXCERPT_TAIL..].iter().collect();
    format!("{head} … {tail}")
}

/// The first `max` characters of the page on one line, for the confirmation screen.
pub fn preview(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        format!("{}…", flat.chars().take(max).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(text: &str) -> Option<String> {
        printed_page_number(text).map(|p| p.to_string())
    }

    #[test]
    fn finds_printed_page_numbers() {
        assert_eq!(num("Steuerbescheid\nSeite 2 von 3"), Some("2 of 3".into()));
        assert_eq!(num("Page 1 of 4"), Some("1 of 4".into()));
        assert_eq!(num("Text\n 2 / 5 \nmore"), Some("2 of 5".into()));
        assert_eq!(num("Text\n- 3 -\n"), Some("3".into()));
        assert_eq!(num("Seite 4 Vertragsnummer 3230"), Some("4".into()));
        assert_eq!(num("Datum 14/3\nkein Seitenhinweis"), None);
        assert_eq!(num("Keine Nummer hier"), None);
    }

    #[test]
    fn ignores_references_to_other_pages() {
        assert_eq!(num("Details finden Sie auf der Seite 3."), None);
        assert_eq!(num("siehe Seite 5"), None);
        assert_eq!(num("Wie auf Seite 2 beschrieben. Seite 1"), Some("1".into()));
    }

    #[test]
    fn detects_blank_pages() {
        assert!(is_blank(""));
        assert!(is_blank("  \n\x0c  . , "));
        assert!(!is_blank("Sehr geehrte Damen und Herren, anbei"));
    }

    #[test]
    fn detects_rescanned_pages() {
        let page_two = "Seite 2 Vertragsnummer 3230 Ihre Zahlungen im Abrechnungszeitraum Betrag Datum \
            Januar Februar Maerz April Mai Juni Juli August September Oktober November Dezember \
            Vorauszahlung Nebenkosten Grundsteuer Muellabfuhr Strassenreinigung Hausmeister \
            Gartenpflege Allgemeinstrom Versicherung Wasser Abwasser Heizung Summe Guthaben";
        let pages = analyze(vec![
            page_two.into(),
            "Seite 3 Vertragsnummer 3230 Heizkosten Warmwasser Verteilung nach Verbrauch".into(),
            // Same page again, with a little OCR noise.
            page_two.replace("Datum", "Datvm"),
            // Same words but a different printed number: a different page.
            page_two.replace("Seite 2", "Seite 4"),
        ]);
        assert_eq!(pages[2].duplicate_of, Some(1));
        assert_eq!(pages[3].duplicate_of, None);
        assert!(pages[0].is_content() && !pages[2].is_content());
    }

    #[test]
    fn excerpt_keeps_start_and_end() {
        let long = format!("START {} END", "x ".repeat(1000));
        let e = excerpt(&long);
        assert!(e.starts_with("START") && e.ends_with("END") && e.contains(" … "));
        assert_eq!(excerpt("short\n\ntext"), "short text");
    }
}
