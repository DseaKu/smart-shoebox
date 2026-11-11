//! Cutting the saved page text (see `pagetext`) into chunks: the pieces that are
//! embedded, searched and shown to the model when answering (ROADMAP step 7).
//!
//! One chunk per page, so a source is always "document, page N". A page longer
//! than `MAX_CHUNK_CHARS` is cut at line breaks into parts, and each part starts
//! with the last line(s) of the one before, so a fact on the border is complete
//! in at least one part. Blank pages give no chunk. Chunks are cheap to make, so
//! they are not stored; they are made again from the page text when needed.

use crate::{Res, index, pages, pagetext, search::CHARS_PER_TOKEN, workspace::Workspace};

/// About 400 tokens: small enough that a few chunks fit into the model's context
/// next to the question, large enough to hold a whole section of a letter.
pub const MAX_CHUNK_CHARS: usize = 400 * CHARS_PER_TOKEN;
/// At most this much of the end of a part is repeated at the start of the next.
const OVERLAP_CHARS: usize = 200;

#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    /// The document (its SHA-256, as in the index and the store).
    pub hash: String,
    /// 1-based page number in the stored PDF.
    pub page: u32,
    /// 0-based part of the page; 0 for a page that fits into one chunk.
    pub part: u32,
    /// Cleaned text: runs of spaces collapsed, empty lines dropped.
    pub text: String,
}

impl Chunk {
    /// A stable name for the chunk, e.g. `<hash>:3:0`, to store data with it.
    pub fn id(&self) -> String {
        format!("{}:{}:{}", self.hash, self.page, self.part)
    }
}

/// The chunks of one document, from its page texts.
pub fn of_document(hash: &str, page_texts: &[String]) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    for (text, page) in page_texts.iter().zip(1..) {
        if pages::is_blank(text) {
            continue;
        }
        for (text, part) in split(text, MAX_CHUNK_CHARS).into_iter().zip(0..) {
            chunks.push(Chunk {
                hash: hash.to_string(),
                page,
                part,
                text,
            });
        }
    }
    chunks
}

/// The chunks of every document in the box.
pub struct BoxChunks {
    pub chunks: Vec<Chunk>,
    /// Documents in the index without saved page text (`shoebox text` fixes it).
    pub without_text: Vec<String>,
}

/// Chunks of all documents in the index, in index order.
pub fn of_box(ws: &Workspace) -> Res<BoxChunks> {
    let rows = index::parse(&index::read(&ws.index_path())?);
    let mut chunks = Vec::new();
    let mut without_text = Vec::new();
    for row in rows {
        match pagetext::load(ws, &row.hash)? {
            Some(page_texts) => chunks.extend(of_document(&row.hash, &page_texts)),
            None => without_text.push(row.hash),
        }
    }
    Ok(BoxChunks {
        chunks,
        without_text,
    })
}

/// The page's lines with runs of whitespace collapsed (PDF text has many spaces
/// for the layout, e.g. between a cost item and its amount), without empty lines.
fn clean_lines(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|line| !line.is_empty())
        .collect()
}

fn len(s: &str) -> usize {
    s.chars().count()
}

/// Cuts a page into parts of at most `max` characters, at line breaks (a single
/// line that is too long is cut between words). Each part after the first
/// starts with the last lines of the part before, up to `OVERLAP_CHARS`.
fn split(text: &str, max: usize) -> Vec<String> {
    // Units: lines, and over-long lines cut into word groups.
    let mut units = Vec::new();
    for line in clean_lines(text) {
        if len(&line) <= max {
            units.push(line);
        } else {
            units.extend(word_groups(&line, max));
        }
    }

    let mut parts: Vec<Vec<String>> = Vec::new();
    let mut current: Vec<String> = Vec::new();
    let mut current_len = 0;
    for unit in units {
        // +1 for the line break that joins it to the lines before.
        if !current.is_empty() && current_len + 1 + len(&unit) > max {
            let overlap = tail(&current, OVERLAP_CHARS);
            parts.push(std::mem::take(&mut current));
            current_len = overlap.iter().map(|l| len(l) + 1).sum();
            current = overlap;
            if current_len + len(&unit) > max {
                current.clear();
                current_len = 0;
            }
        }
        current_len += len(&unit) + usize::from(!current.is_empty());
        current.push(unit);
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts.into_iter().map(|lines| lines.join("\n")).collect()
}

/// The last lines of a part, together at most `max` characters (maybe none).
fn tail(lines: &[String], max: usize) -> Vec<String> {
    let mut taken = Vec::new();
    let mut total = 0;
    for line in lines.iter().rev() {
        total += len(line) + 1;
        if total > max {
            break;
        }
        taken.push(line.clone());
    }
    taken.reverse();
    taken
}

/// A line cut into groups of whole words of at most `max` characters
/// (a single word longer than that is cut as it is).
fn word_groups(line: &str, max: usize) -> Vec<String> {
    let mut groups = Vec::new();
    let mut current = String::new();
    for word in line.split(' ') {
        if !current.is_empty() && len(&current) + 1 + len(word) > max {
            groups.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
        while len(&current) > max {
            let cut: String = current.chars().take(max).collect();
            current = current.chars().skip(max).collect();
            groups.push(cut);
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_page_is_one_cleaned_chunk_and_blank_pages_are_left_out() {
        let page_texts = vec![
            "Grundsteuer          312,10 EUR\n\n\n   Müllabfuhr    188,20 EUR  ".to_string(),
            "   \n ".to_string(),
            "Seite 3 von 3 Rechtsbehelfsbelehrung: Einspruch binnen eines Monats".to_string(),
        ];
        let chunks = of_document("abc", &page_texts);
        assert_eq!(chunks.len(), 2);
        assert_eq!(
            chunks[0].text,
            "Grundsteuer 312,10 EUR\nMüllabfuhr 188,20 EUR"
        );
        assert_eq!((chunks[0].page, chunks[0].part), (1, 0));
        assert_eq!(
            (chunks[1].page, chunks[1].part),
            (3, 0),
            "page numbers stay"
        );
        assert_eq!(chunks[1].id(), "abc:3:0");
    }

    #[test]
    fn a_long_page_is_cut_at_lines_with_overlap() {
        let lines: Vec<String> = (1..=40)
            .map(|i| format!("Zeile {i:02} mit etwas Text über die Betriebskosten"))
            .collect();
        let text = lines.join("\n");
        let parts = split(&text, 300);
        assert!(parts.len() > 1);
        for part in &parts {
            assert!(len(part) <= 300, "{} chars: {part}", len(part));
        }
        // Every line is in some part, in order, and nothing is cut inside a line.
        for line in &lines {
            assert!(parts.iter().any(|p| p.lines().any(|l| l == line)), "{line}");
        }
        // Each part repeats the last line of the part before, at its start.
        for pair in parts.windows(2) {
            let last = pair[0].lines().last().unwrap();
            let first_lines: Vec<&str> = pair[1].lines().take(4).collect();
            assert!(first_lines.contains(&last), "{pair:?}");
        }
    }

    #[test]
    fn an_over_long_line_is_cut_between_words() {
        let line = "Wort ".repeat(100); // 500 characters, one line
        let parts = split(&line, 120);
        assert!(parts.len() >= 5);
        for part in &parts {
            assert!(len(part) <= 120);
            assert!(part.split(' ').all(|w| w == "Wort"), "{part}");
        }
        // A single word longer than a part is cut as it is.
        let parts = split(&"x".repeat(250), 100);
        assert_eq!(
            parts.iter().map(|p| len(p)).collect::<Vec<_>>(),
            [100, 100, 50]
        );
    }

    #[test]
    fn umlauts_count_as_one_character() {
        let text = "ä".repeat(100);
        assert_eq!(split(&text, 100), [text.as_str()]);
    }
}
