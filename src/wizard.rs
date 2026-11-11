//! `wizard`: I drop a pile of PDFs, it handles each one without asking:
//!
//! 1. check every file (OCR if needed): pages, name, already in the box;
//! 2. incomplete → merge it with its counterpart from the same pile and sort the pages;
//! 3. add it to the box with the AI's date, tags and summary; an unreadable name
//!    gets a readable one from the same request (one model call instead of two).
//!
//! The decisions are the same plain rules as `check` and `counterpart` (printed page
//! numbers, shared reference numbers); the AI does what it does there: sort pages,
//! propose names and entries. Whatever cannot be decided safely is left for me,
//! with the reason: pages missing without a clear counterpart, several documents
//! in one scan, no usable answer from the model. Originals are never deleted here.

use crate::{
    Res,
    add::{self, Prepared, Proposal},
    complete::{self, Diagnosis},
    config::Config,
    entry::Entry,
    hash, index,
    llm::Model,
    naming,
    pages::join_numbers,
    pagetext, rename,
    scan::Scan,
    sort, trash,
    workspace::Workspace,
};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// How a file ended.
pub enum Status {
    /// Stored in the box with this entry.
    Added { stored: PathBuf, entry: Entry },
    /// The identical file is in the box already.
    AlreadyInBox,
    /// Nothing to do with it, e.g. an identical copy of another file in the pile.
    Skipped(String),
    /// Left for me, with the reason.
    NeedsYou(String),
}

/// One document of the pile: where it came from, what was done, how it ended.
pub struct Item {
    /// The file(s) it started from (two if it was completed with a counterpart).
    pub originals: Vec<PathBuf>,
    /// The file as it is now: renamed, or the completed result.
    pub file: PathBuf,
    /// What was done, in order.
    pub steps: Vec<String>,
    pub status: Status,
}

impl Item {
    fn new(file: &Path) -> Self {
        Item {
            originals: vec![file.to_path_buf()],
            file: file.to_path_buf(),
            steps: Vec::new(),
            status: Status::Skipped(String::new()),
        }
    }

    /// One line for a log, e.g. "added: 2025-10-29_nebenkosten.pdf (2025-10-29, …)".
    pub fn line(&self) -> String {
        let name = file_name(&self.file);
        let steps = if self.steps.is_empty() {
            String::new()
        } else {
            format!(" [{}]", self.steps.join("; "))
        };
        match &self.status {
            Status::Added { entry, .. } => {
                format!("added: {name} ({}, {}){steps}", entry.date, entry.summary)
            }
            Status::AlreadyInBox => format!("already in the box: {name}{steps}"),
            Status::Skipped(why) => format!("skipped: {name}: {why}"),
            Status::NeedsYou(why) => format!("needs you: {name}: {why}{steps}"),
        }
    }
}

pub struct Summary {
    pub items: Vec<Item>,
}

impl Summary {
    /// Originals that were merged into a result that went into the box: all their
    /// pages are in it, so they may go to the Trash.
    pub fn spare_originals(&self) -> Vec<PathBuf> {
        self.items
            .iter()
            .filter(|i| matches!(i.status, Status::Added { .. }) && i.originals.len() > 1)
            .flat_map(|i| i.originals.iter().cloned())
            .filter(|p| p.is_file())
            .collect()
    }

    pub fn count(&self, what: fn(&Status) -> bool) -> usize {
        self.items.iter().filter(|i| what(&i.status)).count()
    }
}

/// `wizard <PDF or folder>...`: handles the pile, prints what happened, then
/// offers to move the merged originals to the Trash.
pub async fn run(config: &Config, ws: &Workspace, llm: &impl Model, inputs: &[PathBuf]) -> Res<()> {
    let summary = handle(config, ws, llm, inputs, &mut |line| eprintln!("{line}")).await?;
    println!();
    for item in &summary.items {
        println!("{}", item.line());
    }
    println!(
        "\n{} added, {} already in the box, {} need you, {} skipped.",
        summary.count(|s| matches!(s, Status::Added { .. })),
        summary.count(|s| matches!(s, Status::AlreadyInBox)),
        summary.count(|s| matches!(s, Status::NeedsYou(_))),
        summary.count(|s| matches!(s, Status::Skipped(_))),
    );
    let spare = summary.spare_originals();
    if !spare.is_empty() {
        trash::offer(&spare)?;
    }
    Ok(())
}

/// The PDFs to handle: the files given, and the PDFs directly inside the folders given.
pub fn pdfs_in(inputs: &[PathBuf]) -> Res<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for input in inputs {
        if input.is_dir() {
            let mut inside: Vec<PathBuf> = fs::read_dir(input)
                .map_err(|e| format!("cannot read {}: {e}", input.display()))?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.is_file() && is_pdf(p))
                .collect();
            inside.sort();
            paths.extend(inside);
        } else if input.is_file() && is_pdf(input) {
            paths.push(input.clone());
        } else {
            return Err(format!("{} is neither a PDF nor a folder", input.display()).into());
        }
    }
    let mut unique = Vec::new();
    for path in paths {
        if !unique.contains(&path) {
            unique.push(path);
        }
    }
    Ok(unique)
}

/// Handles every PDF of `inputs` (see the module docs). `log` gets a line before
/// each slow step and one per finished document. An error in one document is
/// reported for that document; the others go on.
pub async fn handle(
    config: &Config,
    ws: &Workspace,
    llm: &impl Model,
    inputs: &[PathBuf],
    log: &mut impl FnMut(String),
) -> Res<Summary> {
    let paths = pdfs_in(inputs)?;
    if paths.is_empty() {
        return Err("no PDFs found".into());
    }
    let mut items = Vec::new();

    // 1. Check every file, so counterparts can be matched within the pile.
    let mut checked: Vec<(PathBuf, Diagnosis)> = Vec::new();
    let mut seen: HashMap<String, PathBuf> = HashMap::new();
    for (n, path) in paths.iter().enumerate() {
        log(format!(
            "Checking {} ({}/{}, OCR can take a while)…",
            file_name(path),
            n + 1,
            paths.len()
        ));
        // A pause, so a cancel takes effect between files.
        tokio::task::yield_now().await;
        let mut item = Item::new(path);
        let hash = match hash::sha256_file(path) {
            Ok(hash) => hash,
            Err(e) => {
                item.status = Status::NeedsYou(format!("cannot be read: {e}"));
                log(item.line());
                items.push(item);
                continue;
            }
        };
        if let Some(first) = seen.get(&hash) {
            item.status = Status::Skipped(format!("identical copy of {}", file_name(first)));
            log(item.line());
            items.push(item);
            continue;
        }
        seen.insert(hash, path.clone());
        match complete::diagnose(config, ws, path) {
            Ok(d) if d.in_box => {
                item.status = Status::AlreadyInBox;
                log(item.line());
                items.push(item);
            }
            Ok(d) => checked.push((path.clone(), d)),
            Err(e) => {
                item.status = Status::NeedsYou(format!("cannot be read: {e}"));
                log(item.line());
                items.push(item);
            }
        }
    }

    // 2. Each document: complete, name, add.
    let mut used = vec![false; checked.len()];
    for i in 0..checked.len() {
        if used[i] {
            continue; // merged into an earlier document
        }
        used[i] = true;
        tokio::task::yield_now().await;
        let mut item = Item::new(&checked[i].0);
        item.status =
            match handle_one(config, ws, llm, &checked, &mut used, i, &mut item, log).await {
                Ok(status) => status,
                Err(e) => Status::NeedsYou(e.to_string()),
            };
        log(item.line());
        items.push(item);
    }
    Ok(Summary { items })
}

#[allow(clippy::too_many_arguments)]
async fn handle_one(
    config: &Config,
    ws: &Workspace,
    llm: &impl Model,
    checked: &[(PathBuf, Diagnosis)],
    used: &mut [bool],
    i: usize,
    item: &mut Item,
    log: &mut impl FnMut(String),
) -> Res<Status> {
    let (path, diagnosis) = &checked[i];
    let report = &diagnosis.report;
    // The text is known from the check, so naming and adding need no second OCR.
    // A completed result is built from searchable pages; its text is read from it.
    let mut page_texts = Some(diagnosis.page_texts());

    if report.is_incomplete() {
        let missing = join_numbers(&report.missing);
        let pool: Vec<(usize, &Path, &Scan)> = checked
            .iter()
            .enumerate()
            .filter(|(j, _)| !used[*j])
            .map(|(j, (p, d))| (j, p.as_path(), &d.scan))
            .collect();
        let others: Vec<(&Path, &Scan)> = pool.iter().map(|(_, p, s)| (*p, *s)).collect();
        let (fits, _) = complete::judge(&diagnosis.scan, report, &others);
        let Some((k, best)) = fits.first() else {
            return Ok(Status::NeedsYou(format!(
                "printed page {missing} missing, and no other file of the pile holds it"
            )));
        };
        if let Some((k2, second)) = fits.get(1)
            && second.rank() == best.rank()
        {
            return Ok(Status::NeedsYou(format!(
                "printed page {missing} missing; {} and {} fit equally well, choose with Counterpart",
                file_name(pool[*k].1),
                file_name(pool[*k2].1)
            )));
        }
        let (j, partner, partner_scan) = pool[*k];
        used[j] = true;
        item.originals.push(partner.to_path_buf());

        log(format!(
            "Completing {} with {} and sorting the pages…",
            file_name(path),
            file_name(partner)
        ));
        let (merged, out) = complete::merge_scans(path, &diagnosis.scan, partner_scan)?;
        let plan = sort::propose(config, llm, merged.path(), Some(&out)).await?;
        let (out, pages) = sort::write(&plan, &plan.groups, false)?;
        item.steps.push(format!(
            "completed with {} ({pages} pages)",
            file_name(partner)
        ));
        item.file = out;
        page_texts = None;
        if !best.still.is_empty() {
            return Ok(Status::NeedsYou(format!(
                "printed page {} still missing after completing",
                join_numbers(&best.still)
            )));
        }
    } else if let Some((n, on)) = report.repeated.first() {
        return Ok(Status::NeedsYou(format!(
            "printed page {n} is on pages {}: several documents in one scan, or a misread number; \
             sort or split it first",
            join_numbers(on)
        )));
    }

    if naming::is_readable(&file_stem(&item.file)) {
        return add_as_is(config, ws, llm, item, page_texts, log).await;
    }

    // An unreadable name: one request gives the entry and the name.
    let page_texts = match page_texts {
        Some(texts) => texts,
        None => pagetext::read_pdf(&item.file, &config.ocr_language)?,
    };
    let text = pagetext::joined(&page_texts);
    let hash = hash::sha256_file(&item.file)?;
    let index = index::read(&ws.index_path())?;
    if index::contains(&index, &hash) {
        return Ok(Status::AlreadyInBox);
    }
    log(format!(
        "Asking for a name, date, tags and summary of {}…",
        file_name(&item.file)
    ));
    let (entry, title) =
        match add::propose_named(llm, &text, &index::tags(&index), config.review).await {
            Ok(answer) => answer,
            Err(e) => {
                return Ok(Status::NeedsYou(format!(
                    "not added: the model gave no usable name and entry ({e}); add it yourself"
                )));
            }
        };
    if let Some(stem) = naming::stem(&entry.date, &[&title, &entry.summary])
        && stem != file_stem(&item.file)
    {
        let old = file_name(&item.file);
        item.file = rename::apply(&item.file, &stem)?;
        item.steps.push(format!("renamed from {old}"));
    }
    let proposal = Proposal {
        file: item.file.clone(),
        hash,
        entry,
        pages: page_texts,
        model_error: None,
    };
    let stored = add::save(ws, &proposal, &proposal.entry)?;
    Ok(Status::Added {
        stored,
        entry: proposal.entry.normalized(),
    })
}

/// A file that already has a readable name: only the entry is asked for.
async fn add_as_is(
    config: &Config,
    ws: &Workspace,
    llm: &impl Model,
    item: &Item,
    page_texts: Option<Vec<String>>,
    log: &mut impl FnMut(String),
) -> Res<Status> {
    log(format!(
        "Asking for date, tags and summary of {}…",
        file_name(&item.file)
    ));
    match add::prepare_with_pages(config, ws, llm, &item.file, page_texts).await? {
        Prepared::AlreadyThere(_) => Ok(Status::AlreadyInBox),
        Prepared::New(proposal) => {
            if let Some(e) = &proposal.model_error {
                return Ok(Status::NeedsYou(format!(
                    "not added: the model gave no date, tags and summary ({e}); add it yourself"
                )));
            }
            let stored = add::save(ws, &proposal, &proposal.entry)?;
            Ok(Status::Added {
                stored,
                entry: proposal.entry.clone().normalized(),
            })
        }
    }
}

fn is_pdf(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("pdf"))
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
