//! `check`: is a scanned document complete, judging by its printed page numbers?
//! Is its name readable, is it already in the box? Plain code, no AI.

use crate::{
    Res,
    config::Config,
    hash, index,
    llm::Model,
    naming,
    pages::{Page, join_numbers},
    prompt, rename,
    scan::Scan,
    store,
    workspace::Workspace,
};
use std::collections::BTreeSet;
use std::path::Path;

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
    /// Several documents in one scan, or a misread number.
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
/// whether it is in the box; then offers a readable name if the name is not.
pub async fn check(config: &Config, ws: &Workspace, llm: &impl Model, file: &Path) -> Res<()> {
    let diagnosis = diagnose(config, ws, file)?;
    for line in &diagnosis.lines {
        println!("{line}");
    }
    println!();
    if !diagnosis.readable_name
        && prompt::ask_yes_no(&format!(
            "Give {} a readable name? [y]es / [n]o: ",
            file.display()
        ))?
    {
        // The file was read (and OCR'd) above already.
        let text = diagnosis.text();
        rename::name_file(config, ws, llm, file, Some(&text)).await?;
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
        assert!(report.is_incomplete());
    }
}
