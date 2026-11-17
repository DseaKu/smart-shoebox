// The state of the window and the actions on it, shared by all components.
//
// Slow work runs in the Rust backend, one job at a time (see `job`). The
// window only shows and asks.

import * as api from "./api";
import type {
  Asked,
  BoxInfo,
  CheckView,
  Counterparts,
  Doc,
  Entry,
  Proposal,
  SortLayout,
  SortView,
  WizardSummary,
} from "./types";

export type Tab = "wizard" | "files" | "find" | "ask" | "add";

export interface Notice {
  kind: "info" | "error";
  text: string;
}

export interface OpenFile {
  path: string;
  selected: boolean;
}

export interface NameItem {
  file: string;
  current: string;
  /** The name I am editing, without `.pdf`. */
  stem: string;
  error: string | null;
  /** Set once renamed or skipped. */
  done: string | null;
}

/** What the Files tab shows below the list: the result of the last action. */
export type Pane =
  | { kind: "empty" }
  | { kind: "names"; items: NameItem[] }
  | { kind: "sort"; view: SortView; order: string; layout: SortLayout; deleteBlank: boolean }
  | { kind: "check"; results: CheckView[] }
  | { kind: "counterparts"; found: Extract<Counterparts, { kind: "found" }> }
  | { kind: "done"; heading: string; originals: string[]; offerTrash: boolean };

class App {
  box = $state<BoxInfo | null>(null);
  boxProblem = $state<string | null>(null);
  tab = $state<Tab>("wizard");
  /** What the running job is doing. */
  busy = $state<string | null>(null);
  notice = $state<Notice | null>(null);

  // Wizard
  wizardInputs = $state<{ path: string; kind: "pdf" | "folder" }[]>([]);
  wizardLog = $state<string[]>([]);
  wizard = $state<WizardSummary | null>(null);

  // Files
  files = $state<OpenFile[]>([]);
  pane = $state<Pane>({ kind: "empty" });

  // Find
  docs = $state<Doc[]>([]);
  question = $state("");
  /** The last search: the question and its matches. */
  results = $state<{ question: string; rows: Doc[] } | null>(null);

  // Ask
  askQuestion = $state("");
  asked = $state<Asked | null>(null);

  // Add
  /** Files waiting for an add proposal. */
  queue = $state<string[]>([]);
  /** The proposal I am looking at, and my edited version of its entry. */
  current = $state<{ proposal: Proposal; entry: Entry } | null>(null);

  selected = $derived(this.files.filter((f) => f.selected).map((f) => f.path));
  addCount = $derived(this.queue.length + (this.current ? 1 : 0));

  info(text: string) {
    this.notice = { kind: "info", text };
  }

  error(text: string) {
    this.notice = { kind: "error", text };
  }

  /**
   * Runs one backend job and shows `what` while it runs. Returns `undefined`
   * if it failed or was cancelled (the notice then says so).
   */
  async job<T>(what: string, work: () => Promise<T>): Promise<T | undefined> {
    if (this.busy) return undefined;
    this.busy = what;
    try {
      return await work();
    } catch (e) {
      const text = api.errorText(e);
      if (text === api.CANCELLED) {
        const waiting = this.queue.length;
        this.queue = [];
        this.info(waiting > 0 ? `Cancelled (${waiting} waiting file(s) dropped too).` : "Cancelled.");
      } else {
        this.error(text);
      }
      return undefined;
    } finally {
      this.busy = null;
      // Something may have waited for the job to end.
      queueMicrotask(() => this.nextProposal());
    }
  }

  cancel() {
    if (this.busy) api.cancelJob();
  }

  /** Runs a quick command and shows its error, if any. */
  async quick<T>(work: () => Promise<T>): Promise<T | undefined> {
    try {
      return await work();
    } catch (e) {
      this.error(api.errorText(e));
      return undefined;
    }
  }

  // ----- the box -----

  async loadBox() {
    const state = await this.quick(api.boxState);
    if (!state) return;
    this.box = state.open;
    this.boxProblem = state.problem;
    if (this.box) await this.reload();
  }

  /** After switching boxes: nothing of the old box stays. */
  async useBox(info: BoxInfo) {
    this.box = info;
    this.boxProblem = null;
    this.notice = null;
    this.results = null;
    this.asked = null;
    this.queue = [];
    this.current = null;
    this.pane = { kind: "empty" };
    this.wizard = null;
    this.wizardLog = [];
    await this.reload();
  }

  async reload() {
    const docs = await this.quick(api.documents);
    if (docs) this.docs = docs;
  }

  open(path: string, reveal = false) {
    this.quick(() => api.openFile(path, reveal));
  }

  /** Dropped on the window: wizard inputs on the Wizard tab, else files to work on. */
  async dropped(paths: string[]) {
    if (this.tab === "wizard") await this.addWizardInputs(paths);
    else await this.openFiles(paths);
  }

  // ----- wizard -----

  async addWizardInputs(paths: string[]) {
    const kinds = await this.quick(() => api.classify(paths));
    for (const { path, kind } of kinds ?? []) {
      if (kind === "other") this.error(`${path} is neither a PDF nor a folder.`);
      else if (!this.wizardInputs.some((i) => i.path === path)) this.wizardInputs.push({ path, kind });
    }
  }

  async startWizard() {
    const inputs = this.wizardInputs.map((i) => i.path);
    const count = await this.quick(() => api.countPdfs(inputs));
    if (count === undefined) return;
    if (count === 0) return this.error("No PDFs in what you chose.");
    this.wizard = null;
    this.wizardLog = [];
    this.notice = null;
    const summary = await this.job(`Wizard: handling ${count} PDF(s)…`, () => api.runWizard(inputs));
    await this.reload();
    if (!summary) return;
    const added = summary.items.filter((i) => i.status === "added").length;
    const open = summary.items.filter((i) => i.status === "needs_you").length;
    this.info(`Wizard done: ${added} added, ${open} need you.`);
    this.wizardInputs = [];
    this.wizard = summary;
  }

  async trashSpareOriginals() {
    if (!this.wizard) return;
    const spare = this.wizard.spare_originals;
    const result = await this.quick(() => api.trash(spare));
    if (!result) return;
    this.wizard.spare_originals = spare.filter((p) => !result.moved.includes(p));
    if (result.errors.length === 0) this.info(`Moved ${result.moved.length} original(s) to the Trash.`);
    else this.error(result.errors.join("; "));
  }

  // ----- files -----

  async openFiles(paths: string[]) {
    const kinds = await this.quick(() => api.classify(paths));
    let opened = false;
    for (const { path, kind } of kinds ?? []) {
      if (kind !== "pdf") {
        this.error(`${path} is not a PDF file.`);
        continue;
      }
      opened = true;
      const file = this.files.find((f) => f.path === path);
      if (file) file.selected = true;
      else this.files.push({ path, selected: true });
    }
    if (opened) this.tab = "files";
  }

  /** Adds a result (merged, sorted file) to the list as the only selected file. */
  async showResult(path: string) {
    for (const f of this.files) f.selected = false;
    await this.openFiles([path]);
  }

  move(i: number, j: number) {
    [this.files[i], this.files[j]] = [this.files[j], this.files[i]];
  }

  closePane() {
    this.pane = { kind: "empty" };
  }

  async check(files: string[]) {
    const what =
      files.length === 1
        ? `Checking ${api.basename(files[0])} (OCR can take a while)…`
        : `Checking ${files.length} files (OCR can take a while)…`;
    const results = await this.job(what, () => api.check(files));
    if (!results) return;
    const failed = results.filter((r) => r.error).length;
    if (failed === 0) this.notice = null;
    else this.error(`${failed} of ${results.length} file(s) could not be checked.`);
    this.pane = { kind: "check", results };
  }

  async name(files: string[]) {
    const proposals = await this.job(`Finding names for ${files.length} file(s)…`, () => api.proposeNames(files));
    if (!proposals) return;
    this.notice = null;
    this.pane = {
      kind: "names",
      items: proposals.map((p) => ({
        file: p.file,
        current: p.current,
        stem: p.suggestion,
        error: p.error,
        done: p.suggestion === p.current && !p.error ? "already has a good name" : null,
      })),
    };
  }

  async rename(item: NameItem) {
    const dest = await this.quick(() => api.renameFile(item.file, item.stem));
    if (dest === undefined) return;
    if (dest === null) {
      item.done = "kept the name";
      return;
    }
    item.done = `renamed to ${api.basename(dest)}`;
    for (const f of this.files) if (f.path === item.file) f.path = dest;
    item.file = dest;
  }

  async merge(files: string[]) {
    const merged = await this.quick(() => api.mergeFiles(files));
    if (!merged) return;
    const name = api.basename(merged.out);
    if (merged.complete) this.info(`Wrote ${name} (${merged.pages} pages).`);
    else
      this.error(
        `Wrote ${name}, but it has ${merged.pages} instead of ${merged.expected} pages; keep the originals.`,
      );
    await this.showResult(merged.out);
    this.pane = {
      kind: "done",
      heading: `Merged into ${name}`,
      originals: merged.originals,
      offerTrash: merged.complete,
    };
  }

  private showSort(view: SortView) {
    this.notice = null;
    this.pane = { kind: "sort", view, order: view.order, layout: view.layout, deleteBlank: false };
  }

  async sort(file: string) {
    const view = await this.job(`Sorting ${api.basename(file)} (OCR can take a while)…`, () => api.proposeSort(file));
    if (view) this.showSort(view);
  }

  async editOrder(order: string) {
    if (this.pane.kind !== "sort") return;
    this.pane.order = order;
    const layout = await this.quick(() => api.sortPreview(order));
    // Only the answer to the latest edit counts.
    if (layout && this.pane.kind === "sort" && this.pane.order === order) this.pane.layout = layout;
  }

  async writeSort() {
    if (this.pane.kind !== "sort") return;
    const { order, deleteBlank } = this.pane;
    const written = await this.quick(() => api.writeSort(order, deleteBlank));
    if (!written) return;
    const name = api.basename(written.out);
    if (written.originals.length === 0) {
      this.info(`Wrote ${name} (${written.count} pages); ${written.input_name} is unchanged.`);
      this.pane = { kind: "empty" };
    } else {
      if (written.still === "") this.info(`Wrote ${name} (${written.count} pages).`);
      else
        this.error(
          `Wrote ${name} (${written.count} pages), but printed page ${written.still} is still missing: run Counterpart on it to look further.`,
        );
      this.pane = {
        kind: "done",
        heading: `Merged and sorted into ${name}`,
        originals: written.originals,
        offerTrash: true,
      };
    }
    await this.showResult(written.out);
  }

  async counterpart(file: string) {
    const what = `Looking for the missing pages of ${api.basename(file)} (OCR can take a while)…`;
    const result = await this.job(what, () => api.findCounterparts(file));
    if (!result) return;
    if (result.kind === "complete") {
      this.info(`${api.basename(file)}: no missing pages found, nothing to look for.`);
      this.pane = { kind: "check", results: [result.check] };
      return;
    }
    if (result.candidates.length === 0)
      this.info(`No counterpart found in ${result.dir}; see below why each PDF was not suggested.`);
    else this.notice = null;
    this.pane = { kind: "counterparts", found: result };
  }

  async useCandidate(index: number) {
    if (this.pane.kind !== "counterparts") return;
    const name = this.pane.found.candidates[index]?.name;
    this.pane = { kind: "empty" };
    const view = await this.job(`Merging with ${name} and sorting…`, () => api.mergeWithCandidate(index));
    if (view) this.showSort(view);
  }

  async trashOriginals() {
    if (this.pane.kind !== "done") return;
    const result = await this.quick(() => api.trash(this.pane.kind === "done" ? this.pane.originals : []));
    if (!result) return;
    this.files = this.files.filter((f) => !result.moved.includes(f.path));
    this.pane = { kind: "empty" };
    if (result.errors.length === 0) this.info(`Moved ${result.moved.length} file(s) to the Trash.`);
    else this.error(result.errors.join("; "));
  }

  // ----- find and ask -----

  async startSearch() {
    const question = this.question.trim();
    if (!question) return;
    const found = await this.job("Searching…", () => api.search(question));
    if (!found) return;
    if (found.note) this.info(found.note);
    else if (found.rows.length === 0) this.info("No matching document found.");
    else this.info(`${found.rows.length} matching document(s).`);
    this.results = { question, rows: found.rows };
  }

  showAll() {
    this.results = null;
    this.notice = null;
  }

  async startAsk() {
    const question = this.askQuestion.trim();
    if (!question) return;
    const what = "Searching the documents and asking the model (embedding new pages first)…";
    const asked = await this.job(what, () => api.ask(question));
    if (!asked) return;
    if (asked.without_text > 0)
      this.info(
        `${asked.without_text} documents have no page text yet and were not searched: run \`shoebox text\`.`,
      );
    else this.notice = null;
    this.asked = asked;
  }

  // ----- add -----

  queueForBox(files: string[]) {
    for (const f of files) if (!this.queue.includes(f)) this.queue.push(f);
    this.tab = "add";
    this.nextProposal();
  }

  /** Starts the next file waiting to be added, if nothing else is going on. */
  async nextProposal() {
    if (this.busy || this.current || this.queue.length === 0) return;
    const file = this.queue.shift()!;
    const what = `Reading ${api.basename(file)} and asking the model (OCR can take a while)…`;
    const prepared = await this.job(what, () => api.prepareAdd(file));
    if (!prepared) return;
    if (prepared.kind === "already_there") {
      this.info(`${prepared.name} is already in the box.`);
      return;
    }
    if (prepared.model_error)
      this.error(`The model gave no usable answer (${prepared.model_error}); please fill in the fields.`);
    else this.notice = null;
    this.current = { proposal: prepared, entry: { ...prepared.entry } };
  }

  async saveCurrent() {
    if (!this.current) return;
    const { proposal, entry } = this.current;
    const stored = await this.quick(() => api.saveEntry(proposal.id, $state.snapshot(entry)));
    if (stored === undefined) return;
    this.info(`Added ${proposal.name}.`);
    this.current = null;
    await this.reload();
    this.nextProposal();
  }

  skipCurrent() {
    if (!this.current) return;
    const { proposal } = this.current;
    api.dropProposal(proposal.id);
    this.info(`Skipped ${proposal.name}.`);
    this.current = null;
    this.nextProposal();
  }
}

export const app = new App();
