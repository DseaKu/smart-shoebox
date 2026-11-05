//! `check`: is a scanned document complete, judging by its printed page numbers?
//! `counterpart`: find the missing pages in another PDF, then merge and sort.
//!
//! Both are plain code, no AI: printed page numbers and shared reference numbers
//! (contract numbers, customer numbers, dates) are reliable and fast to compare.

use crate::{
    Res,
    config::Config,
    hash, index,
    llm::Model,
    naming,
    pages::{Page, join_numbers},
    pdf::TempFile,
    pdfdoc, prompt, rename,
    scan::Scan,
    sort, store, trash,
    workspace::Workspace,
};
use regex::Regex;
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// A counterpart must share at least this many reference numbers or dates.
pub const MIN_SHARED: usize = 2;
/// Pages with a number the incomplete scan already has: one is forgiven (a misread
/// number), from this many on the file is another copy or another document.
const MAX_CLASHES: usize = 2;

/// What the printed page numbers say about a scan.
pub struct Report {
    /// Printed page numbers found on content pages, sorted.
    pub numbers: Vec<u32>,
    /// Highest page the document should have ("von N", or the highest number seen).
    pub last: u32,
    /// Pages between 1 and `last` that are not in the scan. Pages without a printed
    /// number are assumed to fill the lowest gaps (usually page 1 with the letterhead).
    pub missing: Vec<u32>,
    /// Printed numbers found on more than one page, with those pages (scan order).
    /// Several documents in one scan, or a misread number; either way the gaps
    /// are still worth a counterpart search.
    pub repeated: Vec<(u32, Vec<u32>)>,
}

impl Report {
    pub fn is_incomplete(&self) -> bool {
        !self.missing.is_empty()
    }
}

pub fn completeness(pages: &[Page]) -> Report {
    let content: Vec<&Page> = pages.iter().filter(|p| p.is_content()).collect();
    let found: Vec<u32> = content
        .iter()
        .filter_map(|p| p.printed_number.map(|x| x.n))
        .collect();
    let numbers: BTreeSet<u32> = found.iter().copied().collect();
    let last = content
        .iter()
        .filter_map(|p| p.printed_number)
        .map(|x| x.total.unwrap_or(x.n).max(x.n))
        .max()
        .unwrap_or(0);
    let unnumbered = content
        .iter()
        .filter(|p| p.printed_number.is_none())
        .count();
    let missing = (1..=last)
        .filter(|n| !numbers.contains(n))
        .skip(unnumbered)
        .collect();
    let repeated = numbers
        .iter()
        .map(|&n| {
            let on: Vec<u32> = content
                .iter()
                .filter(|p| p.printed_number.map(|x| x.n) == Some(n))
                .map(|p| p.number)
                .collect();
            (n, on)
        })
        .filter(|(_, on)| on.len() > 1)
        .collect();
    Report {
        repeated,
        numbers: numbers.into_iter().collect(),
        last,
        missing,
    }
}

/// `check <file>`: a general health check. Reports pages, the file name and
/// whether it is in the box; then offers the fixes: first the counterpart
/// (if pages are missing), then a readable name (for the result, if one was made).
pub async fn check(
    config: &Config,
    ws: &Workspace,
    llm: &impl Model,
    file: &Path,
    search_in: Option<&Path>,
) -> Res<()> {
    let diagnosis = diagnose(config, ws, file)?;
    for line in &diagnosis.lines {
        println!("{line}");
    }
    println!();
    let (scan, report) = (&diagnosis.scan, &diagnosis.report);

    let mut current = file.to_path_buf();
    if report.is_incomplete()
        && prompt::ask_yes_no("Search for the counterpart? [y]es / [n]o: ")?
        && let Some(result) = find_counterpart(config, llm, file, scan, report, search_in).await?
    {
        current = result;
    }
    if current.is_file()
        && !naming::is_readable(&file_stem(&current))
        && prompt::ask_yes_no(&format!(
            "Give {} a readable name? [y]es / [n]o: ",
            current.display()
        ))?
    {
        // The original was read (and OCR'd) above already; a merged result is searchable.
        let text = (current == file).then(|| diagnosis.text());
        rename::name_file(config, ws, llm, &current, text.as_deref()).await?;
    }
    Ok(())
}

/// The result of a check, before any fix.
pub struct Diagnosis {
    pub scan: Scan,
    pub report: Report,
    /// The report as I read it: pages, name, box.
    pub lines: Vec<String>,
    pub readable_name: bool,
    pub in_box: bool,
}

impl Diagnosis {
    /// The text of all pages, e.g. for naming without a second OCR.
    pub fn text(&self) -> String {
        self.scan
            .pages
            .iter()
            .map(|p| p.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Reads the scan (OCR if needed) and checks pages, name and box. No AI.
pub fn diagnose(config: &Config, ws: &Workspace, file: &Path) -> Res<Diagnosis> {
    let scan = Scan::read(file, &config.ocr_language)?;
    let report = completeness(&scan.pages);
    let mut lines = report_lines(file, &scan.pages, &report);

    let stem = file_stem(file);
    let readable_name = naming::is_readable(&stem);
    if readable_name {
        lines.push("  name: ok".into());
    } else {
        lines.push(format!(
            "  name: \"{stem}\" is not readable (like 2026-03-14_steuerbescheid-2025-finanzamt-muenchen)"
        ));
    }
    let hash = hash::sha256_file(file)?;
    let in_box = index::contains(&index::read(&ws.index_path())?, &hash);
    if in_box {
        lines.push(format!(
            "  box: already added ({})",
            store::path(ws, &hash).display()
        ));
    } else {
        lines.push("  box: not added yet".into());
    }
    Ok(Diagnosis {
        scan,
        report,
        lines,
        readable_name,
        in_box,
    })
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `counterpart <file>`: looks for the missing pages right away.
pub async fn counterpart(
    config: &Config,
    llm: &impl Model,
    file: &Path,
    search_in: Option<&Path>,
) -> Res<()> {
    let scan = Scan::read(file, &config.ocr_language)?;
    let report = completeness(&scan.pages);
    if !report.is_incomplete() {
        for line in report_lines(file, &scan.pages, &report) {
            println!("{line}");
        }
        println!("Nothing to look for.");
        return Ok(());
    }
    println!(
        "{}: printed page {} missing",
        file.display(),
        join_numbers(&report.missing)
    );
    find_counterpart(config, llm, file, &scan, &report, search_in)
        .await
        .map(|_| ())
}

fn report_lines(file: &Path, pages: &[Page], report: &Report) -> Vec<String> {
    let mut lines = Vec::new();
    let plural = if pages.len() == 1 { "" } else { "s" };
    lines.push(format!("{}: {} page{plural}", file.display(), pages.len()));
    if report.numbers.is_empty() {
        lines
            .push("  no printed page numbers found, so completeness cannot be checked".to_string());
        return lines;
    }
    lines.push(format!(
        "  printed page numbers: {}",
        join_numbers(&report.numbers)
    ));
    let blank: Vec<u32> = pages.iter().filter(|p| p.blank).map(|p| p.number).collect();
    if !blank.is_empty() {
        lines.push(format!("  blank pages: {}", join_numbers(&blank)));
    }
    let duplicates: Vec<String> = pages
        .iter()
        .filter_map(|p| {
            p.duplicate_of
                .map(|o| format!("{} (same as {o})", p.number))
        })
        .collect();
    if !duplicates.is_empty() {
        lines.push(format!("  duplicate scans: {}", duplicates.join(", ")));
    }
    for (n, on) in &report.repeated {
        lines.push(format!(
            "  printed page {n} appears on pages {}: several documents, or a misread number",
            join_numbers(on)
        ));
    }
    if report.missing.is_empty() {
        lines.push(format!("  complete: pages 1–{}", report.last));
        lines.push(format!(
            "  (pages after {} would not be noticed if no page says \"von N\")",
            report.last
        ));
    } else {
        lines.push(format!(
            "  incomplete: printed page {} not found",
            join_numbers(&report.missing)
        ));
    }
    lines
}

/// A PDF that holds missing pages of the same document.
pub struct Candidate {
    pub path: PathBuf,
    pub scan: Scan,
    /// Missing page numbers it contains.
    pub fills: Vec<u32>,
    /// Reference numbers and dates it shares with the incomplete scan.
    pub shared: usize,
    /// How many of them are distinctive: not in most PDFs of the folder (letters
    /// from the same sender share contract number, IBAN, postcode). Used to rank.
    pub distinctive: usize,
    /// A few shared numbers to show, distinctive ones first.
    pub examples: Vec<String>,
    /// Pages that would still be missing after merging with it.
    pub still: Vec<u32>,
}

/// The result of a counterpart search: the suggestions, and why every other
/// PDF of the folder was not suggested.
#[derive(Default)]
pub struct Search {
    pub candidates: Vec<Candidate>,
    pub rejected: Vec<(PathBuf, String)>,
}

/// Where to look for a counterpart: `search_in`, else the file's folder.
pub fn search_dir(file: &Path, search_in: Option<&Path>) -> PathBuf {
    match search_in {
        Some(dir) => dir.to_path_buf(),
        None => match file.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        },
    }
}

/// Reads every other PDF in `dir` and returns the likely counterparts, best first,
/// and the others with the reason. A counterpart must
/// - contain at least one missing page number,
/// - not contain the scan's own page numbers (from `MAX_CLASHES` on, it is another
///   copy or another document) and not a different "von N" total,
/// - share at least `MIN_SHARED` reference numbers or dates.
pub fn search_counterparts(
    config: &Config,
    file: &Path,
    scan: &Scan,
    report: &Report,
    dir: &Path,
) -> Res<Search> {
    let paths = candidate_paths(file, dir)?;
    if paths.is_empty() {
        return Ok(Search::default());
    }
    let plural = if paths.len() == 1 { "" } else { "s" };
    eprintln!(
        "Looking through {} PDF{plural} in {}…",
        paths.len(),
        dir.display()
    );
    let mut others = Vec::new();
    let mut unreadable = Vec::new();
    for path in paths {
        match Scan::read(&path, &config.ocr_language) {
            Ok(other) => others.push((path, other)),
            Err(e) => unreadable.push((path, format!("cannot be read: {e}"))),
        }
    }
    let mut search = rank_counterparts(scan, report, others);
    search.rejected.extend(unreadable);
    Ok(search)
}

/// The decision itself, without reading files (see `search_counterparts`).
fn rank_counterparts(scan: &Scan, report: &Report, others: Vec<(PathBuf, Scan)>) -> Search {
    let own_ids = identifiers(&scan.pages);
    let other_ids: Vec<HashSet<String>> =
        others.iter().map(|(_, s)| identifiers(&s.pages)).collect();
    // Common: in more than half of the PDFs (this scan included), and in at least 3.
    let files = others.len() + 1;
    let common_from = (files / 2 + 2).max(3);
    let is_distinctive =
        |id: &String| 1 + other_ids.iter().filter(|ids| ids.contains(id)).count() < common_from;
    let own_totals = totals(&scan.pages);

    let mut candidates = Vec::new();
    let mut rejected = Vec::new();
    for ((path, other), ids) in others.into_iter().zip(&other_ids) {
        let numbered = || {
            other
                .pages
                .iter()
                .filter(|p| p.is_content())
                .filter_map(|p| p.printed_number.map(|x| x.n))
        };
        let fills: Vec<u32> = report
            .missing
            .iter()
            .copied()
            .filter(|n| numbered().any(|x| x == *n))
            .collect();
        let mut clashes: Vec<u32> = numbered().filter(|n| report.numbers.contains(n)).collect();
        let other_totals = totals(&other.pages);
        let different_total = !own_totals.is_empty()
            && !other_totals.is_empty()
            && own_totals.is_disjoint(&other_totals);
        let mut shared: Vec<String> = own_ids.intersection(ids).cloned().collect();

        // Every rule that fails, so I can see why a file was not suggested.
        let mut reasons = Vec::new();
        if fills.is_empty() {
            let mut its: Vec<u32> = numbered().collect();
            its.sort_unstable();
            its.dedup();
            reasons.push(if its.is_empty() {
                "no printed page numbers".to_string()
            } else {
                format!(
                    "none of the missing pages (it has page {})",
                    join_numbers(&its)
                )
            });
        }
        if clashes.len() >= MAX_CLASHES {
            clashes.sort_unstable();
            clashes.dedup();
            reasons.push(format!(
                "has this scan's own page {} (another copy or another letter)",
                join_numbers(&clashes)
            ));
        }
        if different_total {
            let mut theirs: Vec<u32> = other_totals.iter().copied().collect();
            let mut ours: Vec<u32> = own_totals.iter().copied().collect();
            theirs.sort_unstable();
            ours.sort_unstable();
            reasons.push(format!(
                "says \"von {}\", this scan \"von {}\"",
                join_numbers(&theirs),
                join_numbers(&ours)
            ));
        }
        if shared.len() < MIN_SHARED {
            reasons.push(format!(
                "shares {} reference numbers/dates, needs {MIN_SHARED}",
                shared.len()
            ));
        }
        if !reasons.is_empty() {
            rejected.push((path, reasons.join("; ")));
            continue;
        }

        // Distinctive first, then longest: contract and invoice numbers say more
        // than short dates.
        shared.sort_by_key(|id| (!is_distinctive(id), std::cmp::Reverse(id.len()), id.clone()));
        let distinctive = shared.iter().filter(|id| is_distinctive(id)).count();
        // Like in `sort`, the counterpart's unnumbered pages (often page 1 with the
        // letterhead) are assumed to fill the lowest remaining gaps.
        let unnumbered = other
            .pages
            .iter()
            .filter(|p| p.is_content() && p.printed_number.is_none())
            .count();
        let still = report
            .missing
            .iter()
            .copied()
            .filter(|n| !fills.contains(n))
            .skip(unnumbered)
            .collect();
        candidates.push(Candidate {
            path,
            scan: other,
            fills,
            shared: shared.len(),
            distinctive,
            examples: shared.into_iter().take(3).collect(),
            still,
        });
    }
    // Best first: fills the most missing pages, then shares the most numbers that
    // set it apart, then the most numbers at all.
    candidates.sort_by_key(|c| std::cmp::Reverse((c.fills.len(), c.distinctive, c.shared)));
    rejected.sort();
    Search {
        candidates,
        rejected,
    }
}

/// The "von N" totals printed on the pages.
fn totals(pages: &[Page]) -> HashSet<u32> {
    pages
        .iter()
        .filter(|p| p.is_content())
        .filter_map(|p| p.printed_number.and_then(|x| x.total))
        .collect()
}

/// How a candidate is described to me, e.g. "has missing page 4, 6, shares 12
/// reference numbers/dates, 3 of them only with this file (29.10.2025, …)".
pub fn describe(candidate: &Candidate) -> String {
    let more = if candidate.shared > candidate.examples.len() {
        ", …"
    } else {
        ""
    };
    let distinctive = if candidate.distinctive < candidate.shared {
        format!(", {} of them not in most other PDFs", candidate.distinctive)
    } else {
        String::new()
    };
    format!(
        "has missing page {}, shares {} reference numbers/dates{distinctive} ({}{more})",
        join_numbers(&candidate.fills),
        candidate.shared,
        candidate.examples.join(", ")
    )
}

/// Merges the searchable versions of both scans into a temporary PDF (so sorting
/// needs no second OCR). Returns it and where the sorted result should go:
/// `<file>_complete.pdf`.
pub fn merge_with(file: &Path, scan: &Scan, candidate: &Candidate) -> Res<(TempFile, PathBuf)> {
    let merged = TempFile::new("counterpart-merged");
    pdfdoc::merge(
        &[
            scan.source().to_path_buf(),
            candidate.scan.source().to_path_buf(),
        ],
        merged.path(),
    )?;
    let out = pdfdoc::output_path(None, file, "complete")?;
    Ok((merged, out))
}

async fn find_counterpart(
    config: &Config,
    llm: &impl Model,
    file: &Path,
    scan: &Scan,
    report: &Report,
    search_in: Option<&Path>,
) -> Res<Option<PathBuf>> {
    let dir = search_dir(file, search_in);
    let Search {
        candidates,
        rejected,
    } = search_counterparts(config, file, scan, report, &dir)?;
    if !rejected.is_empty() {
        println!("Not suggested:");
        for (path, reason) in &rejected {
            println!("  {}: {reason}", path.display());
        }
    }
    if candidates.is_empty() {
        println!("No counterpart found in {}.", dir.display());
        return Ok(None);
    }

    for candidate in &candidates {
        println!();
        println!("Possible counterpart: {}", candidate.path.display());
        println!("  {}", describe(candidate));
        if !candidate.still.is_empty() {
            println!(
                "  still missing afterwards: {}",
                join_numbers(&candidate.still)
            );
        }
        if !prompt::ask_yes_no("Merge and sort with this file? [y]es / [n]o: ")? {
            continue;
        }

        let (merged, out) = merge_with(file, scan, candidate)?;
        let written = sort::run(config, llm, merged.path(), Some(&out), false).await?;
        if written.is_some() {
            if !candidate.still.is_empty() {
                println!(
                    "Still missing: printed page {}. Run `counterpart` on the result to look further.",
                    join_numbers(&candidate.still)
                );
            }
            trash::offer(&[file.to_path_buf(), candidate.path.clone()])?;
        }
        return Ok(written);
    }
    println!("No file merged.");
    Ok(None)
}

/// Other PDFs in `dir`: not the file itself, not an identical copy, and not one of
/// its own results (`<stem>_sorted.pdf`, `<stem>_merged.pdf`, `<stem>_complete.pdf`, …).
fn candidate_paths(file: &Path, dir: &Path) -> Res<Vec<PathBuf>> {
    let own_hash = hash::sha256_file(file)?;
    let own_stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
        })
        .filter(|p| {
            let stem = p
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            stem != own_stem && !stem.starts_with(&format!("{own_stem}_"))
        })
        .filter(|p| hash::sha256_file(p).map(|h| h != own_hash).unwrap_or(false))
        .collect();
    paths.sort();
    Ok(paths)
}

/// Numbers with at least 5 characters, like contract numbers (3230 / 1051.00012.06),
/// customer numbers and dates (29.10.2025). Plain years and amounts are too short.
static IDENTIFIER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\d[\d./-]{3,}\d").expect("valid regex"));

fn identifiers(pages: &[Page]) -> HashSet<String> {
    pages
        .iter()
        .filter(|p| p.is_content())
        .flat_map(|p| {
            IDENTIFIER
                .find_iter(&p.text)
                .map(|m| m.as_str().to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages;

    /// Distinct filler text per page, so pages are not taken for duplicates.
    fn page(i: usize, header: &str) -> String {
        format!(
            "{header} Vertragsnummer 1051.00012.06 vom 29.10.2025 inhalt{i} wort{i}a wort{i}b wort{i}c"
        )
    }

    #[test]
    fn reports_missing_pages() {
        // Pages 2, 4 and 8 of a statement, like a one-sided scan pass.
        let scan = pages::analyze(vec![
            page(1, "Seite 2"),
            page(2, "Seite 4"),
            page(3, "Seite 8"),
        ]);
        let report = completeness(&scan);
        assert_eq!(report.numbers, [2, 4, 8]);
        assert_eq!(report.missing, [1, 3, 5, 6, 7]);
        assert!(report.is_incomplete());
    }

    #[test]
    fn unnumbered_page_counts_as_page_one() {
        let scan = pages::analyze(vec![
            page(1, "LEG Wohnen Briefkopf"),
            page(2, "Seite 2 von 3"),
            page(3, "Seite 3 von 3"),
        ]);
        let report = completeness(&scan);
        assert!(report.missing.is_empty() && !report.is_incomplete());
        assert_eq!(report.last, 3);
    }

    #[test]
    fn total_reveals_missing_last_pages() {
        let scan = pages::analyze(vec![page(1, "Seite 1 von 4"), page(2, "Seite 2 von 4")]);
        assert_eq!(completeness(&scan).missing, [3, 4]);
    }

    #[test]
    fn repeated_numbers_are_reported() {
        let scan = pages::analyze(vec![
            page(1, "Seite 1"),
            page(2, "Seite 2"),
            page(3, "Seite 1"),
        ]);
        let report = completeness(&scan);
        assert_eq!(report.repeated, [(1, vec![1, 3])]);
        assert!(!report.is_incomplete());
    }

    #[test]
    fn a_repeated_number_does_not_hide_the_gaps() {
        // Fronts of a one-sided scan, with one number found twice.
        let scan = pages::analyze(vec![
            page(1, "Seite 3"),
            page(2, "Seite 5"),
            page(3, "Seite 3"),
            page(4, "Seite 7"),
        ]);
        let report = completeness(&scan);
        assert_eq!(report.repeated, [(3, vec![1, 3])]);
        assert_eq!(report.missing, [1, 2, 4, 6]);
        assert!(report.is_incomplete(), "the counterpart search is offered");
    }

    fn scan(texts: &[&str]) -> Scan {
        Scan::from_pages(pages::analyze(
            texts.iter().map(|t| t.to_string()).collect(),
        ))
    }

    fn names(candidates: &[Candidate]) -> Vec<String> {
        candidates
            .iter()
            .map(|c| c.path.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn letters_from_the_same_sender_are_ranked_by_what_sets_them_apart() {
        // Same landlord, same contract number in every letter; only the letter
        // date and invoice number tell this year's letter apart.
        let own = scan(&[
            "Seite 1 von 2 Vertrag 1051.00012.06 Rechnung 2025-117 vom 29.10.2025 Kunde 4711.0815",
        ]);
        let report = completeness(&own.pages);
        let others = vec![
            (
                "a-last-year.pdf".into(),
                scan(&["Seite 2 von 2 Vertrag 1051.00012.06 Kunde 4711.0815 Rechnung 2024-093"]),
            ),
            (
                "b-backs.pdf".into(),
                scan(&[
                    "Seite 2 von 2 Vertrag 1051.00012.06 Kunde 4711.0815 Rechnung 2025-117 29.10.2025",
                ]),
            ),
            (
                "c-year-before.pdf".into(),
                scan(&["Seite 2 von 2 Vertrag 1051.00012.06 Kunde 4711.0815 Rechnung 2023-071"]),
            ),
        ];
        let found = rank_counterparts(&own, &report, others).candidates;
        // All are suggested (they share numbers), but this year's backs come first.
        assert_eq!(
            names(&found),
            ["b-backs.pdf", "a-last-year.pdf", "c-year-before.pdf"]
        );
        assert_eq!((found[0].shared, found[0].distinctive), (4, 2));
        assert_eq!(
            found[0].examples,
            ["29.10.2025", "2025-117", "1051.00012.06"]
        );
    }

    #[test]
    fn another_copy_or_a_different_total_is_no_counterpart() {
        let own = scan(&[
            "Seite 1 von 3 Rechnung 2025-117 vom 29.10.2025 Kunde 4711.0815 Zähler 8812-301",
            "Seite 3 von 3 Rechtsbehelfsbelehrung und freundliche Grüße",
        ]);
        let report = completeness(&own.pages);
        assert_eq!(report.missing, [2]);
        let others = vec![
            (
                "full-copy.pdf".into(),
                scan(&[
                    "Seite 1 von 3 Rechnung 2025-117 vom 29.10.2025",
                    "Seite 2 von 3 Heizkosten und Wasser nach Verbrauch",
                    "Seite 3 von 3 Rechtsbehelfsbelehrung und freundliche Grüße",
                ]),
            ),
            (
                "other-letter.pdf".into(),
                scan(&["Seite 2 von 5 Kunde 4711.0815 Zähler 8812-301"]),
            ),
        ];
        let search = rank_counterparts(&own, &report, others);
        assert!(search.candidates.is_empty());
        let reasons: Vec<&str> = search.rejected.iter().map(|(_, r)| r.as_str()).collect();
        assert_eq!(
            reasons,
            [
                "has this scan's own page 1, 3 (another copy or another letter)",
                "says \"von 5\", this scan \"von 3\"",
            ]
        );
    }

    #[test]
    fn finds_shared_reference_numbers() {
        let a = pages::analyze(vec![page(1, "Seite 1 Kundennummer 0118413100")]);
        let b = pages::analyze(vec![page(2, "Seite 2 Kundennummer 0118413100")]);
        let c = pages::analyze(vec![
            "Seite 3 ganz anderer Brief Rechnung 4711-0815 vom 01.02.2026 Text".into(),
        ]);
        assert_eq!(identifiers(&a).intersection(&identifiers(&b)).count(), 3);
        assert_eq!(identifiers(&a).intersection(&identifiers(&c)).count(), 0);
    }
}
