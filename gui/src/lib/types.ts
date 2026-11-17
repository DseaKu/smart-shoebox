// The JSON views of the Tauri commands in src-tauri/src/lib.rs.

export interface BoxInfo {
  root: string;
  model: string;
}

export interface BoxState {
  open: BoxInfo | null;
  problem: string | null;
}

export interface PathKind {
  path: string;
  kind: "pdf" | "folder" | "other";
}

export interface Trashed {
  moved: string[];
  errors: string[];
}

export interface Doc {
  hash: string;
  date: string;
  tags: string;
  summary: string;
  original_name: string;
  /** The PDF in the store. */
  path: string;
}

export interface Found {
  rows: Doc[];
  note: string | null;
}

export interface Source {
  name: string;
  page: number;
  quote: string;
  path: string;
}

export interface Asked {
  question: string;
  outcome:
    | { kind: "answered"; answer: string; sources: Source[] }
    | { kind: "not_found"; why: string; claimed: string | null };
  without_text: number;
}

export interface Entry {
  date: string;
  tags: string;
  summary: string;
}

export interface WizardItem {
  file: string;
  name: string;
  steps: string[];
  status: "added" | "already_in_box" | "skipped" | "needs_you";
  reason: string | null;
  entry: Entry | null;
  line: string;
}

export interface WizardSummary {
  items: WizardItem[];
  spare_originals: string[];
}

export interface CheckView {
  file: string;
  name: string;
  error: string | null;
  lines: string[];
  incomplete: boolean;
  readable_name: boolean;
  in_box: boolean;
}

export interface NameView {
  file: string;
  current: string;
  suggestion: string;
  error: string | null;
}

export interface MergedView {
  out: string;
  pages: number;
  expected: number;
  complete: boolean;
  originals: string[];
}

export interface SortLayout {
  error: string | null;
  documents: { title: string; pages: { number: number; printed: string | null; preview: string }[] }[];
  missing: string[];
  duplicates: string[];
  blank: number[];
}

export interface SortView {
  heading: string;
  out_name: string;
  order: string;
  layout: SortLayout;
}

export interface SortWritten {
  out: string;
  count: number;
  input_name: string;
  originals: string[];
  still: string;
}

export type Counterparts =
  | { kind: "complete"; check: CheckView }
  | {
      kind: "found";
      name: string;
      dir: string;
      candidates: { path: string; name: string; description: string; still: string }[];
      rejected: { path: string; name: string; reason: string }[];
    };

export type Prepared =
  | { kind: "already_there"; name: string }
  | { kind: "proposal"; id: number; file: string; name: string; entry: Entry; model_error: string | null };

export type Proposal = Extract<Prepared, { kind: "proposal" }>;
