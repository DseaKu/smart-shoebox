//! Finding the chunks that answer a question (ROADMAP step 7.4): by meaning
//! (vectors, see `vectors`), by words (BM25, here), or both merged (hybrid).
//!
//! Words matter where meaning is vague: exact numbers ("012/345/67890"), years
//! ("2025" in "BK-2025-0381") and rare terms. Meaning matters where the words
//! differ ("Katze" → "Tierhaltung"). The hybrid merges both rankings by rank,
//! not by score (reciprocal rank fusion), so the two scales need no tuning.

use crate::{
    Res,
    chunk::{self, Chunk},
    index::{self, Row},
    search::{STOP_WORDS, normalize},
    vectors::{self, Embedder},
    workspace::Workspace,
};
use std::collections::{HashMap, HashSet};

/// How to rank the chunks.
#[derive(Clone, Copy, Debug, PartialEq, clap::ValueEnum)]
pub enum Method {
    /// By meaning only (embeddings)
    Vector,
    /// By words only (BM25)
    Keyword,
    /// Both, merged by rank
    Hybrid,
}

/// The usual BM25 settings: how fast repeated words stop adding (k1) and how
/// much long chunks are dampened (b).
const K1: f32 = 1.2;
const B: f32 = 0.75;
/// Reciprocal rank fusion: score = sum of 1 / (RRF_K + rank). 60 is the value
/// from the original paper; it keeps one list's top rank from dominating.
const RRF_K: f32 = 60.0;
/// A word also matches a longer word that starts or ends with it, if the shorter
/// one has at least this many letters and this share of the longer one: German
/// endings and compounds ("Mietvertrag" ~ "Mietvertrages", "Warmwasser" ~
/// "Warmwasserkosten", "Erstattung" ~ "Steuererstattung").
const PART_MIN_CHARS: usize = 5;
const PART_MIN_SHARE: f32 = 0.6;
/// Such a partial match counts less than the exact word.
const PART_WEIGHT: f32 = 0.7;

/// Question words that do not say what is asked for, on top of `find`'s list.
const QUESTION_WORDS: &[&str] = &[
    "wie", "hoch", "viel", "viele", "hat", "hatte", "war", "waren", "bei", "im", "in", "an", "auf",
    "laut", "seit", "bis", "lange", "zeit", "kann", "darf", "ab", "es", "gegen", "einer", "einem",
    "how", "much", "many", "when", "does", "did", "do", "have", "of", "for",
];

/// A chunk found for a question, with the method's score (higher is better).
pub struct Hit {
    pub chunk: Chunk,
    pub score: f32,
}

/// The chunks of the box, with the texts that are embedded and searched.
pub struct Searchable {
    pub chunks: Vec<Chunk>,
    pub texts: Vec<String>,
    pub rows: Vec<Row>,
    /// Documents without page text (not searched).
    pub without_text: Vec<String>,
}

impl Searchable {
    pub fn load(ws: &Workspace) -> Res<Self> {
        let rows = index::parse(&index::read(&ws.index_path())?);
        let found = chunk::of_box(ws)?;
        let texts = found.chunks.iter().map(search_text).collect();
        Ok(Self {
            chunks: found.chunks,
            texts,
            rows,
            without_text: found.without_text,
        })
    }

    pub fn row(&self, hash: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.hash == hash)
    }

    fn ids(&self) -> Vec<String> {
        self.chunks.iter().map(Chunk::id).collect()
    }
}

/// The text that is embedded and searched for a chunk: its own text. Putting the
/// document's date and summary in front was measured and made it worse (ROADMAP
/// 7.4): the pages of a document became alike, and the letter date carried the
/// wrong year. Changing this text changes the fingerprints, so the vectors are
/// embedded again.
fn search_text(chunk: &Chunk) -> String {
    chunk.text.clone()
}

/// Words of a text for BM25: lowercase, umlauts written out, punctuation around
/// a word dropped. A word with inner punctuation also gives its parts, so
/// "BK-2025-0381" matches "BK-2025-0381", "2025" and "0381".
fn words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in normalize(text).split_whitespace() {
        let word = raw.trim_matches(|c: char| !c.is_alphanumeric());
        if word.is_empty() {
            continue;
        }
        out.push(word.to_string());
        if word.chars().any(|c| !c.is_alphanumeric()) {
            out.extend(
                word.split(|c: char| !c.is_alphanumeric())
                    .filter(|p| !p.is_empty())
                    .map(str::to_string),
            );
        }
    }
    out
}

/// The question's search words: no stop or question words, no single letters.
fn query_words(question: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    words(question)
        .into_iter()
        .filter(|w| {
            (w.chars().count() >= 2 || w.chars().all(|c| c.is_ascii_digit()))
                && !STOP_WORDS.contains(&w.as_str())
                && !QUESTION_WORDS.contains(&w.as_str())
        })
        .filter(|w| seen.insert(w.clone()))
        .collect()
}

/// A BM25 index over the chunk texts. Built for each search: for a few
/// thousand chunks that takes milliseconds.
struct Bm25 {
    counts: Vec<HashMap<String, u32>>,
    lengths: Vec<f32>,
    average: f32,
    /// In how many chunks each word occurs.
    df: HashMap<String, usize>,
}

impl Bm25 {
    fn new(texts: &[String]) -> Self {
        let mut counts = Vec::with_capacity(texts.len());
        let mut lengths = Vec::with_capacity(texts.len());
        let mut df: HashMap<String, usize> = HashMap::new();
        for text in texts {
            let mut c: HashMap<String, u32> = HashMap::new();
            let ws = words(text);
            lengths.push(ws.len() as f32);
            for w in ws {
                *c.entry(w).or_default() += 1;
            }
            for w in c.keys() {
                *df.entry(w.clone()).or_default() += 1;
            }
            counts.push(c);
        }
        let average = lengths.iter().sum::<f32>() / lengths.len().max(1) as f32;
        Self {
            counts,
            lengths,
            average: average.max(1.0),
            df,
        }
    }

    fn idf(&self, word: &str) -> f32 {
        let n = self.counts.len() as f32;
        let df = *self.df.get(word).unwrap_or(&0) as f32;
        ((n - df + 0.5) / (df + 0.5) + 1.0).ln()
    }

    /// Words of the index that count for a query word, with their weight.
    fn matching(&self, query_word: &str) -> Vec<(&str, f32)> {
        self.df
            .keys()
            .filter_map(|w| {
                if w == query_word {
                    return Some((w.as_str(), 1.0));
                }
                let (short, long) = if w.len() < query_word.len() {
                    (w.as_str(), query_word)
                } else {
                    (query_word, w.as_str())
                };
                let (s, l) = (short.chars().count(), long.chars().count());
                (s >= PART_MIN_CHARS
                    && s as f32 >= PART_MIN_SHARE * l as f32
                    && (long.starts_with(short) || long.ends_with(short)))
                .then_some((w.as_str(), PART_WEIGHT))
            })
            .collect()
    }

    /// All chunks with a score above 0, best first.
    fn rank(&self, question: &str) -> Vec<(usize, f32)> {
        let mut scores = vec![0.0f32; self.counts.len()];
        for q in query_words(question) {
            let matches: Vec<(&str, f32, f32)> = self
                .matching(&q)
                .into_iter()
                .map(|(w, weight)| (w, weight, self.idf(w)))
                .collect();
            for (i, counts) in self.counts.iter().enumerate() {
                // The best matching form of the word counts once per chunk.
                let best = matches
                    .iter()
                    .filter_map(|(w, weight, idf)| {
                        let tf = *counts.get(*w)? as f32;
                        let norm = 1.0 - B + B * self.lengths[i] / self.average;
                        Some(weight * idf * tf * (K1 + 1.0) / (tf + K1 * norm))
                    })
                    .fold(0.0f32, f32::max);
                scores[i] += best;
            }
        }
        let mut ranked: Vec<(usize, f32)> = scores
            .into_iter()
            .enumerate()
            .filter(|(_, s)| *s > 0.0)
            .collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
        ranked
    }
}

/// Merges rankings by rank: each list adds 1 / (RRF_K + rank) to a chunk.
fn fuse(rankings: &[Vec<(usize, f32)>]) -> Vec<(usize, f32)> {
    let mut scores: HashMap<usize, f32> = HashMap::new();
    for ranking in rankings {
        for (rank, (i, _)) in ranking.iter().enumerate() {
            *scores.entry(*i).or_default() += 1.0 / (RRF_K + rank as f32 + 1.0);
        }
    }
    let mut fused: Vec<(usize, f32)> = scores.into_iter().collect();
    fused.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    fused
}

/// The `top` chunks for a question. Updates the vectors first (only new or
/// changed chunks are embedded), unless only words are used.
pub async fn retrieve(
    ws: &Workspace,
    embedder: &impl Embedder,
    searchable: &Searchable,
    question: &str,
    method: Method,
    top: usize,
) -> Res<Vec<Hit>> {
    let by_words = || Bm25::new(&searchable.texts).rank(question);
    let ranking = match method {
        Method::Keyword => by_words(),
        Method::Vector | Method::Hybrid => {
            let (vectors, _) = vectors::update(
                ws,
                embedder,
                &searchable.ids(),
                &searchable.texts,
                &mut print_progress,
            )
            .await?;
            let query = embedder.embed(&[question.to_string()]).await?;
            let by_meaning = vectors.nearest(&query[0], vectors.len());
            if method == Method::Vector {
                by_meaning
            } else {
                fuse(&[by_meaning, by_words()])
            }
        }
    };
    Ok(ranking
        .into_iter()
        .take(top)
        .map(|(i, score)| Hit {
            chunk: searchable.chunks[i].clone(),
            score,
        })
        .collect())
}

/// Prints "Embedding chunks: 16 of 40" while the vectors are updated.
fn print_progress(done: usize, total: usize) {
    eprint!("\rEmbedding chunks: {done} of {total}");
    if done == total {
        eprintln!();
    }
}

fn warn_without_text(hashes: &[String]) {
    if !hashes.is_empty() {
        eprintln!(
            "{} documents have no page text yet and are not searched: run `shoebox text`.",
            hashes.len()
        );
    }
}

/// `shoebox embed`: brings the vectors up to date and says what changed.
pub async fn embed(ws: &Workspace, embedder: &impl Embedder) -> Res<()> {
    let searchable = Searchable::load(ws)?;
    warn_without_text(&searchable.without_text);
    let (vectors, updated) = vectors::update(
        ws,
        embedder,
        &searchable.ids(),
        &searchable.texts,
        &mut print_progress,
    )
    .await?;
    println!(
        "{} chunks with {} ({} dimensions): {} embedded, {} unchanged, {} old vectors dropped.",
        vectors.len(),
        vectors.model,
        vectors.dims,
        updated.embedded,
        updated.kept,
        updated.dropped
    );
    Ok(())
}

/// `shoebox search`: the best chunks for a question, with document, page and
/// score. No answer yet (that is `ask`, ROADMAP 7.5); this shows what is found.
pub async fn search(
    ws: &Workspace,
    embedder: &impl Embedder,
    question: &str,
    method: Method,
    top: usize,
) -> Res<()> {
    let searchable = Searchable::load(ws)?;
    warn_without_text(&searchable.without_text);
    if searchable.chunks.is_empty() {
        return Err("no page text to search yet (add documents, or run `shoebox text`)".into());
    }
    let hits = retrieve(ws, embedder, &searchable, question, method, top).await?;
    if hits.is_empty() {
        println!("No chunk contains a word of the question.");
    }
    for (rank, hit) in hits.iter().enumerate() {
        let chunk = &hit.chunk;
        let name = searchable
            .row(&chunk.hash)
            .map_or("?", |r| r.original_name.as_str());
        let part = if chunk.part > 0 {
            format!(" part {}", chunk.part)
        } else {
            String::new()
        };
        let preview: String = chunk
            .text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(110)
            .collect();
        println!(
            "{:2}. {:.4}  {name} p{}{part}  ({})",
            rank + 1,
            hit.score,
            chunk.page,
            &chunk.hash[..12]
        );
        println!("    {preview}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{FakeEmbedder, FakeModel, TestBox, pdf};

    fn texts(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn words_keep_numbers_and_their_parts() {
        assert_eq!(
            words("Abrechnung BK-2025-0381, Steuernummer: 012/345/67890."),
            [
                "abrechnung",
                "bk-2025-0381",
                "bk",
                "2025",
                "0381",
                "steuernummer",
                "012/345/67890",
                "012",
                "345",
                "67890"
            ]
        );
        assert_eq!(words("Gebühr für Müll"), ["gebuehr", "fuer", "muell"]);
        assert_eq!(
            query_words("Wie hoch ist meine Kaution für 2025?"),
            ["kaution", "2025"]
        );
    }

    #[test]
    fn bm25_ranks_rare_exact_words_first() {
        let index = Bm25::new(&texts(&[
            "Steuerbescheid 2025 Steuernummer 012/345/67890 Erstattung",
            "Mietvertrag Kaution 2.340,00 EUR",
            "Stromrechnung Zähler 1EMH0012345678 Verbrauch",
        ]));
        assert_eq!(index.rank("Welche Steuernummer?")[0].0, 0);
        assert_eq!(index.rank("Zähler 1EMH0012345678")[0].0, 2);
        assert_eq!(index.rank("012/345/67890")[0].0, 0);
        assert!(index.rank("Hausratversicherung").is_empty());
    }

    #[test]
    fn bm25_matches_german_endings_and_compounds() {
        let index = Bm25::new(&texts(&[
            "Heizkosten Warmwasser 164,80 EUR",
            "Mietvertrag Lindenweg",
            "Bruttogehalt 4.050,00 EUR",
        ]));
        assert_eq!(index.rank("Warmwasserkosten")[0].0, 0);
        assert_eq!(index.rank("Mietvertrages")[0].0, 1);
        // The main word of a German compound is at its end.
        let index2 = Bm25::new(&texts(&["Erstattung 875,00 EUR", "Nachzahlung"]));
        assert_eq!(index2.rank("Steuererstattung")[0].0, 0);
        // "brutto…" words with different endings are not the same word.
        assert!(index.rank("Bruttoarbeitslohn").is_empty());
    }

    #[test]
    fn fusion_rewards_chunks_found_by_both() {
        let by_meaning = vec![(1, 0.9), (2, 0.8), (0, 0.1)];
        let by_words = vec![(2, 7.0), (0, 3.0)];
        let fused = fuse(&[by_meaning, by_words]);
        assert_eq!(fused[0].0, 2, "second by meaning, first by words");
        assert_eq!(fused.len(), 3);
    }

    #[tokio::test]
    async fn search_finds_the_page_by_words_and_by_meaning() {
        let t = TestBox::new("retrieve");
        let file = t.dir.join("bescheid.pdf");
        pdf(
            &file,
            &[
                "Seite 1 von 2\nSteuerbescheid 2025\nErstattung 875 EUR",
                "Seite 2 von 2\nSteuernummer 012/345/67890\nEinspruch binnen eines Monats",
            ],
        );
        let model = FakeModel::new(|_, _| {
            r#"{"date": "2026-07-14", "tags": ["steuer"], "summary": "Steuerbescheid 2025"}"#
                .to_string()
        });
        crate::prompt::script(&["y"]);
        crate::add::run(&t.config, &t.ws, &model, &file)
            .await
            .unwrap();

        let embedder = FakeEmbedder::new("fake");
        let searchable = Searchable::load(&t.ws).unwrap();
        for method in [Method::Vector, Method::Keyword, Method::Hybrid] {
            let hits = retrieve(
                &t.ws,
                &embedder,
                &searchable,
                "Steuernummer 012/345/67890",
                method,
                1,
            )
            .await
            .unwrap();
            assert_eq!(hits[0].chunk.page, 2, "{method:?}");
        }
        // Words only: no embedding at all.
        let before = embedder.embedded();
        retrieve(
            &t.ws,
            &embedder,
            &searchable,
            "Erstattung",
            Method::Keyword,
            1,
        )
        .await
        .unwrap();
        assert_eq!(embedder.embedded(), before);
    }
}
