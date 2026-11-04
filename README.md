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

`add`, `find` and `sort` use the nearest box: the current folder or any folder
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

## Commands (planned)

| Command             | What it does                                                                                                                                                                                      |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `init [folder]`    | Create a box (default `box/`): `shoebox.yaml` with default settings, `store/` and an empty `index.tsv`. Existing files are kept. |
| `add <file>…`       | Hash the PDF, copy it into `store/`, extract text, let the AI propose date, tags and summary, then ask me to accept or edit, and append the row to `index.tsv`. `--move` moves instead of copies. |
| `find "<question>" [--to <folder>]` | Give the AI the index and the question; it returns the matching row(s) and the path(s). With `--to`, copies the matches into that folder with a readable name like `2026-03-14_steuerbescheid-finanzamt-muenchen-nachzahlung.pdf`. |
| `merge <a.pdf> <b.pdf>… [--out <file.pdf>]` | Append the PDFs, in the given order, into one PDF (default `<a>_merged.pdf`). No AI, no OCR. |
| `sort <scan.pdf> [--out <file.pdf>]` | Let the AI put the pages of one PDF into reading order, then ask me to accept or edit. Writes one PDF with exactly the same pages (default `<scan>_sorted.pdf`). |
| `verify`            | Rehash every file in `store/` and compare with the index. Reports missing, unknown or corrupted files.                                                                                            |

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
2. The AI reads `index.tsv` (it fits in context for a few thousand rows, so no
   embeddings or vector database are needed).
3. The answer is the row and the path `store/<hash>.pdf`.

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
5. I confirm or edit the order (e.g. `1,6,4,5,2`). Blank pages go to the end
   unless I place them; duplicates are left out unless I list them. The
   original scan is never changed.

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

## Open questions

- Language and runtime of the tool (Python is the quickest start).
- Which AI backend `add` and `find` should call (cloud API or local model).
- Language of the summaries (German, matching the documents, or English).
- Whether `add` copies or moves by default (planned: copy, with `--move`).
