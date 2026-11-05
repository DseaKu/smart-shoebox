# Smart Shoebox

*The shoebox for your paperwork, except you can actually find things in it.*

> **This is mainly a learning project.** I'm building it to improve my skills
> with AI and AI agents: running local models with Ollama, splitting work into
> agent roles (extractor, reviewer, finder, sorter), getting structured output
> from a model, and checking that output in code before trusting it. The
> paperwork problem is real, but the main goal is learning, so expect rough
> edges and changing designs.

### Who does what

This project is built together with an AI coding assistant (Claude Code).

**Me**

- the idea, the problem and the first version of this README,
- the starting point: a small Rust/Ollama agent program from an earlier project,
- the decisions: what to build, what not to build, and how (e.g. Rust, local
  models only, a separate `merge` and `sort`, keeping private data out of git),
- testing with my own scans and documents, which is where bugs like the
  broken page sorting and duplicate pages showed up,
- reviewing the changes and committing them.

**The AI assistant**

- writes most of the code, the tests and this documentation, following my
  instructions,
- suggests designs and options, which I pick from or change,
- debugs problems I report and explains how things work.

A git repository that keeps track of my private paperwork PDFs, so that I (or an
AI) can find the right document later.

## Idea

I have many PDFs from private bureaucracy (tax notices, contracts, insurance,
invoices), and finding the right one later is hard.

Smart Shoebox is a self-contained git repo with a small command-line tool
inside, `shoebox`. The tool:

1. copies every PDF I add into a local store, named by its content hash,
2. lets an AI read the document and write a short summary and tags,
3. records everything in a plain-text index table.

When I need a document, I ask the AI ("Where is my rental contract?"). It reads
the index, picks the matching row and returns the path to the PDF.

Only the code is committed. The PDFs and the index never go into git, because
both reveal private information; they stay on my machine.

## How it works

The repo has two places: the **build space** with the code, and the **box**
(user space) with my documents. The box is gitignored as a whole, so nothing
private can be published by accident.

```
smart-shoebox/               # build space (committed)
├── src/                     # the tool: Rust CLI, binary `shoebox`
├── shoebox.example.yaml     # default settings, copied by `shoebox init`
├── .gitignore               # ignores box/, *.pdf, index.tsv, store/, shoebox.yaml
├── README.md
└── box/                     # user space (NOT committed)
    ├── shoebox.yaml         # my settings: model, context size, OCR language
    ├── index.tsv            # the index: one line per document
    └── store/               # PDFs, named <sha256>.pdf
```

Getting started:

```
cargo install --path .       # puts `shoebox` on the PATH
shoebox init                 # creates box/ (or: shoebox init <folder>)
cd box
shoebox add ~/Downloads/bescheid.pdf
```

`add`, `find`, `name` and `sort` use the nearest box: the current folder or any folder
above it, or its `box/` subfolder (so they also work from the repo root). To
keep the box somewhere else entirely, e.g. in an encrypted or backed-up
folder, create it there with `shoebox init <folder>` and set `SHOEBOX_DIR` to
that folder. `merge` needs no box.

- **Store:** The tool creates `store/` if it does not exist. Every added PDF is
  copied there as `<full-sha256>.pdf`.
- **Hash = location:** The hash is both the identity and the filename, so the
  path of a document is always `store/<hash>.pdf`. Renaming or moving the
  original has no effect.
- **Deduplication:** Adding the same file twice is a no-op, because it produces
  the same hash.
- **Index:** `index.tsv` is a tab-separated text file, easy to read, grep and
  diff.

## Index format

| Column          | Meaning                                             |
| --------------- | --------------------------------------------------- |
| `hash`          | SHA-256 of the file (also the filename in `store/`) |
| `date`          | Date of the document (not the date it was added)    |
| `original_name` | Filename at the time of adding, as a hint           |
| `tags`          | Comma-separated tags, e.g. `steuer,finanzamt`       |
| `summary`       | One-line summary written by the AI                  |

Example:

```
hash    date        original_name     tags              summary
a3f9…   2026-03-14  bescheid.pdf      steuer,finanzamt  Steuerbescheid 2025, Nachzahlung 312 €
7c21…   2025-11-02  mietvertrag.pdf   wohnung,vertrag   Mietvertrag Wohnung, unbefristet, Kaltmiete 780 €
```

## Commands

| Command             | What it does                                                                                                                                                                                      |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `init [folder]`    | Create a box (default `box/`): `shoebox.yaml` with default settings, `store/` and an empty `index.tsv`. Existing files are kept. |
| `add <file>…`       | Hash the PDF, copy it into `store/`, extract text, let the AI propose date, tags and summary, then ask me to accept or edit, and append the row to `index.tsv`. |
| `find "<question>" [--to <folder>]` | Give the AI the index and the question; it returns the matching row(s) and the path(s). With `--to`, copies the matches into that folder with a readable name like `2026-03-14_steuerbescheid-finanzamt-muenchen-nachzahlung.pdf`. |
| `name <file.pdf>…`  | Rename PDFs in place to a readable name like `2026-03-14_steuerbescheid-2025-finanzamt-muenchen.pdf`. Documents already in the box are named from the index; others get a date and short title from the AI. I confirm or edit each name; existing files are never overwritten. |
| `merge <a.pdf> <b.pdf>… [--out <file.pdf>]` | Append the PDFs, in the given order, into one PDF (default `<a>_merged.pdf`). No AI, no OCR. Then asks whether to delete the originals (they go to the Trash; only offered if every page made it into the result). |
| `sort <scan.pdf> [--out <file.pdf>] [--delete-blank]` | Let the AI put the pages of one PDF into reading order, then ask me to accept or edit. Writes one PDF with the same pages (default `<scan>_sorted.pdf`); if there are blank pages, it asks at the end whether to delete them (`--delete-blank` deletes them without asking). |
| `check <scan.pdf> [--in <folder>]` | General health check: from the printed page numbers, is the scan complete (missing pages, blank pages, duplicates)? Is the file name readable? Is it already in the box? Then offers the fixes: `counterpart` if pages are missing, and a readable name (as with `name`) if the name is not, for the merged result if one was made. |
| `counterpart <scan.pdf> [--in <folder>]` | Look through the other PDFs in the same folder (or `--in`) for the missing pages. A match must contain a missing page, must not contain the scan's own page numbers (another copy or another letter) or a different "von N", and must share at least 2 reference numbers or dates. Matches that share numbers found in few other PDFs (letter date, invoice number rather than contract number or IBAN) come first; every PDF not suggested is listed with the reason. If I agree, merge and sort both into `<scan>_complete.pdf`, then ask whether to move the two originals to the Trash. |
| `verify` (planned) | Rehash every file in `store/` and compare with the index. Reports missing, unknown or corrupted files.                                                                                            |

`shoebox --help` lists all commands, and `shoebox <command> --help` (e.g.
`shoebox sort --help`) shows the options of one; mistyped options and commands
get a suggestion.

`-y` / `--yes` (anywhere on the command line) answers yes to every
confirmation in `add`, `name` and `sort`, e.g. `shoebox name -y dummy/*.pdf`.
It skips my review, so wrong AI proposals go through unchecked.

### Adding a document

1. Compute the SHA-256 over the whole PDF file (all bytes, not the text or the
   filename).
2. Extract the text (`pdf-extract`; for scanned PDFs without a text layer,
   OCR with `ocrmypdf` on a temporary copy).
3. Send the text to the AI together with the **list of existing tags**, with the
   instruction to reuse existing tags and only create a new one if none fit.
   This prevents tag drift (`steuer` / `steuern` / `tax`). A second AI pass
   reviews the proposal against the text.
4. Show the proposed entry; I confirm or edit it.
5. Copy the file to `store/<hash>.pdf` (only after confirming, so a rejected
   document leaves nothing behind).
6. Append the row to `index.tsv`. The extracted text is discarded (or kept
   outside git), since it is sensitive and not needed for lookup.

### Finding a document

1. I ask a question in plain language.
2. In code, the rows are ranked by the question's words (also with other
   endings, e.g. "Mietverträge" finds "Mietvertrag") and years, then by date.
3. The AI gets numbered rows (`#12 | date | tags | summary | file name`) and
   answers with the numbers of the matches, as JSON. Numbers it was not given
   are ignored.
   - If rows match the question's words, only the best 60 are sent: small
     models miss the right row in a long list even when it is there.
   - Otherwise as many rows as fit into `context_size` are sent, so the AI can
     match by meaning ("rental contract" → "Mietvertrag"). About 300 rows fit
     into 16384 tokens; with a bigger box, `find` says how many it searched.
4. The answer is the row and the path `store/<hash>.pdf`.

### Sorting scans

My scanner is one-sided: two-sided letters are scanned in two passes, the
backs come out reversed, and stacks get mixed up. Two separate steps fix that:

```
shoebox merge fronts.pdf backs.pdf     # → fronts_merged.pdf
shoebox sort fronts_merged.pdf         # → fronts_merged_sorted.pdf
shoebox add fronts_merged_sorted.pdf
```

`merge` only appends files. `sort` reorders pages; it never splits them, and
the only pages it leaves out are duplicate scans:

1. OCR pages without text (`ocrmypdf --skip-text`, which also straightens and
   rotates upside-down pages). The result is built from this searchable copy.
2. In code, without AI: detect blank pages, printed page numbers
   (`Seite 2 von 3`, `2 / 3`, `- 2 -`, …; references like "auf der Seite 3"
   are ignored) and duplicate scans (same printed number, ≥ 90 % of the words
   the same).
3. The AI groups the remaining pages into documents, using sender, reference
   numbers, dates and text that continues across pages. Its answer is checked
   in code (every page exactly once); if it is invalid, it is asked once more,
   then everything falls back to scan order.
4. In code: each document is ordered by its printed page numbers. Pages
   without a number fill the gaps (usually page 1 with the letterhead), and
   numbers still missing are reported ("printed page 6 is not in the scan").
5. I confirm or edit the order (e.g. `1,6,4,5,2`). Duplicates are left out
   unless I list them.
6. If blank pages are left over, it asks whether to delete them; if not, they
   go to the end. `--delete-blank` deletes them without asking, and a blank
   page I placed myself while editing is always kept. The original scan is
   never changed.

### Completing a scan

A one-sided scanner often leaves a document spread over two files, e.g. the
fronts in one and the backs in another:

```
shoebox check input/nebenkostenabrechnung.pdf
```

1. `check` reads the printed page numbers (with OCR if needed). Pages without a
   number fill the lowest gaps, usually page 1 with the letterhead. A "von N"
   on any page also reveals missing last pages; without it, missing pages at
   the end cannot be noticed.
2. If pages are missing, it offers to look for the **counterpart**: another PDF
   in the same folder that contains missing page numbers and shares reference
   numbers or dates (contract numbers, customer numbers, …), and whose page
   numbers fit (none of this scan's own pages, the same "von N"). This is plain
   code, no AI. The best match is shown first: the one whose shared numbers are
   in the fewest other PDFs. Every PDF not suggested is listed with the reason,
   e.g. "has this scan's own page 1, 3 (another copy or another letter)".
3. If I agree, both files are merged and go through the normal `sort`, with
   its confirmation. The result is `<scan>_complete.pdf`; the originals stay as
   they are. Pages still missing afterwards are reported.

### The window (optional)

Besides the command line there is a small window, `shoebox-gui`. It is built
separately, so a plain `cargo build` / `cargo install --path .` builds only the
command-line tool, without the GUI libraries.

```sh
cargo run-gui                 # start it while developing
cargo build-gui               # one program: target/release/shoebox-gui
scripts/bundle-macos.sh       # Smart Shoebox.app, to double-click or drag into /Applications
```

- **Files**: open one or more PDFs (file dialog or drop them on the window), select
  some and run an action: Add to box, Name, Merge (in list order), Sort (with an
  editable page order and the blank-page choice), Check (one or more files: a report
  per file with Search counterpart / Name it / Sort it / Add to box, and for several
  files Name all unreadable / Add all to the box) or Counterpart (pick one of the PDFs
  that hold the missing pages, then edit the sort and write `<file>_complete.pdf`;
  offers to move both originals to the Trash).
- **Find**: browse all documents or search by question; open a document in Preview
  or show it in Finder.
- **Add**: review and edit the AI's proposal, then save.

Started from a terminal it uses the same box as `shoebox`. Started by a double
click it uses the box chosen last time, or asks for one once (and offers to create
it); the choice is remembered in `~/Library/Application Support/Smart Shoebox/`.
It uses the same code as the commands; there is no PDF preview inside the window.
`ocrmypdf` and Ollama are still needed. The app is not signed: on another Mac,
open it the first time with right-click → Open.

## Design decisions

- **Plain text over a database.** The index is greppable, diffable and survives
  any tool.
- **Hash-named store.** Content-addressed, dedupes for free, and `verify` can
  detect bit rot.
- **AI writes the metadata, I review it.** Writing comments by hand would be
  abandoned after a few files. The review step keeps wrong summaries from
  silently entering the index.
- **Structured fields next to the summary.** Date and tags stay searchable even
  if a summary is vague.

## Privacy and backup

- The index is sensitive too (it reveals contracts, taxes, insurance), so it is
  gitignored like the PDFs. Only the code is ever pushed; still, keep the repo
  **private**.
- Sending the index or document text to a cloud AI exposes it to that provider.
  A local model is an option if that is a concern.
- The box (`store/` and `index.tsv`) is **the only copy** of the documents and their
  metadata. Git does not protect them. They need their own backup (external
  drive, restic, etc.). Restoring is easy: copy the box back and run `verify`.

## Not goals

- Not a replacement for full-text search or OCR suites like paperless-ngx.
  Smart Shoebox is deliberately small and transparent.
- No web interface, no sync, no multi-user support.

## Decided

- **Language:** Rust, starting from my earlier Rust/Ollama agent program.
- **AI backend:** local models only, through Ollama, so documents never leave
  my machine. The model is set in `shoebox.yaml`.
- **Summary language:** the language of the document (usually German).
- **`add` copies, it does not move:** the original file is never touched.

## Open questions

Planned changes are listed in [ROADMAP.md](ROADMAP.md).

- `verify` and `add --move` are planned but not built yet.
- Small models reuse existing tags too eagerly (e.g. tagging an insurance
  letter `steuer` because that tag already exists). Better role prompts or a
  bigger model?
- A `split` command for stacks with several letters, now that `sort` only
  reorders pages.

## License

[MIT](LICENSE) © 2026 Volker Kuntner
