//! `check`: is a scanned document complete, judging by its printed page numbers?
//! `counterpart`: find the missing pages in another PDF, then merge and sort.
//!
//! Both are plain code, no AI: printed page numbers and shared reference numbers
//! (contract numbers, customer numbers, dates) are reliable and fast to compare.

use crate::{
    Res, config::Config, hash, index, naming, pages::Page, pdf::TempFile, pdfdoc, prompt, rename,
    scan::Scan, sort, store, trash, workspace::Workspace,
};
use regex::Regex;
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// A counterpart must share at least this many reference numbers or dates.
const MIN_SHARED: usize = 2;

/// What the printed page numbers say about a scan.
pub struct Report {
    /// Printed page numbers found on content pages, sorted.
    pub numbers: Vec<u32>,
    /// Highest page the document should have ("von N", or the highest number seen).
    pub last: u32,
    /// Pages between 1 and `last` that are not in the scan. Pages without a printed
    /// number are assumed to fill the lowest gaps (usually page 1 with the letterhead).
    pub missing: Vec<u32>,
    /// A page number appears twice: the scan probably holds several documents.
    pub several_documents: bool,
}

impl Report {
    pub fn is_incomplete(&self) -> bool {
        !self.several_documents && !self.missing.is_empty()
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
    let unnumbered = content.iter().filter(|p| p.printed_number.is_none()).count();
    let missing = (1..=last)
        .filter(|n| !numbers.contains(n))
        .skip(unnumbered)
        .collect();
    Report {
        several_documents: numbers.len() != found.len(),
        numbers: numbers.into_iter().collect(),
        last,
        missing,
    }
}

/// `check <file>`: prints the report; if pages are missing, offers to look for them.
/// `check <file>`: a general health check. Reports pages, the file name and
/// whether it is in the box; then offers the fixes: first the counterpart
/// (if pages are missing), then a readable name (for the result, if one was made).
pub async fn check(
    config: &Config,
    ws: &Workspace,
    file: &Path,
    search_in: Option<&Path>,
) -> Res<()> {
    let scan = Scan::read(file, &config.ocr_language)?;
    let report = completeness(&scan.pages);
    print_report(file, &scan.pages, &report);

    let stem = file_stem(file);
    if naming::is_readable(&stem) {
        println!("  name: ok");
    } else {
        println!("  name: \"{stem}\" is not readable (like 2026-03-14_steuerbescheid-2025-finanzamt-muenchen)");
    }
    let hash = hash::sha256_file(file)?;
    if index::contains(&index::read(&ws.index_path())?, &hash) {
        println!("  box: already added ({})", store::path(ws, &hash).display());
    } else {
        println!("  box: not added yet");
    }
    println!();

    let mut current = file.to_path_buf();
    if report.is_incomplete()
        && prompt::ask_yes_no("Search for the counterpart? [y]es / [n]o: ")?
        && let Some(result) = find_counterpart(config, file, &scan, &report, search_in).await?
    {
        current = result;
    }
    if current.is_file()
        && !naming::is_readable(&file_stem(&current))
        && prompt::ask_yes_no(&format!("Give {} a readable name? [y]es / [n]o: ", current.display()))?
    {
        rename::name_file(config, ws, &current).await?;
    }
    Ok(())
}

fn file_stem(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// `counterpart <file>`: looks for the missing pages right away.
pub async fn counterpart(config: &Config, file: &Path, search_in: Option<&Path>) -> Res<()> {
    let scan = Scan::read(file, &config.ocr_language)?;
    let report = completeness(&scan.pages);
    if !report.is_incomplete() {
        print_report(file, &scan.pages, &report);
        println!("Nothing to look for.");
        return Ok(());
    }
    println!("{}: printed page {} missing", file.display(), join(&report.missing));
    find_counterpart(config, file, &scan, &report, search_in).await.map(|_| ())
}

fn print_report(file: &Path, pages: &[Page], report: &Report) {
    let plural = if pages.len() == 1 { "" } else { "s" };
    println!("{}: {} page{plural}", file.display(), pages.len());
    if report.numbers.is_empty() {
        println!("  no printed page numbers found, so completeness cannot be checked");
        return;
    }
    println!("  printed page numbers: {}", join(&report.numbers));
    let blank: Vec<u32> = pages.iter().filter(|p| p.blank).map(|p| p.number).collect();
    if !blank.is_empty() {
        println!("  blank pages: {}", join(&blank));
    }
    let duplicates: Vec<String> = pages
        .iter()
        .filter_map(|p| p.duplicate_of.map(|o| format!("{} (same as {o})", p.number)))
        .collect();
    if !duplicates.is_empty() {
        println!("  duplicate scans: {}", duplicates.join(", "));
    }
    if report.several_documents {
        println!("  page numbers repeat: this scan probably holds several documents");
    } else if report.missing.is_empty() {
        println!("  complete: pages 1–{}", report.last);
        println!("  (pages after {} would not be noticed if no page says \"von N\")", report.last);
    } else {
        println!("  incomplete: printed page {} not found", join(&report.missing));
    }
}

/// A PDF that holds missing pages of the same document.
struct Candidate {
    path: PathBuf,
    scan: Scan,
    /// Missing page numbers it contains.
    fills: Vec<u32>,
    /// Reference numbers and dates it shares with the incomplete scan.
    shared: usize,
}

async fn find_counterpart(
    config: &Config,
    file: &Path,
    scan: &Scan,
    report: &Report,
    search_in: Option<&Path>,
) -> Res<Option<PathBuf>> {
    let dir = match search_in {
        Some(dir) => dir.to_path_buf(),
        None => match file.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        },
    };
    let paths = candidate_paths(file, &dir)?;
    if paths.is_empty() {
        println!("No other PDFs in {}.", dir.display());
        return Ok(None);
    }
    let plural = if paths.len() == 1 { "" } else { "s" };
    eprintln!("Looking through {} PDF{plural} in {}…", paths.len(), dir.display());

    let own_ids = identifiers(&scan.pages);
    let mut candidates = Vec::new();
    for path in paths {
        let other = match Scan::read(&path, &config.ocr_language) {
            Ok(other) => other,
            Err(e) => {
                eprintln!("  skipped {}: {e}", path.display());
                continue;
            }
        };
        let fills: Vec<u32> = report
            .missing
            .iter()
            .copied()
            .filter(|&n| {
                other
                    .pages
                    .iter()
                    .any(|p| p.is_content() && p.printed_number.map(|x| x.n) == Some(n))
            })
            .collect();
        let shared = own_ids.intersection(&identifiers(&other.pages)).count();
        if !fills.is_empty() && shared >= MIN_SHARED {
            candidates.push(Candidate { path, scan: other, fills, shared });
        }
    }
    // Best first: fills the most missing pages, then shares the most references.
    candidates.sort_by_key(|c| std::cmp::Reverse((c.fills.len(), c.shared)));

    if candidates.is_empty() {
        println!(
            "No counterpart found in {}. It must contain at least one missing page and share \
             at least {MIN_SHARED} reference numbers or dates.",
            dir.display()
        );
        return Ok(None);
    }

    for candidate in &candidates {
        // Like in `sort`, the counterpart's unnumbered pages (often page 1 with the
        // letterhead) are assumed to fill the lowest remaining gaps.
        let unnumbered = candidate
            .scan
            .pages
            .iter()
            .filter(|p| p.is_content() && p.printed_number.is_none())
            .count();
        let still: Vec<u32> = report
            .missing
            .iter()
            .copied()
            .filter(|n| !candidate.fills.contains(n))
            .skip(unnumbered)
            .collect();
        println!();
        println!("Possible counterpart: {}", candidate.path.display());
        println!(
            "  has missing page {}, shares {} reference numbers/dates",
            join(&candidate.fills),
            candidate.shared
        );
        if !still.is_empty() {
            println!("  still missing afterwards: {}", join(&still));
        }
        if !prompt::ask_yes_no("Merge and sort with this file? [y]es / [n]o: ")? {
            continue;
        }

        // Merge the searchable versions, so sort does not run OCR on them again.
        let merged = TempFile::new("counterpart-merged");
        pdfdoc::merge(
            &[scan.source().to_path_buf(), candidate.scan.source().to_path_buf()],
            merged.path(),
        )?;
        let out = pdfdoc::output_path(None, file, "complete")?;
        let written = sort::run(config, merged.path(), Some(&out), false).await?;
        if written.is_some() {
            if !still.is_empty() {
                println!(
                    "Still missing: printed page {}. Run `counterpart` on the result to look further.",
                    join(&still)
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
    let own_stem = file.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut paths: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.is_file())
        .filter(|p| {
            p.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
        })
        .filter(|p| {
            let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
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
        .flat_map(|p| IDENTIFIER.find_iter(&p.text).map(|m| m.as_str().to_string()))
        .collect()
}

fn join(numbers: &[u32]) -> String {
    numbers.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pages;

    /// Distinct filler text per page, so pages are not taken for duplicates.
    fn page(i: usize, header: &str) -> String {
        format!("{header} Vertragsnummer 1051.00012.06 vom 29.10.2025 inhalt{i} wort{i}a wort{i}b wort{i}c")
    }

    #[test]
    fn reports_missing_pages() {
        // Pages 2, 4 and 8 of a statement, like a one-sided scan pass.
        let scan = pages::analyze(vec![page(1, "Seite 2"), page(2, "Seite 4"), page(3, "Seite 8")]);
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
    fn repeated_numbers_mean_several_documents() {
        let scan = pages::analyze(vec![page(1, "Seite 1"), page(2, "Seite 2"), page(3, "Seite 1")]);
        let report = completeness(&scan);
        assert!(report.several_documents && !report.is_incomplete());
    }

    #[test]
    fn finds_shared_reference_numbers() {
        let a = pages::analyze(vec![page(1, "Seite 1 Kundennummer 0118413100")]);
        let b = pages::analyze(vec![page(2, "Seite 2 Kundennummer 0118413100")]);
        let c = pages::analyze(vec!["Seite 3 ganz anderer Brief Rechnung 4711-0815 vom 01.02.2026 Text".into()]);
        assert_eq!(identifiers(&a).intersection(&identifiers(&b)).count(), 3);
        assert_eq!(identifiers(&a).intersection(&identifiers(&c)).count(), 0);
    }
}
