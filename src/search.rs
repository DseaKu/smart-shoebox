//! Choosing which index rows the finder sees, and reading its answer.
//!
//! The model's context is limited (`context_size` in `shoebox.yaml`). As long as
//! the whole index fits, every row is sent, so the AI can match by meaning
//! ("rental contract" also finds "Mietvertrag Wohnung"). When it does not fit,
//! rows that share words or years with the question come first, then the newest.

use crate::index::Row;
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::HashSet;

/// Rough size of German text with numbers; on the safe side.
pub const CHARS_PER_TOKEN: usize = 3;
/// Room for the role, the question and the answer.
const RESERVED_TOKENS: usize = 800;
/// When rows share words with the question, only this many are sent: small
/// models miss the right row in a long list even when it is there.
const MAX_ROWS_WITH_HITS: usize = 60;

/// Words that say nothing about which document is meant.
const STOP_WORDS: &[&str] = &[
    "wo",
    "ist",
    "sind",
    "mein",
    "meine",
    "meinen",
    "meiner",
    "meines",
    "der",
    "die",
    "das",
    "den",
    "dem",
    "des",
    "und",
    "oder",
    "ich",
    "habe",
    "hab",
    "von",
    "vom",
    "fuer",
    "mit",
    "ein",
    "eine",
    "einen",
    "wann",
    "was",
    "welche",
    "welcher",
    "welches",
    "zum",
    "zur",
    "brauche",
    "suche",
    "finde",
    "dokument",
    "dokumente",
    "where",
    "what",
    "which",
    "my",
    "the",
    "and",
    "find",
    "document",
    "documents",
    "is",
    "are",
];

/// The finder's answer: numbers of the matching rows, best first.
#[derive(Deserialize, JsonSchema)]
pub struct Answer {
    pub matches: Vec<u32>,
}

pub struct Selection<'a> {
    /// `(number, row)`, where `number` is the row's 1-based position in the index.
    pub rows: Vec<(u32, &'a Row)>,
    /// How many rows were not sent.
    pub left_out: usize,
    /// True if rows were left out because others matched the question better,
    /// false if they did not fit into the context.
    pub focused: bool,
}

impl<'a> Selection<'a> {
    /// The rows as the model sees them, one per line.
    pub fn prompt_lines(&self) -> String {
        self.rows
            .iter()
            .map(|(n, row)| line(*n, row))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Maps the model's numbers back to rows. Numbers that were not sent are
    /// ignored, so the model cannot point to anything outside the selection.
    pub fn resolve(&self, answer: &Answer) -> Vec<&'a Row> {
        let mut seen = HashSet::new();
        answer
            .matches
            .iter()
            .filter(|n| seen.insert(**n))
            .filter_map(|n| self.rows.iter().find(|(m, _)| m == n).map(|(_, row)| *row))
            .collect()
    }
}

/// Picks the rows to send: best matches for the question first, then the newest.
/// If any row matches, at most `MAX_ROWS_WITH_HITS`; otherwise as many as fit
/// into `context_size`. They are shown to the model in index order.
pub fn select<'a>(rows: &'a [Row], question: &str, context_size: u64) -> Selection<'a> {
    let budget = (context_size as usize).saturating_sub(RESERVED_TOKENS) * CHARS_PER_TOKEN;
    let terms = terms(question);
    let mut ranked: Vec<(u32, &Row, usize)> = rows
        .iter()
        .zip(1..)
        .map(|(row, n)| (n, row, score(row, &terms)))
        .collect();
    ranked.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| b.1.date.cmp(&a.1.date)));
    let has_hits = ranked.first().is_some_and(|r| r.2 > 0);
    let max_rows = if has_hits {
        MAX_ROWS_WITH_HITS
    } else {
        usize::MAX
    };

    let mut used = 0;
    let mut chosen = Vec::new();
    let mut out_of_room = false;
    for (n, row, _) in ranked {
        let len = line(n, row).chars().count() + 1;
        if used + len > budget {
            out_of_room = true;
            break;
        }
        if chosen.len() == max_rows {
            break;
        }
        used += len;
        chosen.push((n, row));
    }
    chosen.sort_by_key(|(n, _)| *n);
    Selection {
        left_out: rows.len() - chosen.len(),
        focused: !out_of_room,
        rows: chosen,
    }
}

fn line(n: u32, row: &Row) -> String {
    format!(
        "#{n} | {} | {} | {} | {}",
        row.date, row.tags, row.summary, row.original_name
    )
}

/// Search words from the question: lowercase, umlauts written out, no stop
/// words; numbers only if they look like a year.
fn terms(question: &str) -> Vec<String> {
    normalize(question)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| {
            if w.chars().all(|c| c.is_ascii_digit()) {
                w.len() == 4
            } else {
                w.chars().count() >= 3 && !STOP_WORDS.contains(w)
            }
        })
        .map(str::to_string)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect()
}

/// How many search words occur in the row. Longer words also match by their
/// start (up to 4 letters shorter, at least 5), so German plurals and endings
/// still match: "mietvertraege" finds "mietvertrag", "rechnungen" finds "rechnung".
fn score(row: &Row, terms: &[String]) -> usize {
    let text = normalize(&format!(
        "{} {} {} {}",
        row.date, row.tags, row.summary, row.original_name
    ));
    terms
        .iter()
        .filter(|t| {
            let chars: Vec<char> = t.chars().collect();
            let shortest = chars.len().saturating_sub(4).max(5).min(chars.len());
            (shortest..=chars.len())
                .rev()
                .any(|n| text.contains(&chars[..n].iter().collect::<String>()))
        })
        .count()
}

fn normalize(text: &str) -> String {
    text.to_lowercase()
        .replace('ä', "ae")
        .replace('ö', "oe")
        .replace('ü', "ue")
        .replace('ß', "ss")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(i: usize, date: &str, tags: &str, summary: &str) -> Row {
        Row {
            hash: format!("{i:064x}"),
            date: date.into(),
            tags: tags.into(),
            summary: summary.into(),
            original_name: format!("scan_{i}.pdf"),
        }
    }

    #[test]
    fn small_index_is_sent_completely() {
        let rows = vec![
            row(
                1,
                "2025-11-02",
                "wohnung,vertrag",
                "Mietvertrag Wohnung LEG",
            ),
            row(2, "2026-03-14", "steuer", "Steuerbescheid 2025"),
        ];
        let selection = select(&rows, "Wo ist mein rental contract?", 16384);
        assert_eq!(selection.left_out, 0);
        assert_eq!(selection.rows.len(), 2);
        assert!(
            selection
                .prompt_lines()
                .starts_with("#1 | 2025-11-02 | wohnung,vertrag")
        );
    }

    #[test]
    fn large_index_keeps_the_best_matches() {
        let mut rows: Vec<Row> = (0..2000)
            .map(|i| {
                row(
                    i,
                    "2024-01-01",
                    "rechnung",
                    "Rechnung Baumarkt Material und Werkzeug",
                )
            })
            .collect();
        rows.push(row(
            2000,
            "2019-05-01",
            "wohnung,vertrag",
            "Mietvertrag Wohnung LEG, unbefristet",
        ));
        let selection = select(&rows, "Wo sind meine Mietverträge?", 16384);
        assert!(
            selection.rows.iter().any(|(n, _)| *n == 2001),
            "the matching row must be sent"
        );
        assert_eq!(selection.rows.len(), MAX_ROWS_WITH_HITS);
        assert!(selection.focused);

        // Without any word match, as many rows as fit are sent instead.
        let selection = select(&rows, "Wo ist mein rental contract?", 16384);
        assert!(selection.rows.len() > MAX_ROWS_WITH_HITS && selection.left_out > 0);
        assert!(!selection.focused);
    }

    #[test]
    fn word_endings_still_match() {
        let r = row(
            1,
            "2025-01-01",
            "rechnung",
            "Mietvertrag Wohnung, Rechnung Zahnarzt",
        );
        assert_eq!(score(&r, &terms("Mietverträge")), 1);
        assert_eq!(score(&r, &terms("Rechnungen")), 1);
        assert_eq!(score(&r, &terms("Versicherung")), 0);
    }

    #[test]
    fn years_and_stop_words() {
        let mut t = terms("Wo ist mein Steuerbescheid von 2025?");
        t.sort();
        assert_eq!(t, ["2025", "steuerbescheid"]);
    }

    #[test]
    fn answer_numbers_map_back_to_sent_rows_only() {
        let rows = [
            row(1, "2025-01-01", "a", "Eins"),
            row(2, "2025-01-02", "b", "Zwei"),
        ];
        let selection = Selection {
            rows: vec![(2, &rows[1])],
            left_out: 1,
            focused: true,
        };
        let answer: Answer = crate::llm::parse_json(r#"{"matches": [2, 1, 2, 99]}"#).unwrap();
        let found = selection.resolve(&answer);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].summary, "Zwei");
        assert!(crate::llm::parse_json::<Answer>("none").is_err());
    }
}
