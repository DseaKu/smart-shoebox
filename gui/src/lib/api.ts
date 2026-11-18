// Typed wrappers for the Tauri commands in src-tauri/src/lib.rs.

import { invoke } from "@tauri-apps/api/core";
import type {
  Asked,
  BoxInfo,
  BoxState,
  CheckView,
  Counterparts,
  Doc,
  Entry,
  Found,
  MergedView,
  NameView,
  PathKind,
  Prepared,
  SortLayout,
  SortView,
  SortWritten,
  Trashed,
  WizardSummary,
} from "./types";

/** The error text of a cancelled job. */
export const CANCELLED = "Cancelled.";

export const cancelJob = () => invoke<void>("cancel_job");

// The box
export const boxState = () => invoke<BoxState>("box_state");
/** `null` if the folder holds no box. */
export const openBox = (path: string) => invoke<BoxInfo | null>("open_box", { path });
export const createBox = (path: string) => invoke<BoxInfo>("create_box", { path });

// Files
export const classify = (paths: string[]) => invoke<PathKind[]>("classify", { paths });
export const openFile = (path: string, reveal = false) => invoke<void>("open_file", { path, reveal });
export const trash = (paths: string[]) => invoke<Trashed>("trash", { paths });

// Documents
export const documents = () => invoke<Doc[]>("documents");
export const search = (question: string) => invoke<Found>("search", { question });
export const ask = (question: string) => invoke<Asked>("ask", { question });

// Wizard
export const countPdfs = (inputs: string[]) => invoke<number>("count_pdfs", { inputs });
export const runWizard = (inputs: string[]) => invoke<WizardSummary>("run_wizard", { inputs });

// Files actions
export const check = (files: string[]) => invoke<CheckView[]>("check", { files });
export const proposeNames = (files: string[]) => invoke<NameView[]>("propose_names", { files });
/** The new path, or `null` if the name did not change. */
export const renameFile = (file: string, stem: string) => invoke<string | null>("rename_file", { file, stem });
export const mergeFiles = (files: string[]) => invoke<MergedView>("merge_files", { files });
export const proposeSort = (file: string) => invoke<SortView>("propose_sort", { file });
export const sortPreview = (order: string) => invoke<SortLayout>("sort_preview", { order });
export const writeSort = (order: string, deleteBlank: boolean) =>
  invoke<SortWritten>("write_sort", { order, deleteBlank });
export const findCounterparts = (file: string) => invoke<Counterparts>("find_counterparts", { file });
export const mergeWithCandidate = (index: number) => invoke<SortView>("merge_with_candidate", { index });

// Add to the box
export const prepareAdd = (file: string) => invoke<Prepared>("prepare_add", { file });
export const saveEntry = (id: number, entry: Entry) => invoke<string>("save_entry", { id, entry });
export const dropProposal = (id: number) => invoke<void>("drop_proposal", { id });
export const isIsoDate = (date: string) => invoke<boolean>("is_iso_date", { date });

/** The error text of a failed command. */
export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}

export function basename(path: string) {
  return path.split(/[\\/]/).pop() ?? path;
}

export function dirname(path: string) {
  const i = Math.max(path.lastIndexOf("/"), path.lastIndexOf("\\"));
  return i > 0 ? path.slice(0, i) : "";
}

/** The name of the button that shows a file in its folder, as the system calls it. */
export const REVEAL_LABEL = navigator.userAgent.includes("Mac")
  ? "Show in Finder"
  : navigator.userAgent.includes("Windows")
    ? "Show in Explorer"
    : "Show in folder";
