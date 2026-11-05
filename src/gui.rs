//! `gui`: a small window for the everyday jobs.
//!
//! - Files: open PDFs (dialog or drop), select some, and run an action on them:
//!   add to the box, name, merge, sort or check.
//! - Find: browse all documents or search by question.
//! - Add: review the AI's proposal for each file before it goes into the box.
//!
//! The window only shows and asks; the work is done by the same functions the
//! command line uses (`add::prepare`, `rename::propose`, `sort::propose`, …).
//! Anything slow (OCR, the model) runs on a background thread, so the window
//! never freezes.

use crate::{
    Res,
    add::{self, Prepared, Proposal},
    complete::{self, Diagnosis},
    config::Config,
    entry::Entry,
    find, index,
    index::Row,
    llm::Llm,
    merge, pages,
    rename::{self, NameProposal},
    sort::{self, SortPlan},
    store, trash,
    workspace::Workspace,
};
use eframe::egui;
use std::collections::VecDeque;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

/// Opens the window and returns when it is closed.
pub fn run(ws: Workspace, config: Config) -> Res<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Smart Shoebox")
            .with_inner_size([900.0, 700.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Smart Shoebox",
        options,
        Box::new(|_cc| Ok(Box::new(App::new(ws, config)))),
    )
    .map_err(|e| format!("cannot open the window: {e}").into())
}

#[derive(PartialEq)]
enum Tab {
    Files,
    Find,
    Add,
}

/// Results coming back from a background job.
enum Msg {
    Found(Result<find::Found, String>),
    Prepared(PathBuf, Result<Prepared, String>),
    Named(Vec<(PathBuf, Result<NameProposal, String>)>),
    Sorted(PathBuf, Result<SortPlan, String>),
    Checked(Vec<(PathBuf, Result<Diagnosis, String>)>),
}

/// A file in the Files tab.
struct OpenFile {
    path: PathBuf,
    selected: bool,
}

/// What the Files tab shows below the list: the result of the last action.
enum Pane {
    Empty,
    Names(Vec<NameItem>),
    Sort(SortView),
    /// One report per checked file (or why it could not be checked).
    Check(Vec<(PathBuf, Result<Diagnosis, String>)>),
    /// A result was written; offers to move the originals to the Trash.
    Done {
        heading: String,
        originals: Vec<PathBuf>,
        offer_trash: bool,
    },
}

struct NameItem {
    proposal: NameProposal,
    /// The name I am editing, without `.pdf`.
    stem: String,
    /// Set once renamed or skipped.
    done: Option<String>,
}

struct SortView {
    input: PathBuf,
    plan: SortPlan,
    /// The page order I am editing, e.g. `1,3,2; 4,5`.
    order: String,
    delete_blank: bool,
}

/// Something I clicked in a pane; applied after drawing it.
enum Click {
    Rename(usize),
    SkipName(usize),
    WriteSort,
    Close,
    Name(PathBuf, Option<String>),
    Sort(PathBuf),
    AddToBox(PathBuf),
    TrashOriginals,
    /// Name several files; each with its text if it is already known.
    NameAll(Vec<(PathBuf, Option<String>)>),
    AddAllToBox(Vec<PathBuf>),
}

struct App {
    ws: Workspace,
    config: Config,
    tab: Tab,
    files: Vec<OpenFile>,
    pane: Pane,
    /// All documents, newest first (shown when there is no search result).
    all: Vec<Row>,
    question: String,
    /// The last search: the question and its matches.
    results: Option<(String, Vec<Row>)>,
    /// Files waiting for an add proposal.
    queue: VecDeque<PathBuf>,
    /// The add proposal I am looking at, and my edited version of it.
    current: Option<(Proposal, Entry)>,
    /// What a running job is doing.
    busy: Option<String>,
    /// The last message, and whether it is an error.
    status: Option<(String, bool)>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl App {
    fn new(ws: Workspace, config: Config) -> Self {
        let (tx, rx) = channel();
        let mut app = App {
            ws,
            config,
            tab: Tab::Files,
            files: Vec::new(),
            pane: Pane::Empty,
            all: Vec::new(),
            question: String::new(),
            results: None,
            queue: VecDeque::new(),
            current: None,
            busy: None,
            status: None,
            tx,
            rx,
        };
        app.reload();
        app
    }

    fn reload(&mut self) {
        match index::read(&self.ws.index_path()) {
            Ok(text) => {
                let mut rows = index::parse(&text);
                rows.sort_by(|a, b| b.date.cmp(&a.date));
                self.all = rows;
            }
            Err(e) => self.error(e.to_string()),
        }
    }

    fn info(&mut self, text: impl Into<String>) {
        self.status = Some((text.into(), false));
    }

    fn error(&mut self, text: impl Into<String>) {
        self.status = Some((text.into(), true));
    }

    /// Runs `job` on its own thread with its own model client and sends the
    /// result back. The window is repainted as soon as the result is there.
    fn spawn<F, Fut>(&mut self, ctx: &egui::Context, what: &str, job: F)
    where
        F: FnOnce(Workspace, Config, Llm) -> Fut + Send + 'static,
        Fut: Future<Output = Msg>,
    {
        self.busy = Some(what.to_string());
        let (ws, config, tx, ctx) = (
            self.ws.clone(),
            self.config.clone(),
            self.tx.clone(),
            ctx.clone(),
        );
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("cannot start the async runtime");
            let llm = Llm::new(&config);
            let msg = runtime.block_on(job(ws, config, llm));
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
    }

    // ----- files -----

    fn open_files(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        let mut opened = false;
        for path in paths {
            let is_pdf = path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
            if !is_pdf || !path.is_file() {
                self.error(format!("{} is not a PDF file.", path.display()));
                continue;
            }
            opened = true;
            match self.files.iter_mut().find(|f| f.path == path) {
                Some(file) => file.selected = true,
                None => self.files.push(OpenFile {
                    path,
                    selected: true,
                }),
            }
        }
        if opened {
            self.tab = Tab::Files;
        }
    }

    fn selected(&self) -> Vec<PathBuf> {
        self.files
            .iter()
            .filter(|f| f.selected)
            .map(|f| f.path.clone())
            .collect()
    }

    /// Adds a result (merged, sorted file) to the list as the only selected file.
    fn show_result(&mut self, path: PathBuf) {
        for file in &mut self.files {
            file.selected = false;
        }
        self.open_files([path]);
    }

    fn renamed(&mut self, old: &Path, new: &Path) {
        for file in &mut self.files {
            if file.path == old {
                file.path = new.to_path_buf();
            }
        }
    }

    // ----- starting jobs -----

    fn start_search(&mut self, ctx: &egui::Context) {
        let question = self.question.trim().to_string();
        if question.is_empty() || self.busy.is_some() {
            return;
        }
        self.spawn(ctx, "Searching…", move |ws, config, llm| async move {
            let found = find::search(&config, &ws, &llm, &question)
                .await
                .map_err(|e| e.to_string());
            Msg::Found(found)
        });
    }

    /// Starts the next file waiting to be added, if nothing else is going on.
    fn next_proposal(&mut self, ctx: &egui::Context) {
        if self.busy.is_some() || self.current.is_some() {
            return;
        }
        let Some(file) = self.queue.pop_front() else {
            return;
        };
        let what = format!(
            "Reading {} and asking the model (OCR can take a while)…",
            file_name(&file)
        );
        self.spawn(ctx, &what, move |ws, config, llm| async move {
            let result = add::prepare(&config, &ws, &llm, &file)
                .await
                .map_err(|e| e.to_string());
            Msg::Prepared(file, result)
        });
    }

    fn queue_for_box(&mut self, files: Vec<PathBuf>) {
        for file in files {
            if !self.queue.contains(&file) {
                self.queue.push_back(file);
            }
        }
        self.tab = Tab::Add;
    }

    /// Proposes names; each file comes with its text if it is already known
    /// (e.g. from a check), so it is not read and OCR'd again.
    fn start_naming(&mut self, ctx: &egui::Context, files: Vec<(PathBuf, Option<String>)>) {
        let what = format!("Finding names for {} file(s)…", files.len());
        self.spawn(ctx, &what, move |ws, config, llm| async move {
            let mut results = Vec::new();
            for (file, text) in files {
                let proposal = rename::propose(&config, &ws, &llm, &file, text.as_deref())
                    .await
                    .map_err(|e| e.to_string());
                results.push((file, proposal));
            }
            Msg::Named(results)
        });
    }

    fn start_sort(&mut self, ctx: &egui::Context, file: PathBuf) {
        let what = format!("Sorting {} (OCR can take a while)…", file_name(&file));
        self.spawn(ctx, &what, move |_ws, config, llm| async move {
            let plan = sort::propose(&config, &llm, &file, None)
                .await
                .map_err(|e| e.to_string());
            Msg::Sorted(file, plan)
        });
    }

    fn start_check(&mut self, ctx: &egui::Context, files: Vec<PathBuf>) {
        let what = match files.as_slice() {
            [file] => format!("Checking {} (OCR can take a while)…", file_name(file)),
            _ => format!("Checking {} files (OCR can take a while)…", files.len()),
        };
        self.spawn(ctx, &what, move |ws, config, _llm| async move {
            let results = files
                .into_iter()
                .map(|file| {
                    let diagnosis =
                        complete::diagnose(&config, &ws, &file).map_err(|e| e.to_string());
                    (file, diagnosis)
                })
                .collect();
            Msg::Checked(results)
        });
    }

    fn do_merge(&mut self, files: Vec<PathBuf>) {
        match merge::merge(&files, None) {
            Ok(merged) => {
                if merged.complete {
                    self.info(format!(
                        "Wrote {} ({} pages).",
                        file_name(&merged.out),
                        merged.pages
                    ));
                } else {
                    self.error(format!(
                        "Wrote {}, but it has {} instead of {} pages; keep the originals.",
                        file_name(&merged.out),
                        merged.pages,
                        merged.expected
                    ));
                }
                self.show_result(merged.out.clone());
                self.pane = Pane::Done {
                    heading: format!("Merged into {}", file_name(&merged.out)),
                    originals: merge::originals(&files),
                    offer_trash: merged.complete,
                };
            }
            Err(e) => self.error(e.to_string()),
        }
    }

    // ----- results -----

    fn receive(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.busy = None;
            match msg {
                Msg::Found(Ok(found)) => {
                    match (&found.note, found.rows.len()) {
                        (Some(note), _) => self.info(note.clone()),
                        (None, 0) => self.info("No matching document found."),
                        (None, n) => self.info(format!("{n} matching document(s).")),
                    }
                    self.results = Some((self.question.trim().to_string(), found.rows));
                }
                Msg::Found(Err(e)) => self.error(e),
                Msg::Prepared(file, Ok(Prepared::AlreadyThere(_))) => {
                    self.info(format!("{} is already in the box.", file_name(&file)));
                }
                Msg::Prepared(_, Ok(Prepared::New(proposal))) => {
                    match &proposal.model_error {
                        Some(e) => self.error(format!(
                            "The model gave no usable answer ({e}); please fill in the fields."
                        )),
                        None => self.status = None,
                    }
                    let entry = proposal.entry.clone();
                    self.current = Some((proposal, entry));
                }
                Msg::Prepared(file, Err(e)) => self.error(format!("{}: {e}", file_name(&file))),
                Msg::Named(results) => {
                    let mut items = Vec::new();
                    let mut errors = Vec::new();
                    for (file, result) in results {
                        match result {
                            Ok(proposal) => {
                                let done = (proposal.suggestion == proposal.current)
                                    .then(|| "already has a good name".to_string());
                                items.push(NameItem {
                                    stem: proposal.suggestion.clone(),
                                    proposal,
                                    done,
                                });
                            }
                            Err(e) => errors.push(format!("{}: {e}", file_name(&file))),
                        }
                    }
                    if errors.is_empty() {
                        self.status = None;
                    } else {
                        self.error(errors.join("; "));
                    }
                    self.pane = Pane::Names(items);
                }
                Msg::Sorted(input, Ok(plan)) => {
                    self.status = None;
                    let order = sort::format_groups(&plan.groups);
                    self.pane = Pane::Sort(SortView {
                        input,
                        plan,
                        order,
                        delete_blank: false,
                    });
                }
                Msg::Sorted(file, Err(e)) => {
                    self.error(format!("{}: {e}", file_name(&file)));
                }
                Msg::Checked(results) => {
                    let failed = results.iter().filter(|(_, r)| r.is_err()).count();
                    if failed == 0 {
                        self.status = None;
                    } else {
                        self.error(format!(
                            "{failed} of {} file(s) could not be checked.",
                            results.len()
                        ));
                    }
                    self.pane = Pane::Check(results);
                }
            }
        }
    }

    fn apply(&mut self, ctx: &egui::Context, click: Click) {
        match click {
            Click::Close => self.pane = Pane::Empty,
            Click::Rename(i) => {
                let Pane::Names(items) = &mut self.pane else {
                    return;
                };
                let item = &mut items[i];
                let stem = naming_stem(&item.stem);
                if stem.is_empty() {
                    self.status = Some(("The name is empty.".into(), true));
                    return;
                }
                if stem == item.proposal.current {
                    item.done = Some("kept the name".into());
                    return;
                }
                match rename::apply(&item.proposal.file, &stem) {
                    Ok(dest) => {
                        item.done = Some(format!("renamed to {}", file_name(&dest)));
                        let old = item.proposal.file.clone();
                        item.proposal.file = dest.clone();
                        self.renamed(&old, &dest);
                    }
                    Err(e) => self.error(e.to_string()),
                }
            }
            Click::SkipName(i) => {
                if let Pane::Names(items) = &mut self.pane {
                    items[i].done = Some("not renamed".into());
                }
            }
            Click::WriteSort => {
                let Pane::Sort(view) = &self.pane else {
                    return;
                };
                let result = sort::parse_edit(&view.order, &view.plan.scan.pages)
                    .map_err(|e| e.into())
                    .and_then(|groups| sort::write(&view.plan, &groups, view.delete_blank));
                match result {
                    Ok((out, count)) => {
                        self.info(format!(
                            "Wrote {} ({count} pages); {} is unchanged.",
                            file_name(&out),
                            file_name(&view.input)
                        ));
                        self.pane = Pane::Empty;
                        self.show_result(out);
                    }
                    Err(e) => self.error(format!("Not written: {e}")),
                }
            }
            Click::Name(file, text) => self.start_naming(ctx, vec![(file, text)]),
            Click::NameAll(files) => self.start_naming(ctx, files),
            Click::AddAllToBox(files) => self.queue_for_box(files),
            Click::Sort(file) => self.start_sort(ctx, file),
            Click::AddToBox(file) => self.queue_for_box(vec![file]),
            Click::TrashOriginals => {
                let Pane::Done { originals, .. } = &self.pane else {
                    return;
                };
                let originals = originals.clone();
                let mut errors = Vec::new();
                for file in &originals {
                    match trash::move_to_trash(file) {
                        Ok(_) => self.files.retain(|f| &f.path != file),
                        Err(e) => errors.push(format!("{}: {e}", file_name(file))),
                    }
                }
                self.pane = Pane::Empty;
                if errors.is_empty() {
                    self.info(format!("Moved {} file(s) to the Trash.", originals.len()));
                } else {
                    self.error(errors.join("; "));
                }
            }
        }
    }

    // ----- drawing -----

    fn files_tab(&mut self, ui: &mut egui::Ui) {
        let idle = self.busy.is_none();
        ui.horizontal(|ui| {
            if ui.button("Open PDFs…").clicked()
                && let Some(paths) = rfd::FileDialog::new()
                    .add_filter("PDF", &["pdf", "PDF"])
                    .pick_files()
            {
                self.open_files(paths);
            }
            if ui.button("Select all").clicked() {
                self.files.iter_mut().for_each(|f| f.selected = true);
            }
            if ui.button("Select none").clicked() {
                self.files.iter_mut().for_each(|f| f.selected = false);
            }
            if ui.button("Remove from list").clicked() {
                self.files.retain(|f| !f.selected);
            }
        });
        ui.weak("Or drop PDFs on this window. Removing from the list keeps the files.");
        ui.add_space(4.0);

        let mut swap = None;
        let mut problem = None;
        egui::ScrollArea::vertical()
            .id_salt("files")
            .max_height(200.0)
            .show(ui, |ui| {
                let count = self.files.len();
                for (i, file) in self.files.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut file.selected, file_name(&file.path));
                        if let Some(dir) = file.path.parent() {
                            ui.weak(dir.display().to_string());
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Open").clicked()
                                && let Err(e) = open(&file.path, false)
                            {
                                problem = Some(e);
                            }
                            if ui
                                .add_enabled(i + 1 < count, egui::Button::new("↓").small())
                                .on_hover_text("Move down (order for merge)")
                                .clicked()
                            {
                                swap = Some((i, i + 1));
                            }
                            if ui
                                .add_enabled(i > 0, egui::Button::new("↑").small())
                                .on_hover_text("Move up (order for merge)")
                                .clicked()
                            {
                                swap = Some((i - 1, i));
                            }
                        });
                    });
                }
            });
        if let Some((a, b)) = swap {
            self.files.swap(a, b);
        }
        if let Some(e) = problem {
            self.error(e);
        }

        let selected = self.selected();
        let n = selected.len();
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(format!("{n} selected:"));
            let ctx = ui.ctx().clone();
            if action(
                ui,
                idle && n >= 1,
                "Add to box",
                "AI proposes date, tags, summary",
            )
            .clicked()
            {
                self.queue_for_box(selected.clone());
            }
            if action(ui, idle && n >= 1, "Name", "Rename to a readable name").clicked() {
                self.start_naming(&ctx, selected.iter().map(|f| (f.clone(), None)).collect());
            }
            if action(
                ui,
                idle && n >= 2,
                "Merge",
                "Append into one PDF, in list order",
            )
            .clicked()
            {
                self.do_merge(selected.clone());
            }
            if action(
                ui,
                idle && n == 1,
                "Sort",
                "Put the pages into reading order",
            )
            .clicked()
            {
                self.start_sort(&ctx, selected[0].clone());
            }
            if action(
                ui,
                idle && n >= 1,
                "Check",
                "Pages complete? Name readable? In the box?",
            )
            .clicked()
            {
                self.start_check(&ctx, selected.clone());
            }
        });
        ui.separator();

        let click = egui::ScrollArea::vertical()
            .id_salt("pane")
            .show(ui, |ui| self.pane_ui(ui, idle))
            .inner;
        if let Some(click) = click {
            self.apply(ui.ctx(), click);
        }
    }

    fn pane_ui(&mut self, ui: &mut egui::Ui, idle: bool) -> Option<Click> {
        let mut click = None;
        match &mut self.pane {
            Pane::Empty => {
                ui.weak("Select files and choose an action.");
            }
            Pane::Names(items) => {
                ui.heading("Names");
                for (i, item) in items.iter_mut().enumerate() {
                    ui.add_space(4.0);
                    ui.label(file_name(&item.proposal.file));
                    if let Some(e) = &item.proposal.model_error {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            format!("The model gave no usable answer ({e}); type a name."),
                        );
                    }
                    match &item.done {
                        Some(done) => {
                            ui.weak(format!("  {done}"));
                        }
                        None => {
                            ui.horizontal(|ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut item.stem)
                                        .desired_width(ui.available_width() - 180.0),
                                );
                                ui.label(".pdf");
                                if ui.button("Rename").clicked() {
                                    click = Some(Click::Rename(i));
                                }
                                if ui.button("Skip").clicked() {
                                    click = Some(Click::SkipName(i));
                                }
                            });
                        }
                    }
                }
                ui.add_space(8.0);
                if ui.button("Done").clicked() {
                    click = Some(Click::Close);
                }
            }
            Pane::Sort(view) => {
                ui.heading(format!("Sort {}", file_name(&view.input)));
                let pages = &view.plan.scan.pages;
                let edited = sort::parse_edit(&view.order, pages);
                let groups = edited.as_ref().unwrap_or(&view.plan.groups);
                for (g, group) in groups.iter().enumerate() {
                    let title = view
                        .plan
                        .groups
                        .get(g)
                        .map(|p| p.title.as_str())
                        .filter(|t| !t.is_empty())
                        .unwrap_or("(no title)");
                    ui.strong(format!("Document {}: {title}", g + 1));
                    for &n in &group.pages {
                        let page = &pages[n as usize - 1];
                        let printed = page
                            .printed_number
                            .as_ref()
                            .map(|p| format!("[{p}] "))
                            .unwrap_or_default();
                        ui.monospace(format!(
                            "  page {n:>3}  {printed}{}",
                            pages::preview(&page.text, 80)
                        ));
                    }
                }
                for group in &view.plan.groups {
                    if !group.missing.is_empty() {
                        ui.colored_label(
                            ui.visuals().warn_fg_color,
                            format!(
                                "Printed page {} is not in the scan.",
                                pages::join_numbers(&group.missing)
                            ),
                        );
                    }
                }
                let duplicates: Vec<String> = pages
                    .iter()
                    .filter(|p| !groups.iter().any(|g| g.pages.contains(&p.number)))
                    .filter_map(|p| {
                        p.duplicate_of
                            .map(|o| format!("{} (same as {o})", p.number))
                    })
                    .collect();
                if !duplicates.is_empty() {
                    ui.weak(format!(
                        "Duplicate scans, left out: {}",
                        duplicates.join(", ")
                    ));
                }
                ui.add_space(6.0);
                ui.label("Page order (';' separates documents; blank pages and duplicates may be left out):");
                ui.add(egui::TextEdit::singleline(&mut view.order).desired_width(f32::INFINITY));
                if let Err(e) = &edited {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                let blank = sort::leftover_blank(groups, pages);
                if !blank.is_empty() {
                    ui.checkbox(
                        &mut view.delete_blank,
                        format!(
                            "Delete blank page(s) {} (otherwise they go to the end)",
                            pages::join_numbers(&blank)
                        ),
                    );
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            edited.is_ok(),
                            egui::Button::new(format!("Write {}", file_name(&view.plan.out))),
                        )
                        .clicked()
                    {
                        click = Some(Click::WriteSort);
                    }
                    if ui.button("Cancel").clicked() {
                        click = Some(Click::Close);
                    }
                });
            }
            Pane::Check(results) => {
                ui.heading(match results.len() {
                    1 => format!("Check {}", file_name(&results[0].0)),
                    n => format!("Check of {n} files"),
                });
                let several = results.len() > 1;
                for (file, result) in results.iter() {
                    if several {
                        ui.add_space(8.0);
                        ui.strong(file_name(file));
                    }
                    let diagnosis = match result {
                        Ok(diagnosis) => diagnosis,
                        Err(e) => {
                            ui.colored_label(ui.visuals().error_fg_color, e);
                            continue;
                        }
                    };
                    for line in &diagnosis.lines {
                        ui.monospace(line);
                    }
                    ui.horizontal(|ui| {
                        if !diagnosis.readable_name
                            && ui.add_enabled(idle, egui::Button::new("Name it")).clicked()
                        {
                            click = Some(Click::Name(file.clone(), Some(diagnosis.text())));
                        }
                        if ui.add_enabled(idle, egui::Button::new("Sort it")).clicked() {
                            click = Some(Click::Sort(file.clone()));
                        }
                        if !diagnosis.in_box && ui.button("Add to box").clicked() {
                            click = Some(Click::AddToBox(file.clone()));
                        }
                    });
                }

                // For several files: the fixes that work on all of them at once.
                let ok = || {
                    results
                        .iter()
                        .filter_map(|(f, r)| Some((f, r.as_ref().ok()?)))
                };
                let unnamed: Vec<(PathBuf, Option<String>)> = ok()
                    .filter(|(_, d)| !d.readable_name)
                    .map(|(f, d)| (f.clone(), Some(d.text())))
                    .collect();
                let not_in_box: Vec<PathBuf> = ok()
                    .filter(|(_, d)| !d.in_box)
                    .map(|(f, _)| f.clone())
                    .collect();
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if several
                        && unnamed.len() > 1
                        && ui
                            .add_enabled(
                                idle,
                                egui::Button::new(format!("Name all {} unreadable", unnamed.len())),
                            )
                            .clicked()
                    {
                        click = Some(Click::NameAll(unnamed.clone()));
                    }
                    if several
                        && not_in_box.len() > 1
                        && ui
                            .button(format!("Add all {} to the box", not_in_box.len()))
                            .clicked()
                    {
                        click = Some(Click::AddAllToBox(not_in_box.clone()));
                    }
                    if ui.button("Close").clicked() {
                        click = Some(Click::Close);
                    }
                });
            }
            Pane::Done {
                heading,
                originals,
                offer_trash,
            } => {
                ui.heading(heading.as_str());
                ui.label("From:");
                for file in originals.iter() {
                    ui.label(format!("  {}", file_name(file)));
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if *offer_trash && ui.button("Move the originals to the Trash").clicked() {
                        click = Some(Click::TrashOriginals);
                    }
                    if ui.button("Keep them").clicked() {
                        click = Some(Click::Close);
                    }
                });
            }
        }
        click
    }

    fn find_tab(&mut self, ui: &mut egui::Ui) {
        let idle = self.busy.is_none();
        let mut search = false;
        ui.horizontal(|ui| {
            let field = ui.add_enabled(
                idle,
                egui::TextEdit::singleline(&mut self.question)
                    .hint_text("Ask in plain language, e.g. Wo ist mein Mietvertrag?")
                    .desired_width(ui.available_width() - 160.0),
            );
            if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                search = true;
            }
            if ui.add_enabled(idle, egui::Button::new("Search")).clicked() {
                search = true;
            }
            if self.results.is_some() && ui.button("Show all").clicked() {
                self.results = None;
                self.status = None;
            }
        });
        if search {
            self.start_search(ui.ctx());
        }
        ui.separator();

        let (heading, rows) = match &self.results {
            Some((question, rows)) => (format!("Matches for “{question}”"), rows),
            None => (format!("All documents ({})", self.all.len()), &self.all),
        };
        ui.strong(heading);
        if rows.is_empty() {
            ui.weak(if self.results.is_some() {
                "Nothing found."
            } else {
                "The box is empty. Open PDFs in Files and choose Add to box."
            });
        }
        let mut problem = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            for row in rows {
                if let Err(e) = document_row(ui, &self.ws, row) {
                    problem = Some(e);
                }
            }
        });
        if let Some(e) = problem {
            self.error(e);
        }
    }

    fn add_tab(&mut self, ui: &mut egui::Ui) {
        if !self.queue.is_empty() {
            let names: Vec<String> = self.queue.iter().map(|f| file_name(f)).collect();
            ui.weak(format!("Waiting: {}", names.join(", ")));
            ui.separator();
        }
        let Some((proposal, entry)) = &mut self.current else {
            if self.busy.is_none() {
                ui.weak("Nothing to review. Open PDFs in Files and choose Add to box.");
            }
            return;
        };
        ui.heading(file_name(&proposal.file));
        ui.weak("Check the proposal, change what is wrong, then save.");
        ui.add_space(6.0);
        egui::Grid::new("entry")
            .num_columns(2)
            .spacing([12.0, 8.0])
            .show(ui, |ui| {
                ui.label("Date");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut entry.date).desired_width(120.0));
                    if !entry.has_valid_date() {
                        ui.weak("YYYY-MM-DD or unknown");
                    }
                });
                ui.end_row();
                ui.label("Tags");
                ui.add(
                    egui::TextEdit::singleline(&mut entry.tags)
                        .hint_text("comma, separated")
                        .desired_width(f32::INFINITY),
                );
                ui.end_row();
                ui.label("Summary");
                ui.add(
                    egui::TextEdit::multiline(&mut entry.summary)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                );
                ui.end_row();
            });
        ui.add_space(8.0);
        let (mut save, mut skip) = (false, false);
        ui.horizontal(|ui| {
            save = ui.button("Save to box").clicked();
            skip = ui.button("Skip").clicked();
            if ui.button("Open PDF").clicked()
                && let Err(e) = open(&proposal.file, false)
            {
                self.status = Some((e, true));
            }
        });
        if save {
            let (proposal, entry) = self.current.take().expect("checked above");
            match add::save(&self.ws, &proposal, &entry) {
                Ok(_) => {
                    self.info(format!("Added {}.", file_name(&proposal.file)));
                    self.reload();
                }
                Err(e) => {
                    self.error(e.to_string());
                    self.current = Some((proposal, entry));
                }
            }
        } else if skip {
            let (proposal, _) = self.current.take().expect("checked above");
            self.info(format!("Skipped {}.", file_name(&proposal.file)));
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.receive();
        let ctx = ui.ctx().clone();
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if !dropped.is_empty() {
            self.open_files(dropped);
        }
        self.next_proposal(&ctx);

        egui::Panel::top("tabs").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.tab, Tab::Files, "Files");
                ui.selectable_value(&mut self.tab, Tab::Find, "Find");
                let add_label = match self.queue.len() + self.current.is_some() as usize {
                    0 => "Add".to_string(),
                    n => format!("Add ({n})"),
                };
                ui.selectable_value(&mut self.tab, Tab::Add, add_label);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.weak(format!(
                        "{} · {}",
                        self.ws.root().display(),
                        self.config.model
                    ));
                });
            });
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(what) = &self.busy {
                    ui.spinner();
                    ui.label(what);
                } else if let Some((text, is_error)) = &self.status {
                    if *is_error {
                        ui.colored_label(ui.visuals().error_fg_color, text);
                    } else {
                        ui.label(text);
                    }
                } else {
                    ui.weak("Ready.");
                }
            });
        });
        egui::CentralPanel::default_margins().show(ui, |ui| match self.tab {
            Tab::Files => self.files_tab(ui),
            Tab::Find => self.find_tab(ui),
            Tab::Add => self.add_tab(ui),
        });
    }
}

/// An action button with a tooltip.
fn action(ui: &mut egui::Ui, enabled: bool, label: &str, hint: &str) -> egui::Response {
    ui.add_enabled(enabled, egui::Button::new(label))
        .on_hover_text(hint)
        .on_disabled_hover_text(hint)
}

/// One document: date, summary, tags and buttons to look at it.
fn document_row(ui: &mut egui::Ui, ws: &Workspace, row: &Row) -> Result<(), String> {
    let path = store::path(ws, &row.hash);
    let mut result = Ok(());
    ui.horizontal(|ui| {
        ui.monospace(&row.date);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(&row.summary).strong());
            ui.weak(format!("{}  ·  {}", row.tags, row.original_name));
            ui.horizontal(|ui| {
                if ui.small_button("Open").clicked() {
                    result = open(&path, false);
                }
                if ui.small_button("Show in Finder").clicked() {
                    result = open(&path, true);
                }
            });
        });
    });
    ui.add_space(6.0);
    result
}

/// Opens the file in its default app (Preview for PDFs), or reveals it in Finder.
fn open(path: &Path, reveal: bool) -> Result<(), String> {
    if !path.exists() {
        return Err(format!("{} does not exist", path.display()));
    }
    let mut command = std::process::Command::new("open");
    if reveal {
        command.arg("-R");
    }
    command
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("cannot open {}: {e}", path.display()))
}

/// The typed name without a trailing `.pdf`, slashes or surrounding spaces.
fn naming_stem(typed: &str) -> String {
    let typed = typed.trim();
    let typed = typed
        .strip_suffix(".pdf")
        .or_else(|| typed.strip_suffix(".PDF"))
        .unwrap_or(typed);
    typed.replace(['/', ':'], "-").trim().to_string()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
