# Roadmap

Planned changes from the project review (2026-11-04), in the order they should be
done. Each step can be committed and tested on its own.

## 1. Make `find` scale (high priority) — done

**Problem:** one index row costs about 110 tokens (the 64-character hash alone
is about half of it). With `context_size: 16384`, only about 140 documents fit.
Beyond that, Ollama silently cuts part of the prompt, and older documents can no
longer be found, without any error.

- [x] Send short row numbers instead of hashes to the finder
      (`#12 2026-03-14 steuer,finanzamt Steuerbescheid …`); the model answers
      `12`, and the code maps it back to the hash. About half the tokens per row.
- [x] Pre-filter in code: rank rows by matching words, tags and years from the
      question, and send only the best ones. Works with thousands of documents.
      When words match, at most 60 rows are sent: in a test with 1,501 rows the
      model missed the right row among 500, but found it among 60.
- [x] Warn instead of failing silently: estimate the prompt size and say when
      rows were left out ("searched the best 120 of 900 documents").
- [x] Fix the README ("a few thousand rows fit in context" is wrong).
- [x] Tests for the pre-filter and for mapping numbers back to hashes.

## 2. Don't crash on unusual PDFs (high priority) — done

**Problem:** `pdf-extract` panics instead of returning an error in about 100
places. One odd PDF ends the whole run, e.g. `name dummy/*.pdf` stops at file 3
of 20.

- [x] Catch panics around `pdf-extract` in `pdf.rs`, so a broken PDF becomes a
      normal error ("cannot read x.pdf: …"). Also around `lopdf` loading.
- [x] Batch commands (`name`) continue with the next file after such an error.
- [x] A test with deliberately broken PDFs (a missing font and a Type0 font
      without descendant fonts; both really crash `pdf-extract`).

## 3. Consistent AI answers (medium) — done

**Problem:** `sort` uses a strict JSON schema (which fixed the model getting
stuck in loops), but `add` and `name` still parse free-text
"date: … / tags: …" lines.

- [x] Move the extractor, reviewer and namer to strict JSON, like `sort`. All AI
      calls now go through one helper (`Llm::ask_for`), and the free-text line
      parsing is gone.
- [x] Make the reviewer optional (`review: true/false` in `shoebox.yaml`), to
      measure whether the second AI call is worth it with small models.
      **Measured with `qwen3:8b` on two test documents:** same result for one,
      and for the other the reviewer made the tags *worse* (it copied the tax
      tags `finanzamt,nachzahlung` onto an insurance letter). With the reviewer,
      `add` took 9–12 s instead of 3–4 s. So the reviewer is now off by default.
- [x] Same retry and error handling as `sort`: if the model aborts, ask once
      more, then let me fill in the fields (or, for a failed review, use the
      first proposal).

## 4. Command line with `clap` (medium) — done

**Problem:** `main.rs` parses arguments by hand, differently per command.
Unknown flags are treated as file names (`--dlete-blank` → "does not exist"),
and there is no per-command help.

- [x] Replace the hand-written parsing with `clap`.
- [x] Typos are caught ("unknown option `--dlete-blank`, did you mean
      `--delete-blank`?"), and `shoebox sort --help` works.
- [x] Commands and options stay the same, so the README stays valid;
      `-y` keeps working everywhere. Also new: `shoebox --version`.

## 5. Testable command flows (lower)

**Problem:** the model is a concrete type, so `add`, `sort`, `check` and
`counterpart` can only be tested by hand.

- [ ] A small trait for the model, with a fake model for tests.
- [ ] End-to-end tests for `add`, `find`, `sort`, `check` and `counterpart`
      with generated test PDFs in a temporary box, without Ollama.

## 6. Tidy-up (lower)

- [ ] Merge duplicate helpers: one for free file names
      (`pdfdoc::output_path`, `naming::free_path`), one date check
      (`naming::is_iso_date`, `Entry::has_valid_date`), one number-list `join`.
- [ ] Give `add`, `find` and `merge` their own modules like the other commands,
      and remove `commands.rs`.

## Not planned for now

- `verify` and `split` commands
- Trash support outside macOS
- Moving the heuristics (blank-page threshold, duplicate similarity,
  counterpart matching) into `shoebox.yaml`
