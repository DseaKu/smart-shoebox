//! `ask`: answers a question from the text inside the documents, with sources
//! (ROADMAP step 7.5).
//!
//! The best chunks (hybrid search, see `retrieve`) go to the model, numbered.
//! It answers with a quote for each source. Then plain code checks the answer:
//! - each quote must occur in the excerpt it names (word for word, ignoring
//!   case and spacing); sources whose quote does not are dropped;
//! - each number in the answer must occur in a source that passed (or in the
//!   question), so an amount or date cannot be invented.
//!
//! - a second model call (the verifier) sees only the question, the answer
//!   and the source excerpts, and says whether they really answer the
//!   question. The quote check proves that the words exist, not that they are
//!   about the thing asked for ("Hauptfälligkeit" for "Hauptuntersuchung").
//!
//! If nothing passes, the answer is "not found", never a guess.

use crate::{
    Res,
    chunk::Chunk,
    llm::Model,
    retrieve::{self, Hit, Method, Searchable},
    roles,
    search::normalize,
    store,
    vectors::Embedder,
    workspace::Workspace,
};
use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;
use std::sync::LazyLock;

/// Excerpts sent to the model: in the measurements (ROADMAP 7.4) the right page
/// was always in the top 5; 8 leaves room, and 8 × 1,200 characters fit well
/// into the context next to the question.
pub const EXCERPTS: usize = 8;
/// Shorter quotes prove nothing ("EUR", "2025").
const MIN_QUOTE_CHARS: usize = 4;

/// The answerer's reply.
#[derive(Deserialize, JsonSchema)]
struct Reply {
    found: bool,
    answer: String,
    sources: Vec<Quote>,
}

#[derive(Deserialize, JsonSchema)]
struct Quote {
    /// The excerpt's number, 1-based.
    excerpt: u32,
    quote: String,
}

/// The verifier's reply.
#[derive(Deserialize, JsonSchema)]
struct Verdict {
    supported: bool,
    reason: String,
}

/// A source that passed the check.
#[derive(Debug, PartialEq)]
pub struct Source {
    pub chunk: Chunk,
    pub quote: String,
}

/// The outcome of a question.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Answered {
        answer: String,
        sources: Vec<Source>,
    },
    /// No answer that passed the check. `why` says what went wrong, `claimed`
    /// is what the model said, if anything (shown, but marked as unchecked).
    NotFound {
        why: String,
        claimed: Option<String>,
    },
}

/// The question with the numbered excerpts, as the model sees it.
fn message(question: &str, hits: &[Hit], searchable: &Searchable) -> String {
    let mut text = String::new();
    for (n, hit) in hits.iter().enumerate() {
        let (name, date) = searchable
            .row(&hit.chunk.hash)
            .map_or(("?", "?"), |r| (r.original_name.as_str(), r.date.as_str()));
        text.push_str(&format!(
            "[{}] {name}, page {} ({date})\n{}\n\n",
            n + 1,
            hit.chunk.page,
            hit.chunk.text
        ));
    }
    format!("Excerpts:\n\n{text}Question: {question}")
}

/// Lowercase, umlauts written out, all runs of whitespace as one space, and
/// typographic quotes and dashes as plain ones, so a quote matches its excerpt
/// even if the model changed the spacing or the case.
fn comparable(text: &str) -> String {
    normalize(text)
        .replace(['\u{201e}', '\u{201c}', '\u{201d}'], "\"")
        .replace(['\u{2013}', '\u{2014}'], "-")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

static NUMBER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\d(?:[\d.,/-]*\d)?").expect("valid regex"));

/// The numbers of a text, without separators: "2.340,00 EUR" → "234000",
/// "01.04.2024" → "01042024", so "2.340,00" and "2340,00" are the same.
fn numbers(text: &str) -> Vec<String> {
    NUMBER
        .find_iter(text)
        .map(|m| m.as_str().chars().filter(char::is_ascii_digit).collect())
        .collect()
}

/// A number of the answer is backed if it occurs in a source chunk; written
/// with or without separators, and with or without ",00" cents.
fn is_backed(number: &str, chunks: &[&Chunk]) -> bool {
    let candidates: Vec<String> = chunks.iter().flat_map(|c| numbers(&c.text)).collect();
    candidates.iter().any(|c| {
        c == number
            || c.strip_suffix("00") == Some(number)
            || c.contains(number) && number.len() >= 4
    })
}

/// Checks the reply against the excerpts (see the module comment).
fn check(reply: Reply, hits: &[Hit], question: &str) -> Outcome {
    let claimed = (!reply.answer.trim().is_empty()).then(|| reply.answer.trim().to_string());
    if !reply.found || claimed.is_none() {
        return Outcome::NotFound {
            why: "the excerpts do not contain the answer".into(),
            claimed: None,
        };
    }
    let mut sources: Vec<Source> = Vec::new();
    for q in reply.sources {
        let Some(hit) = (q.excerpt as usize)
            .checked_sub(1)
            .and_then(|i| hits.get(i))
        else {
            continue;
        };
        let quote = q.quote.trim().trim_matches('"').trim();
        if quote.chars().count() < MIN_QUOTE_CHARS
            || !comparable(&hit.chunk.text).contains(&comparable(quote))
        {
            continue;
        }
        if !sources
            .iter()
            .any(|s| s.chunk == hit.chunk && s.quote == quote)
        {
            sources.push(Source {
                chunk: hit.chunk.clone(),
                quote: quote.to_string(),
            });
        }
    }
    if sources.is_empty() {
        return Outcome::NotFound {
            why: "no quote of the answer is in the excerpts it names".into(),
            claimed,
        };
    }
    let answer = claimed.expect("checked above");
    let chunks: Vec<&Chunk> = sources.iter().map(|s| &s.chunk).collect();
    // Numbers from the question are my own words ("Bruttoarbeitslohn 2025"),
    // even if the source page does not repeat them.
    let asked = numbers(question);
    let unbacked: Vec<String> = numbers(&answer)
        .into_iter()
        .filter(|n| !asked.contains(n) && !is_backed(n, &chunks))
        .collect();
    if !unbacked.is_empty() {
        return Outcome::NotFound {
            why: format!(
                "the answer has numbers that are not in its sources ({})",
                unbacked.join(", ")
            ),
            claimed: Some(answer),
        };
    }
    Outcome::Answered { answer, sources }
}

/// The question, the answer and its sources, as the verifier sees them.
fn verify_message(
    question: &str,
    answer: &str,
    sources: &[Source],
    searchable: &Searchable,
) -> String {
    let mut text = String::new();
    for (n, s) in sources.iter().enumerate() {
        let name = searchable
            .row(&s.chunk.hash)
            .map_or("?", |r| r.original_name.as_str());
        text.push_str(&format!(
            "[{}] {name}, page {}\n{}\nQuote: \"{}\"\n\n",
            n + 1,
            s.chunk.page,
            s.chunk.text,
            s.quote
        ));
    }
    format!("Question: {question}\n\nProposed answer: {answer}\n\nExcerpts:\n\n{text}")
}

/// The second check (see the module comment). An answer that passed the code
/// checks becomes "not found" if the verifier rejects it.
async fn verify(
    llm: &impl Model,
    question: &str,
    outcome: Outcome,
    searchable: &Searchable,
) -> Res<Outcome> {
    let Outcome::Answered { answer, sources } = outcome else {
        return Ok(outcome);
    };
    let verdict: Verdict = llm
        .ask_for(
            roles::VERIFIER,
            &verify_message(question, &answer, &sources, searchable),
        )
        .await
        .map_err(|e| format!("the model gave no usable check ({e})"))?;
    if verdict.supported {
        Ok(Outcome::Answered { answer, sources })
    } else {
        Ok(Outcome::NotFound {
            why: format!(
                "the second check says the sources do not answer the question: {}",
                verdict.reason.trim()
            ),
            claimed: Some(answer),
        })
    }
}

/// Searches, asks the model and checks its answer.
pub async fn answer(
    ws: &Workspace,
    llm: &impl Model,
    embedder: &impl Embedder,
    searchable: &Searchable,
    question: &str,
) -> Res<Outcome> {
    let hits =
        retrieve::retrieve(ws, embedder, searchable, question, Method::Hybrid, EXCERPTS).await?;
    if hits.is_empty() {
        return Ok(Outcome::NotFound {
            why: "no page matches the question".into(),
            claimed: None,
        });
    }
    let reply: Reply = llm
        .ask_for(roles::ANSWERER, &message(question, &hits, searchable))
        .await
        .map_err(|e| format!("the model gave no usable answer ({e})"))?;
    verify(llm, question, check(reply, &hits, question), searchable).await
}

/// `shoebox ask`: prints the answer and its sources, or "Not found".
pub async fn run(
    ws: &Workspace,
    llm: &impl Model,
    embedder: &impl Embedder,
    question: &str,
) -> Res<()> {
    let searchable = Searchable::load(ws)?;
    if !searchable.without_text.is_empty() {
        eprintln!(
            "{} documents have no page text yet and are not searched: run `shoebox text`.",
            searchable.without_text.len()
        );
    }
    if searchable.chunks.is_empty() {
        return Err("no page text to search yet (add documents, or run `shoebox text`)".into());
    }
    eprintln!("Asking {}…", llm.name());
    match answer(ws, llm, embedder, &searchable, question).await? {
        Outcome::Answered { answer, sources } => {
            println!("{answer}");
            println!("\nSources:");
            for s in &sources {
                let name = searchable
                    .row(&s.chunk.hash)
                    .map_or("?", |r| r.original_name.as_str());
                println!(
                    "  - {name} p{} ({}): \"{}\"",
                    s.chunk.page,
                    &s.chunk.hash[..12],
                    s.quote
                );
                println!("    {}", store::path(ws, &s.chunk.hash).display());
            }
        }
        Outcome::NotFound { why, claimed } => {
            println!("Not found in your documents.");
            eprintln!("  ({why})");
            if let Some(claimed) = claimed {
                eprintln!("  The model said, unchecked: {claimed}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(page: u32, text: &str) -> Hit {
        Hit {
            chunk: Chunk {
                hash: "abc".into(),
                page,
                part: 0,
                text: text.into(),
            },
            score: 1.0,
        }
    }

    fn reply(found: bool, answer: &str, sources: &[(u32, &str)]) -> Reply {
        Reply {
            found,
            answer: answer.into(),
            sources: sources
                .iter()
                .map(|(excerpt, quote)| Quote {
                    excerpt: *excerpt,
                    quote: quote.to_string(),
                })
                .collect(),
        }
    }

    fn hits() -> Vec<Hit> {
        vec![
            hit(1, "Steuerbescheid 2025\nErstattung 875,00 EUR"),
            hit(
                2,
                "Mietvertrag\nDie Kaution beträgt 2.340,00 EUR\nab 01.04.2024",
            ),
        ]
    }

    #[test]
    fn a_quoted_answer_passes_even_with_other_spacing_and_case() {
        let outcome = check(
            reply(
                true,
                "Die Kaution beträgt 2.340 EUR.",
                &[(2, "die  Kaution beträgt 2.340,00 EUR")],
            ),
            &hits(),
            "",
        );
        let Outcome::Answered { sources, .. } = outcome else {
            panic!("{outcome:?}")
        };
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].chunk.page, 2);
    }

    #[test]
    fn a_quote_that_is_not_in_its_excerpt_is_not_an_answer() {
        // The quote is real, but from excerpt 2, not 1.
        let outcome = check(
            reply(true, "2.340,00 EUR", &[(1, "Kaution beträgt 2.340,00 EUR")]),
            &hits(),
            "",
        );
        assert!(matches!(
            outcome,
            Outcome::NotFound {
                claimed: Some(_),
                ..
            }
        ));
        // An excerpt number that was never sent.
        let outcome = check(
            reply(true, "875 EUR", &[(9, "Erstattung 875,00 EUR")]),
            &hits(),
            "",
        );
        assert!(matches!(outcome, Outcome::NotFound { .. }));
        // Too short to prove anything.
        let outcome = check(reply(true, "875 EUR", &[(1, "EUR")]), &hits(), "");
        assert!(matches!(outcome, Outcome::NotFound { .. }));
    }

    #[test]
    fn an_invented_number_is_caught() {
        let outcome = check(
            reply(
                true,
                "Die Erstattung beträgt 785,00 EUR.",
                &[(1, "Erstattung 875,00 EUR")],
            ),
            &hits(),
            "",
        );
        let Outcome::NotFound { why, .. } = outcome else {
            panic!("{outcome:?}")
        };
        assert!(why.contains("78500"), "{why}");
        // The same number in another form is fine: 875 for 875,00; a date.
        let ok = check(
            reply(
                true,
                "875 EUR, Mietbeginn 01.04.2024",
                &[(1, "Erstattung 875,00 EUR"), (2, "ab 01.04.2024")],
            ),
            &hits(),
            "",
        );
        assert!(matches!(ok, Outcome::Answered { .. }), "{ok:?}");
        // A number from the question may be repeated, even if the page lacks it.
        let r = || {
            reply(
                true,
                "Die Erstattung 2025 betrug 875 EUR.",
                &[(1, "Erstattung 875,00 EUR")],
            )
        };
        let hits = vec![hit(1, "Erstattung 875,00 EUR")];
        assert!(matches!(
            check(r(), &hits, "Erstattung 2025?"),
            Outcome::Answered { .. }
        ));
        assert!(matches!(
            check(r(), &hits, "Erstattung?"),
            Outcome::NotFound { .. }
        ));
    }

    #[test]
    fn found_false_means_not_found() {
        let outcome = check(reply(false, "", &[]), &hits(), "");
        assert!(matches!(outcome, Outcome::NotFound { claimed: None, .. }));
    }
}
