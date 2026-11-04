//! `gui`: a small window for the two everyday jobs, finding and adding.
//!
//! The window only shows and asks; the work is done by the same functions the
//! command line uses (`find::search`, `add::prepare`, `add::save`). Anything that
//! talks to the model runs on a background thread, so the window never freezes.

use crate::{
    Res,
    add::{self, Prepared, Proposal},
    config::Config,
    entry::Entry,
    find, index,
    index::Row,
    llm::Llm,
    store,
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
            .with_inner_size([860.0, 640.0]),
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
    Find,
    Add,
}

/// Results coming back from a background job.
enum Msg {
    Found(Result<find::Found, String>),
    Prepared(PathBuf, Result<Prepared, String>),
}

struct App {
    ws: Workspace,
    config: Config,
    tab: Tab,
    /// All documents, newest first (shown when there is no search result).
    all: Vec<Row>,
    question: String,
    /// The last search: the question and its matches.
    results: Option<(String, Vec<Row>)>,
    /// Files waiting for a proposal.
    queue: VecDeque<PathBuf>,
    /// The proposal I am looking at, and my edited version of it.
    current: Option<(Proposal, Entry)>,
    path_input: String,
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
            tab: Tab::Find,
            all: Vec::new(),
            question: String::new(),
            results: None,
            queue: VecDeque::new(),
            current: None,
            path_input: String::new(),
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

    /// Starts the next queued file, if nothing else is going on.
    fn next_proposal(&mut self, ctx: &egui::Context) {
        if self.busy.is_some() || self.current.is_some() {
            return;
        }
        let Some(file) = self.queue.pop_front() else {
            return;
        };
        let name = file_name(&file);
        let what = format!("Reading {name} and asking the model (OCR can take a while)…");
        self.spawn(ctx, &what, move |ws, config, llm| async move {
            let result = add::prepare(&config, &ws, &llm, &file)
                .await
                .map_err(|e| e.to_string());
            Msg::Prepared(file, result)
        });
    }

    fn receive(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            self.busy = None;
            match msg {
                Msg::Found(Ok(found)) => {
                    match (&found.note, found.rows.len()) {
                        (Some(note), _) => self.info(note.clone()),
                        (None, 0) => self.info("No matching document found."),
                        (None, n) => self.info(format!("{n} matching documents.")),
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
            }
        }
    }

    fn queue_files(&mut self, files: impl IntoIterator<Item = PathBuf>) {
        let mut added = 0;
        for file in files {
            let is_pdf = file
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("pdf"));
            if !is_pdf || !file.is_file() {
                self.error(format!("{} is not a PDF file.", file.display()));
                continue;
            }
            if !self.queue.contains(&file) {
                self.queue.push_back(file);
                added += 1;
            }
        }
        if added > 0 {
            self.tab = Tab::Add;
        }
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
                "The box is empty. Drop PDFs on this window to add them."
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
        ui.label("Drop PDFs on this window, or enter a path:");
        let mut add_path = false;
        ui.horizontal(|ui| {
            let field = ui.add(
                egui::TextEdit::singleline(&mut self.path_input)
                    .hint_text("/Users/…/scan.pdf")
                    .desired_width(ui.available_width() - 60.0),
            );
            if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                add_path = true;
            }
            if ui.button("Add").clicked() {
                add_path = true;
            }
        });
        if add_path && !self.path_input.trim().is_empty() {
            let path = PathBuf::from(self.path_input.trim());
            self.path_input.clear();
            self.queue_files([path]);
        }
        if !self.queue.is_empty() {
            let names: Vec<String> = self.queue.iter().map(|f| file_name(f)).collect();
            ui.weak(format!("Waiting: {}", names.join(", ")));
        }
        ui.separator();

        let Some((proposal, entry)) = &mut self.current else {
            if self.busy.is_none() {
                ui.weak("Nothing to review.");
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
        self.queue_files(dropped);
        self.next_proposal(&ctx);

        egui::Panel::top("tabs").show(ui, |ui| {
            ui.horizontal(|ui| {
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
            Tab::Find => self.find_tab(ui),
            Tab::Add => self.add_tab(ui),
        });
    }
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

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}
