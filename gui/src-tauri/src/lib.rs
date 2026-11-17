//! Desktop app for Smart Shoebox: Tauri commands around the `smart_shoebox` library.
//!
//! The window only shows and asks; the work is done by the same functions the
//! command line uses (`add::prepare`, `rename::propose`, `sort::propose`, …).
//!
//! - Slow jobs (OCR, the model) run one at a time, each on its own thread with
//!   its own model client, because the model's futures are not `Send`.
//!   `cancel_job` ends the waiting command at once; the thread stops at its next
//!   pause and its result is dropped.
//! - Results the window acts on later (a check, a sort plan, an add proposal)
//!   stay here; the window gets a small view of them as JSON.
//!
//! The box: `SHOEBOX_DIR`, else the current folder and above (like `shoebox`),
//! else the box chosen last time, else the window asks for one.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use smart_shoebox::{
    Res, add,
    add::{Prepared, Proposal},
    ask::{self, Outcome},
    complete::{self, Candidate, Diagnosis},
    config::{self, Config},
    entry::Entry,
    find, index,
    index::Row,
    llm::Llm,
    merge, naming, pages,
    pdf::TempFile,
    rename,
    retrieve::Searchable,
    sort::{self, Group, SortPlan},
    store, trash,
    vectors::OllamaEmbedder,
    wizard::{self, Status, Summary},
    workspace::Workspace,
};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::{oneshot, watch};

type CmdRes<T> = Result<T, String>;

/// The error text of a cancelled job; the window shows it as information.
const CANCELLED: &str = "Cancelled.";

fn text(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn path_text(path: &Path) -> String {
    path.display().to_string()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn is_pdf(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

// ---------------------------------------------------------------------------------------------
// State

/// For sorting a scan merged with its counterpart: the temporary merged PDF
/// (kept until the result is written), the two originals and what is still missing.
#[derive(Default)]
struct SortExtra {
    /// Only held, so the merged PDF exists until the result is written.
    _temp: Option<TempFile>,
    originals: Vec<PathBuf>,
    still: Vec<u32>,
}

struct SortSession {
    input: PathBuf,
    plan: SortPlan,
    extra: SortExtra,
}

struct CounterpartSession {
    file: PathBuf,
    diagnosis: Diagnosis,
    candidates: Vec<Candidate>,
}

#[derive(Default)]
struct Shared {
    /// The open box and its settings.
    open: Mutex<Option<(Workspace, Config)>>,
    /// Why no box could be opened at the start, if so.
    problem: Mutex<Option<String>>,
    /// The running job: its number and the switch that cancels it.
    job: Mutex<Option<(u64, watch::Sender<bool>)>>,
    next_id: AtomicU64,
    /// The last check, by file, so naming and counterpart search need no second OCR.
    checked: Mutex<HashMap<PathBuf, Diagnosis>>,
    sort: Mutex<Option<SortSession>>,
    counterparts: Mutex<Option<CounterpartSession>>,
    proposals: Mutex<HashMap<u64, Proposal>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    // A panic in another command must not lock the window out for good.
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Shared {
    fn id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn current(&self) -> CmdRes<(Workspace, Config)> {
        lock(&self.open)
            .clone()
            .ok_or_else(|| "No shoebox is open.".to_string())
    }

    fn is_running(&self, id: u64) -> bool {
        lock(&self.job).as_ref().is_some_and(|(i, _)| *i == id)
    }
}

/// Runs `job` on its own thread with its own model client and returns its
/// result, or `CANCELLED` as soon as `cancel_job` is called.
async fn run_job<T, F, Fut>(state: &Shared, job: F) -> CmdRes<T>
where
    T: Send + 'static,
    F: FnOnce(Workspace, Config, Llm) -> Fut + Send + 'static,
    Fut: Future<Output = Res<T>>,
{
    run_job_with_id(state, |_| job).await
}

/// Like `run_job`; `make` gets the job's number first (for progress events).
async fn run_job_with_id<T, M, F, Fut>(state: &Shared, make: M) -> CmdRes<T>
where
    T: Send + 'static,
    M: FnOnce(u64) -> F,
    F: FnOnce(Workspace, Config, Llm) -> Fut + Send + 'static,
    Fut: Future<Output = Res<T>>,
{
    let (ws, config) = state.current()?;
    let (cancel_tx, cancel_rx) = watch::channel(false);
    let id = {
        let mut slot = lock(&state.job);
        if slot.is_some() {
            return Err("Another job is still running.".into());
        }
        let id = state.id();
        *slot = Some((id, cancel_tx));
        id
    };
    let job = make(id);
    let (done_tx, done_rx) = oneshot::channel();
    let mut thread_cancel = cancel_rx.clone();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("cannot start the async runtime");
        let llm = Llm::new(&config);
        // Cancelling drops the job at its next pause (e.g. while waiting for
        // the model), which also ends the request to Ollama. A step that is
        // already running (such as OCR) finishes first, its result is dropped.
        let result = runtime.block_on(async {
            tokio::select! {
                biased;
                _ = thread_cancel.wait_for(|c| *c) => None,
                r = job(ws, config, llm) => Some(r.map_err(text)),
            }
        });
        let _ = done_tx.send(result);
    });

    let mut cancel_rx = cancel_rx;
    let result = tokio::select! {
        r = done_rx => r.ok().flatten(),
        _ = cancel_rx.wait_for(|c| *c) => None,
    };
    let mut slot = lock(&state.job);
    if slot.as_ref().is_some_and(|(i, _)| *i == id) {
        *slot = None;
    }
    result.unwrap_or_else(|| Err(CANCELLED.into()))
}

#[tauri::command]
fn cancel_job(state: State<'_, Shared>) {
    if let Some((_, cancel)) = lock(&state.job).take() {
        let _ = cancel.send(true);
    }
}

// ---------------------------------------------------------------------------------------------
// The box

#[derive(Serialize)]
struct BoxInfo {
    root: String,
    model: String,
}

#[derive(Serialize)]
struct BoxState {
    open: Option<BoxInfo>,
    problem: Option<String>,
}

fn box_info(ws: &Workspace, config: &Config) -> BoxInfo {
    BoxInfo {
        root: path_text(ws.root()),
        model: config.model.clone(),
    }
}

/// Where the last box is remembered: a one-line file next to other app data.
fn memory_file() -> Option<PathBuf> {
    let home = env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/Smart Shoebox/box-path"))
}

fn remembered() -> Option<Workspace> {
    let text = fs::read_to_string(memory_file()?).ok()?;
    Workspace::open(&PathBuf::from(text.trim()))
}

/// Best effort: if it cannot be saved, I am simply asked again next time.
fn remember(ws: &Workspace) {
    let Some(file) = memory_file() else {
        return;
    };
    if let Some(dir) = file.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let root = fs::canonicalize(ws.root()).unwrap_or_else(|_| ws.root().to_path_buf());
    let _ = fs::write(file, root.to_string_lossy().as_bytes());
}

/// Loads the settings, makes the box the open one and remembers it.
fn use_box(state: &Shared, ws: Workspace) -> CmdRes<BoxInfo> {
    let config = config::load(&ws.config_path()).map_err(text)?;
    remember(&ws);
    let info = box_info(&ws, &config);
    *lock(&state.open) = Some((ws, config));
    *lock(&state.problem) = None;
    lock(&state.checked).clear();
    *lock(&state.sort) = None;
    *lock(&state.counterparts) = None;
    lock(&state.proposals).clear();
    Ok(info)
}

/// At the start: `SHOEBOX_DIR` or the current folder and above, like `shoebox`;
/// otherwise the box used last time.
fn find_box(state: &Shared) {
    let found = if env::var_os("SHOEBOX_DIR").is_some() {
        Workspace::locate().map_err(text)
    } else {
        Workspace::locate()
            .ok()
            .or_else(remembered)
            .ok_or_else(String::new)
    };
    let problem = match found.and_then(|ws| use_box(state, ws)) {
        Ok(_) => None,
        Err(e) if e.is_empty() => None,
        Err(e) => Some(e),
    };
    *lock(&state.problem) = problem;
}

#[tauri::command]
fn box_state(state: State<'_, Shared>) -> BoxState {
    let open = lock(&state.open).as_ref().map(|(ws, c)| box_info(ws, c));
    BoxState {
        open,
        problem: lock(&state.problem).clone(),
    }
}

/// Opens the box in `path` (or its `box/` subfolder). `None` if there is none.
#[tauri::command]
fn open_box(state: State<'_, Shared>, path: String) -> CmdRes<Option<BoxInfo>> {
    match Workspace::open(Path::new(&path)) {
        Some(ws) => use_box(&state, ws).map(Some),
        None => Ok(None),
    }
}

#[tauri::command]
fn create_box(state: State<'_, Shared>, path: String) -> CmdRes<BoxInfo> {
    let ws = Workspace::init(Path::new(&path)).map_err(text)?;
    use_box(&state, ws)
}

// ---------------------------------------------------------------------------------------------
// Files

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Pdf,
    Folder,
    Other,
}

#[derive(Serialize)]
struct PathKind {
    path: String,
    kind: Kind,
}

/// What each dropped or chosen path is.
#[tauri::command]
fn classify(paths: Vec<String>) -> Vec<PathKind> {
    paths
        .into_iter()
        .map(|path| {
            let p = Path::new(&path);
            let kind = if p.is_dir() {
                Kind::Folder
            } else if p.is_file() && is_pdf(p) {
                Kind::Pdf
            } else {
                Kind::Other
            };
            PathKind { path, kind }
        })
        .collect()
}

/// Opens the file in its default app (Preview for PDFs), or reveals it in Finder.
#[tauri::command]
fn open_file(path: String, reveal: bool) -> CmdRes<()> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err(format!("{path} does not exist"));
    }
    let result = if reveal {
        tauri_plugin_opener::reveal_item_in_dir(p)
    } else {
        tauri_plugin_opener::open_path(p, None::<&str>)
    };
    result.map_err(|e| format!("cannot open {path}: {e}"))
}

#[derive(Serialize)]
struct Trashed {
    moved: Vec<String>,
    errors: Vec<String>,
}

#[tauri::command]
fn trash(paths: Vec<String>) -> Trashed {
    let mut out = Trashed {
        moved: Vec::new(),
        errors: Vec::new(),
    };
    for path in paths {
        match trash::move_to_trash(Path::new(&path)) {
            Ok(_) => out.moved.push(path),
            Err(e) => out.errors.push(format!("{}: {e}", file_name(Path::new(&path)))),
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Documents: list, find, ask

#[derive(Serialize)]
struct Doc {
    hash: String,
    date: String,
    tags: String,
    summary: String,
    original_name: String,
    path: String,
}

fn doc(ws: &Workspace, row: Row) -> Doc {
    Doc {
        path: path_text(&store::path(ws, &row.hash)),
        hash: row.hash,
        date: row.date,
        tags: row.tags,
        summary: row.summary,
        original_name: row.original_name,
    }
}

/// All documents, newest first.
#[tauri::command]
fn documents(state: State<'_, Shared>) -> CmdRes<Vec<Doc>> {
    let (ws, _) = state.current()?;
    let text = index::read(&ws.index_path()).map_err(text)?;
    let mut rows = index::parse(&text);
    rows.sort_by(|a, b| b.date.cmp(&a.date));
    Ok(rows.into_iter().map(|r| doc(&ws, r)).collect())
}

#[derive(Serialize)]
struct FoundView {
    rows: Vec<Doc>,
    note: Option<String>,
}

#[tauri::command]
async fn search(state: State<'_, Shared>, question: String) -> CmdRes<FoundView> {
    run_job(&state, move |ws, config, llm| async move {
        let found = find::search(&config, &ws, &llm, &question).await?;
        Ok(FoundView {
            rows: found.rows.into_iter().map(|r| doc(&ws, r)).collect(),
            note: found.note,
        })
    })
    .await
}

#[derive(Serialize)]
struct SourceView {
    name: String,
    page: u32,
    quote: String,
    path: String,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum AskOutcome {
    Answered {
        answer: String,
        sources: Vec<SourceView>,
    },
    NotFound {
        why: String,
        claimed: Option<String>,
    },
}

#[derive(Serialize)]
struct Asked {
    question: String,
    outcome: AskOutcome,
    /// Documents without page text, which were not searched.
    without_text: usize,
}

#[tauri::command]
async fn ask(state: State<'_, Shared>, question: String) -> CmdRes<Asked> {
    run_job(&state, move |ws, config, llm| async move {
        let searchable = Searchable::load(&ws)?;
        if searchable.chunks.is_empty() {
            return Err("No page text to search yet (add documents, or run `shoebox text`).".into());
        }
        let embedder = OllamaEmbedder::new(&config);
        let outcome = ask::answer(&ws, &llm, &embedder, &searchable, &question).await?;
        let outcome = match outcome {
            Outcome::Answered { answer, sources } => AskOutcome::Answered {
                answer,
                sources: sources
                    .into_iter()
                    .map(|s| SourceView {
                        name: searchable
                            .row(&s.chunk.hash)
                            .map_or("?".to_string(), |r| r.original_name.clone()),
                        page: s.chunk.page,
                        path: path_text(&store::path(&ws, &s.chunk.hash)),
                        quote: s.quote,
                    })
                    .collect(),
            },
            Outcome::NotFound { why, claimed } => AskOutcome::NotFound { why, claimed },
        };
        Ok(Asked {
            question,
            outcome,
            without_text: searchable.without_text.len(),
        })
    })
    .await
}

// ---------------------------------------------------------------------------------------------
// Wizard

#[derive(Serialize, Deserialize, Clone)]
struct EntryView {
    date: String,
    tags: String,
    summary: String,
}

impl From<&Entry> for EntryView {
    fn from(e: &Entry) -> Self {
        EntryView {
            date: e.date.clone(),
            tags: e.tags.clone(),
            summary: e.summary.clone(),
        }
    }
}

impl From<EntryView> for Entry {
    fn from(e: EntryView) -> Self {
        Entry {
            date: e.date,
            tags: e.tags,
            summary: e.summary,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "snake_case")]
enum ItemStatus {
    Added,
    AlreadyInBox,
    Skipped,
    NeedsYou,
}

#[derive(Serialize)]
struct WizardItem {
    file: String,
    name: String,
    steps: Vec<String>,
    status: ItemStatus,
    /// Why it was skipped or needs me.
    reason: Option<String>,
    entry: Option<EntryView>,
    line: String,
}

#[derive(Serialize)]
struct WizardSummary {
    items: Vec<WizardItem>,
    /// Originals merged into documents that are now in the box.
    spare_originals: Vec<String>,
}

fn wizard_summary(summary: &Summary) -> WizardSummary {
    let items = summary
        .items
        .iter()
        .map(|item| {
            let (status, reason, entry) = match &item.status {
                Status::Added { entry, .. } => (ItemStatus::Added, None, Some(entry.into())),
                Status::AlreadyInBox => (ItemStatus::AlreadyInBox, None, None),
                Status::Skipped(why) => (ItemStatus::Skipped, Some(why.clone()), None),
                Status::NeedsYou(why) => (ItemStatus::NeedsYou, Some(why.clone()), None),
            };
            WizardItem {
                file: path_text(&item.file),
                name: file_name(&item.file),
                steps: item.steps.clone(),
                status,
                reason,
                entry,
                line: item.line(),
            }
        })
        .collect();
    WizardSummary {
        items,
        spare_originals: summary.spare_originals().iter().map(|p| path_text(p)).collect(),
    }
}

/// How many PDFs the inputs hold (folders are searched).
#[tauri::command]
fn count_pdfs(inputs: Vec<String>) -> CmdRes<usize> {
    let inputs: Vec<PathBuf> = inputs.into_iter().map(PathBuf::from).collect();
    wizard::pdfs_in(&inputs).map(|p| p.len()).map_err(text)
}

/// Handles the pile. Each progress line comes as a `wizard-log` event.
#[tauri::command]
async fn run_wizard(
    app: AppHandle,
    state: State<'_, Shared>,
    inputs: Vec<String>,
) -> CmdRes<WizardSummary> {
    let inputs: Vec<PathBuf> = inputs.into_iter().map(PathBuf::from).collect();
    run_job_with_id(&state, move |id| {
        move |ws, config, llm| async move {
            let mut log = |line: String| {
                // A cancelled run may still finish a step; its lines are dropped.
                if app.state::<Shared>().is_running(id) {
                    let _ = app.emit("wizard-log", line);
                }
            };
            let summary = wizard::handle(&config, &ws, &llm, &inputs, &mut log).await?;
            Ok(wizard_summary(&summary))
        }
    })
    .await
}

// ---------------------------------------------------------------------------------------------
// Files: check, name, merge, sort, counterpart

#[derive(Serialize)]
struct CheckView {
    file: String,
    name: String,
    error: Option<String>,
    lines: Vec<String>,
    incomplete: bool,
    readable_name: bool,
    in_box: bool,
}

fn check_view(file: &Path, result: Result<&Diagnosis, &str>) -> CheckView {
    let mut view = CheckView {
        file: path_text(file),
        name: file_name(file),
        error: None,
        lines: Vec::new(),
        incomplete: false,
        readable_name: true,
        in_box: true,
    };
    match result {
        Ok(d) => {
            view.lines = d.lines.clone();
            view.incomplete = d.report.is_incomplete();
            view.readable_name = d.readable_name;
            view.in_box = d.in_box;
        }
        Err(e) => view.error = Some(e.to_string()),
    }
    view
}

/// Pages complete? Name readable? In the box? Reads (and OCRs) each file. No AI.
#[tauri::command]
async fn check(state: State<'_, Shared>, files: Vec<String>) -> CmdRes<Vec<CheckView>> {
    let results = run_job(&state, move |ws, config, _llm| async move {
        Ok(files
            .into_iter()
            .map(|f| {
                let file = PathBuf::from(f);
                let diagnosis = complete::diagnose(&config, &ws, &file).map_err(text);
                (file, diagnosis)
            })
            .collect::<Vec<_>>())
    })
    .await?;
    let mut checked = lock(&state.checked);
    checked.clear();
    let mut views = Vec::new();
    for (file, result) in results {
        views.push(check_view(&file, result.as_ref().map_err(String::as_str)));
        if let Ok(diagnosis) = result {
            checked.insert(file, diagnosis);
        }
    }
    Ok(views)
}

#[derive(Serialize)]
struct NameView {
    file: String,
    /// The current name, without `.pdf`.
    current: String,
    suggestion: String,
    error: Option<String>,
}

/// Proposes readable names. Files from the last check are not read again.
#[tauri::command]
async fn propose_names(state: State<'_, Shared>, files: Vec<String>) -> CmdRes<Vec<NameView>> {
    let files: Vec<(PathBuf, Option<String>)> = {
        let checked = lock(&state.checked);
        files
            .into_iter()
            .map(PathBuf::from)
            .map(|f| {
                let text = checked.get(&f).map(Diagnosis::text);
                (f, text)
            })
            .collect()
    };
    run_job(&state, move |ws, config, llm| async move {
        let mut views = Vec::new();
        for (file, text) in files {
            let view = match rename::propose(&config, &ws, &llm, &file, text.as_deref()).await {
                Ok(p) => NameView {
                    file: path_text(&p.file),
                    current: p.current,
                    suggestion: p.suggestion,
                    error: p
                        .model_error
                        .map(|e| format!("The model gave no usable answer ({e}); type a name.")),
                },
                Err(e) => NameView {
                    file: path_text(&file),
                    current: file_name(&file),
                    suggestion: String::new(),
                    error: Some(e.to_string()),
                },
            };
            views.push(view);
        }
        Ok(views)
    })
    .await
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

/// Renames the file. Returns the new path, or `None` if the name is unchanged.
#[tauri::command]
fn rename_file(file: String, stem: String) -> CmdRes<Option<String>> {
    let file = PathBuf::from(file);
    let stem = naming_stem(&stem);
    if stem.is_empty() {
        return Err("The name is empty.".into());
    }
    if file.file_stem().is_some_and(|s| s.to_string_lossy() == stem) {
        return Ok(None);
    }
    rename::apply(&file, &stem)
        .map(|dest| Some(path_text(&dest)))
        .map_err(text)
}

#[derive(Serialize)]
struct MergedView {
    out: String,
    pages: usize,
    expected: usize,
    complete: bool,
    originals: Vec<String>,
}

#[tauri::command]
fn merge_files(files: Vec<String>) -> CmdRes<MergedView> {
    let files: Vec<PathBuf> = files.into_iter().map(PathBuf::from).collect();
    let merged = merge::merge(&files, None).map_err(text)?;
    Ok(MergedView {
        out: path_text(&merged.out),
        pages: merged.pages,
        expected: merged.expected,
        complete: merged.complete,
        originals: merge::originals(&files).iter().map(|p| path_text(p)).collect(),
    })
}

#[derive(Serialize)]
struct PageView {
    number: u32,
    printed: Option<String>,
    preview: String,
}

#[derive(Serialize)]
struct DocumentView {
    title: String,
    pages: Vec<PageView>,
}

/// The documents as the page order says, and the notes around them.
#[derive(Serialize)]
struct SortLayout {
    /// Why the typed order is not valid; the AI's order is shown then.
    error: Option<String>,
    documents: Vec<DocumentView>,
    /// "Printed page 4 is not in the scan."
    missing: Vec<String>,
    /// Duplicate scans that are left out, e.g. "5 (same as 2)".
    duplicates: Vec<String>,
    /// Blank pages that are not placed: deleted, or put at the end.
    blank: Vec<u32>,
}

#[derive(Serialize)]
struct SortView {
    heading: String,
    out_name: String,
    /// The AI's page order, e.g. `1,3,2; 4,5`.
    order: String,
    layout: SortLayout,
}

fn sort_layout(plan: &SortPlan, order: &str) -> SortLayout {
    let pages = &plan.scan.pages;
    let edited = sort::parse_edit(order, pages);
    let groups: &[Group] = edited.as_deref().unwrap_or(&plan.groups);
    let documents = groups
        .iter()
        .enumerate()
        .map(|(g, group)| DocumentView {
            title: plan
                .groups
                .get(g)
                .map(|p| p.title.clone())
                .filter(|t| !t.is_empty())
                .unwrap_or_else(|| "(no title)".into()),
            pages: group
                .pages
                .iter()
                .map(|&n| {
                    let page = &pages[n as usize - 1];
                    PageView {
                        number: n,
                        printed: page.printed_number.map(|p| p.to_string()),
                        preview: pages::preview(&page.text, 120),
                    }
                })
                .collect(),
        })
        .collect();
    let missing = plan
        .groups
        .iter()
        .filter(|g| !g.missing.is_empty())
        .map(|g| {
            format!(
                "Printed page {} is not in the scan.",
                pages::join_numbers(&g.missing)
            )
        })
        .collect();
    let duplicates = pages
        .iter()
        .filter(|p| !groups.iter().any(|g| g.pages.contains(&p.number)))
        .filter_map(|p| p.duplicate_of.map(|o| format!("{} (same as {o})", p.number)))
        .collect();
    SortLayout {
        error: edited.as_ref().err().cloned(),
        documents,
        missing,
        duplicates,
        blank: sort::leftover_blank(groups, pages),
    }
}

fn sort_view(session: &SortSession) -> SortView {
    let order = sort::format_groups(&session.plan.groups);
    SortView {
        heading: match session.extra.originals.as_slice() {
            [a, b] => format!("Sort {} + {}", file_name(a), file_name(b)),
            _ => format!("Sort {}", file_name(&session.input)),
        },
        out_name: file_name(&session.plan.out),
        layout: sort_layout(&session.plan, &order),
        order,
    }
}

/// Keeps the plan and returns its view.
fn start_sort_session(state: &Shared, session: SortSession) -> SortView {
    let view = sort_view(&session);
    *lock(&state.sort) = Some(session);
    view
}

#[tauri::command]
async fn propose_sort(state: State<'_, Shared>, file: String) -> CmdRes<SortView> {
    let input = PathBuf::from(file);
    let job_input = input.clone();
    let plan = run_job(&state, move |_ws, config, llm| async move {
        sort::propose(&config, &llm, &job_input, None).await
    })
    .await?;
    Ok(start_sort_session(
        &state,
        SortSession {
            input,
            plan,
            extra: SortExtra::default(),
        },
    ))
}

/// The layout for a page order I typed.
#[tauri::command]
fn sort_preview(state: State<'_, Shared>, order: String) -> CmdRes<SortLayout> {
    let sort = lock(&state.sort);
    let session = sort.as_ref().ok_or("Nothing to sort.")?;
    Ok(sort_layout(&session.plan, &order))
}

#[derive(Serialize)]
struct SortWritten {
    out: String,
    count: usize,
    input_name: String,
    /// The scans it was merged from (empty for a plain sort).
    originals: Vec<String>,
    /// Printed pages that are still missing.
    still: String,
}

#[tauri::command]
fn write_sort(state: State<'_, Shared>, order: String, delete_blank: bool) -> CmdRes<SortWritten> {
    let mut slot = lock(&state.sort);
    let session = slot.as_ref().ok_or("Nothing to sort.")?;
    let groups = sort::parse_edit(&order, &session.plan.scan.pages)?;
    let (out, count) =
        sort::write(&session.plan, &groups, delete_blank).map_err(|e| format!("Not written: {e}"))?;
    let session = slot.take().expect("checked above");
    Ok(SortWritten {
        out: path_text(&out),
        count,
        input_name: file_name(&session.input),
        originals: session.extra.originals.iter().map(|p| path_text(p)).collect(),
        still: pages::join_numbers(&session.extra.still),
    })
}

#[derive(Serialize)]
struct CandidateView {
    path: String,
    name: String,
    description: String,
    /// Pages still missing after merging with it.
    still: String,
}

#[derive(Serialize)]
struct RejectedView {
    path: String,
    name: String,
    reason: String,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CounterpartsView {
    /// No missing pages: nothing to look for.
    Complete { check: CheckView },
    Found {
        name: String,
        dir: String,
        candidates: Vec<CandidateView>,
        rejected: Vec<RejectedView>,
    },
}

/// Looks for the missing pages in the other PDFs of the file's folder.
/// Uses the last check of the file, so it is not OCR'd again.
#[tauri::command]
async fn find_counterparts(state: State<'_, Shared>, file: String) -> CmdRes<CounterpartsView> {
    let file = PathBuf::from(file);
    let known = lock(&state.checked).remove(&file);
    let dir = complete::search_dir(&file, None);
    let (job_file, job_dir) = (file.clone(), dir.clone());
    let (diagnosis, search) = run_job(&state, move |ws, config, _llm| async move {
        let diagnosis = match known {
            Some(d) => d,
            None => complete::diagnose(&config, &ws, &job_file)?,
        };
        let search = if diagnosis.report.is_incomplete() {
            Some(complete::search_counterparts(
                &config,
                &job_file,
                &diagnosis.scan,
                &diagnosis.report,
                &job_dir,
            ))
        } else {
            None
        };
        Ok((diagnosis, search.map(|s| s.map_err(text))))
    })
    .await?;
    let Some(search) = search else {
        let check = check_view(&file, Ok(&diagnosis));
        lock(&state.checked).insert(file, diagnosis);
        return Ok(CounterpartsView::Complete { check });
    };
    let search = match search {
        Ok(s) => s,
        Err(e) => {
            lock(&state.checked).insert(file.clone(), diagnosis);
            return Err(format!("{}: {e}", file_name(&file)));
        }
    };
    let view = CounterpartsView::Found {
        name: file_name(&file),
        dir: path_text(&dir),
        candidates: search
            .candidates
            .iter()
            .map(|c| CandidateView {
                path: path_text(&c.path),
                name: file_name(&c.path),
                description: complete::describe(c),
                still: pages::join_numbers(&c.still),
            })
            .collect(),
        rejected: search
            .rejected
            .iter()
            .map(|(p, reason)| RejectedView {
                path: path_text(p),
                name: file_name(p),
                reason: reason.clone(),
            })
            .collect(),
    };
    *lock(&state.counterparts) = Some(CounterpartSession {
        file,
        diagnosis,
        candidates: search.candidates,
    });
    Ok(view)
}

/// Merges the scan with the chosen counterpart and lets the AI sort both.
#[tauri::command]
async fn merge_with_candidate(state: State<'_, Shared>, index: usize) -> CmdRes<SortView> {
    let CounterpartSession {
        file,
        diagnosis,
        mut candidates,
    } = lock(&state.counterparts)
        .take()
        .ok_or("No counterpart search to use.")?;
    if index >= candidates.len() {
        return Err("No such counterpart.".into());
    }
    let candidate = candidates.swap_remove(index);
    let job_file = file.clone();
    let (temp, plan, candidate) = run_job(&state, move |_ws, config, llm| async move {
        let (temp, out) = complete::merge_with(&job_file, &diagnosis.scan, &candidate)?;
        let plan = sort::propose(&config, &llm, temp.path(), Some(&out)).await?;
        Ok((temp, plan, candidate))
    })
    .await?;
    Ok(start_sort_session(
        &state,
        SortSession {
            input: file.clone(),
            plan,
            extra: SortExtra {
                _temp: Some(temp),
                originals: vec![file, candidate.path],
                still: candidate.still,
            },
        },
    ))
}

// ---------------------------------------------------------------------------------------------
// Add to the box

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PreparedView {
    AlreadyThere {
        name: String,
    },
    Proposal {
        id: u64,
        file: String,
        name: String,
        entry: EntryView,
        model_error: Option<String>,
    },
}

/// Reads the file (OCR if needed) and asks the AI for date, tags and summary.
#[tauri::command]
async fn prepare_add(state: State<'_, Shared>, file: String) -> CmdRes<PreparedView> {
    let file = PathBuf::from(file);
    let job_file = file.clone();
    let prepared = run_job(&state, move |ws, config, llm| async move {
        add::prepare(&config, &ws, &llm, &job_file).await
    })
    .await
    .map_err(|e| {
        if e == CANCELLED {
            e
        } else {
            format!("{}: {e}", file_name(&file))
        }
    })?;
    Ok(match prepared {
        Prepared::AlreadyThere(_) => PreparedView::AlreadyThere {
            name: file_name(&file),
        },
        Prepared::New(proposal) => {
            let id = state.id();
            let view = PreparedView::Proposal {
                id,
                file: path_text(&proposal.file),
                name: file_name(&proposal.file),
                entry: (&proposal.entry).into(),
                model_error: proposal.model_error.clone(),
            };
            lock(&state.proposals).insert(id, proposal);
            view
        }
    })
}

#[tauri::command]
fn save_entry(state: State<'_, Shared>, id: u64, entry: EntryView) -> CmdRes<String> {
    let (ws, _) = state.current()?;
    let proposals = lock(&state.proposals);
    let proposal = proposals.get(&id).ok_or("This proposal is gone.")?;
    let stored = add::save(&ws, proposal, &entry.into()).map_err(text)?;
    drop(proposals);
    lock(&state.proposals).remove(&id);
    Ok(path_text(&stored))
}

#[tauri::command]
fn drop_proposal(state: State<'_, Shared>, id: u64) {
    lock(&state.proposals).remove(&id);
}

#[tauri::command]
fn is_iso_date(date: String) -> bool {
    naming::is_iso_date(&date)
}

// ---------------------------------------------------------------------------------------------

/// Apps started from Finder get a minimal PATH without Homebrew, so `ocrmypdf`
/// would not be found. Adds the usual Homebrew folders if they are missing.
fn add_tool_folders_to_path() {
    let current = env::var_os("PATH").unwrap_or_default();
    let mut paths: Vec<PathBuf> = env::split_paths(&current).collect();
    for dir in ["/opt/homebrew/bin", "/usr/local/bin"] {
        let dir = PathBuf::from(dir);
        if dir.is_dir() && !paths.contains(&dir) {
            paths.push(dir);
        }
    }
    if let Ok(joined) = env::join_paths(paths) {
        // SAFETY: called first thing in `run`, before any other thread exists.
        unsafe { env::set_var("PATH", joined) };
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    add_tool_folders_to_path();
    let shared = Shared::default();
    find_box(&shared);
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(shared)
        .invoke_handler(tauri::generate_handler![
            cancel_job,
            box_state,
            open_box,
            create_box,
            classify,
            open_file,
            trash,
            documents,
            search,
            ask,
            count_pdfs,
            run_wizard,
            check,
            propose_names,
            rename_file,
            merge_files,
            propose_sort,
            sort_preview,
            write_sort,
            find_counterparts,
            merge_with_candidate,
            prepare_add,
            save_entry,
            drop_proposal,
            is_iso_date,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the Smart Shoebox app");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn naming_stem_drops_extension_and_slashes() {
        assert_eq!(naming_stem("  a/b:c.pdf "), "a-b-c");
        assert_eq!(naming_stem("x.PDF"), "x");
        assert_eq!(naming_stem("  "), "");
    }
}
