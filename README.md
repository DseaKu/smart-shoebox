# Smart Shoebox

*The shoebox for your paperwork, except you can actually find things in it.*

> **Learning project, under development.** I build it to learn AI agents and
> local models with Ollama. Interfaces, the index format and the prompts can
> still change, so keep a backup before batch commands like `wizard` or `name -y`.
> Built together with an AI coding assistant (Claude Code).

## What it does

`shoebox` is a small command-line tool (Rust) for private PDFs such as tax
notices, contracts and invoices:

1. copies each PDF into a local store, named by its SHA-256 hash,
2. lets a local AI propose a date, tags and a one-line summary, which I review,
3. records everything in a plain-text index, `index.tsv`.

Then `shoebox find "Where is my rental contract?"` returns the path to the PDF.

Only the code is in git. The PDFs and the index stay on my machine, because
they reveal private information.

## Status

| Area                              | State                          |
| --------------------------------- | ------------------------------ |
| `init`, `add`, `find`, `name`     | working, tested with my own documents |
| `merge`, `sort`                   | working, page sorting still being tuned |
| `check`, `counterpart`            | working, matching rules still being tuned |
| `wizard`                          | new, least tested              |
| `shoebox-gui` (macOS window)      | working, rough edges           |
| `verify`, `add --move`            | planned, not built             |

## Quick start

```sh
cargo install --path .       # puts `shoebox` on the PATH
shoebox init                 # creates box/ with settings, store/ and index.tsv
cd box
shoebox add ~/Downloads/bescheid.pdf
```

Requires `ocrmypdf` and [Ollama](https://ollama.com) with a local model.

## Commands

| Command | What it does |
| ------- | ------------ |
| `init [folder]` | Create a box (settings, `store/`, empty index). |
| `add <file>…` | Copy a PDF into the store, let the AI propose the entry, review it, add it to the index. |
| `ask "<question>"` | Answer a question from the text inside the documents, with the source pages and a quote that the code checks. "Not found" instead of a guess. |
| `find "<question>"` | Ask the AI which index rows match; `--to <folder>` copies them out with readable names. |
| `name <file>…` | Rename PDFs to readable names like `2026-03-14_steuerbescheid-2025-finanzamt.pdf`. |
| `merge <a.pdf> <b.pdf>…` | Append PDFs into one. No AI. |
| `sort <scan.pdf>` | Put the pages of a scan into reading order, using printed page numbers and AI grouping. |
| `check <scan.pdf>` | Is the scan complete, readable, and already in the box? Offers the fixes. |
| `counterpart <scan.pdf>` | Find the other PDF that holds the missing pages, merge and sort both. |
| `wizard <pdf or folder>…` | Handle a whole pile: check, complete, name and add, with a list of what needs a decision. |
| `text [--all]` | Save the page text of documents added before it was kept (for searching inside documents, planned). |
| `chunks [--show <doc>]` | Show how the documents are cut into chunks for searching inside them. No AI. |
| `embed` | Turn the chunks into vectors with the embedding model (only new or changed ones). |
| `search "<question>"` | Search inside the documents by meaning and by words: the best matching pages, with scores. No answer yet. |
| `verify` | Planned: rehash the store and compare with the index. |

`shoebox --help` and `shoebox <command> --help` list all options. `-y` answers
yes to every prompt, so wrong AI proposals go through unchecked.

Scanners that are one-sided produce split or reversed pages. Typical fix:

```sh
shoebox merge fronts.pdf backs.pdf     # → fronts_merged.pdf
shoebox sort fronts_merged.pdf         # → fronts_merged_sorted.pdf
shoebox add fronts_merged_sorted.pdf
```

## Window

`shoebox-gui` is a small macOS window with the same functions as the commands
(wizard, files, find, add). It is built separately, so `cargo install` stays
free of GUI libraries:

```sh
cargo build-gui               # target/release/shoebox-gui
scripts/bundle-macos.sh       # Smart Shoebox.app
```

## Design

- **Plain text over a database.** The index is greppable, diffable and survives any tool.
- **Hash-named store.** Content-addressed, so duplicates are skipped for free.
- **AI writes, I review.** The AI proposes metadata; nothing enters the index unreviewed unless I use `-y`.
- **Local models only.** Documents never leave the machine.

## Privacy and backup

- The box (`store/`, `index.tsv`) is gitignored. Keep the repository **private** anyway.
- The box is the **only copy** of the documents. Git does not back it up; use an external drive or similar.

## Not goals

Not a replacement for full-text search suites like paperless-ngx. No web
interface, no sync, no multi-user support.

## More

- [docs/DETAILS.md](docs/DETAILS.md): full reference (index format, workflows for adding, finding, sorting and completing scans, the wizard, the window, decisions and open questions)
- [ROADMAP.md](ROADMAP.md): what comes next

## License

[MIT](LICENSE) © 2026 Volker Kuntner
