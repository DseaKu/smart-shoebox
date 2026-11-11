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

## 5. Testable command flows (lower) — done

**Problem:** the model is a concrete type, so `add`, `sort`, `check` and
`counterpart` can only be tested by hand.

- [x] A small trait for the model (`Model`), with a fake model for tests
      (`testutil::FakeModel`). `main` creates the model and passes it to the
      commands. Prompts can be answered by a script in tests.
- [x] End-to-end tests for `add`, `find`, `sort`, `check` and `counterpart`
      with generated test PDFs in a temporary box, without Ollama (`src/flows.rs`,
      7 tests, plus `name`). Checked that they catch bugs: with the page ordering
      in `sort` deliberately broken, the `sort` and `check` tests fail.

## 6. Tidy-up (lower) — done

- [x] Merge duplicate helpers: one for free file names
      (`pdfdoc::output_path`, `naming::free_path`), one date check
      (`naming::is_iso_date`, `Entry::has_valid_date`), one number-list `join`
      (`pages::join_numbers`).
- [x] Give `add`, `find` and `merge` their own modules like the other commands,
      and remove `commands.rs` (and `pipeline.rs`: each command now has its AI
      call next to its flow). Also formatted everything with `cargo fmt`.

## 7. Answers from the document text: RAG (planned, 2026-11-11)

**Goal:** today `find` searches the index rows (date, tags, one-line summary)
and returns PDFs. A new command `ask` searches the text *inside* the documents
and answers with sources:
`shoebox ask "Wie hoch war meine Steuererstattung 2025?"` →
"875,00 EUR (steuerbescheid_2025.pdf, page 1)". Everything stays local.

Order: 7.1 and 7.6 first, so the effect of 7.2–7.5 can be measured.

### 7.1 Keep the page text — done
- [x] `add` (and `wizard`, and the window, through `add::save`) saves the
      extracted (or OCR) text per page as `store/<hash>.pages.json` (`pagetext.rs`).
      It stays private (`store/` is ignored). Pages without text are now OCR'd
      also in a partly digital PDF; if that OCR fails, the other pages are kept.
- [x] Backfill: `shoebox text` (`--all` reads every document again).
- [x] Tests: save/load, one text per page, `add` and `wizard` save it, backfill
      (including a missing PDF).

### 7.2 Chunks — done
- [x] One chunk per page; long pages split at about 400 tokens (1,200 characters)
      at line breaks, with up to 200 characters of overlap (`chunk.rs`).
      Runs of layout spaces are collapsed, blank pages give no chunk.
      Each chunk keeps its hash and page number (needed for the sources).
- [x] Chunks are not stored: they are made from `store/<hash>.pages.json`
      when needed (`chunk::of_box`, which also lists documents without page text).
- [x] `shoebox chunks [--show <doc>]` shows the cutting, to check it by eye.
- [x] Checked on the test data: the fake documents give one chunk per page;
      the public law texts and US forms are cut into parts of at most 1,200 characters.
- [x] ~~Put the document's date and summary in front of each chunk~~:
      measured in 7.4, it made the search worse, so it is not done.

### 7.3 Embeddings with Ollama — done
- [x] Embed the chunks via `/api/embed` (`ollama-rs`), with `bge-m3`
      (multilingual, 1024 dimensions, no prefix for questions); set as
      `embed_model` in `shoebox.yaml` (`vectors.rs`).
- [x] Stored as two plain files: `store/vectors.json` (model, dimensions,
      chunk ids, text fingerprints) and `store/vectors.f32` (the normalized
      vectors). Only new or changed chunks are embedded; a different model or
      files that do not match mean a rebuild.
- [x] No vector database: brute-force cosine search.
- [x] `shoebox embed` brings the vectors up to date; `shoebox search
      "<question>" [--top N]` shows the best chunks with document, page and
      score (and updates the vectors first). No answer yet: that is 7.5.
- [x] Tests with a fake embedder (`testutil::FakeEmbedder`): only changes are
      embedded, the order follows the chunks, another model or a damaged file
      means a rebuild, ranking by cosine.
- [x] Measured (`eval_rag.py --mode search`, 25 questions, 2026-11-11):
      correct page at rank 1: **20 of 25**, in the top 3: 23, in the top 5: **25**.
      Correct document at rank 1: 21 of 25 (`find`: 15 of the same 25).
      All 3 `number` questions at rank 1. Weak spots: the `year` questions
      (1 of 3 at rank 1: the 2024 statement ranks above the 2025 one, since
      page 3 does not show the year) and questions where similar words win
      ("Bruttoarbeitslohn" → the employment contract's "Bruttogehalt").
      Embedding the 24 chunks took about 2.5 s.

### 7.4 Hybrid retrieval — done
- [x] Keyword score (BM25) over the chunks (`retrieve.rs`). Words keep their
      inner punctuation and also give their parts, so "BK-2025-0381" matches
      "2025"; umlauts and stop words as in `search.rs`. A word also matches a
      longer word that starts or ends with it (at least 5 letters and 60 % of
      it, weight 0.7): "Mietvertrages", "Warmwasserkosten", "Steuererstattung".
- [x] Merge the keyword and vector rankings by rank (reciprocal rank fusion, k = 60).
- [x] `shoebox search --method vector|keyword|hybrid` (default hybrid);
      `eval_rag.py --mode search --method …` compares them.
- [x] Measured (25 questions, `bge-m3`, 2026-11-11), correct page at rank 1 / in top 3 / in top 5:

      | Method | @1 | @3 | @5 |
      | --- | --- | --- | --- |
      | vector | 20 | 23 | 25 |
      | keyword | 19 | 21 | 23 |
      | **hybrid** | **23** | **24** | **25** |
      | hybrid with document header | 21 | 23 | 24 |

      Without the end-of-word match, hybrid lost "Steuererstattung" (the page
      says "Erstattung") and was 22 / 23 / 24.
- [x] Not done: a document header (date and summary) in front of each chunk.
      It made the search worse: all pages of a document looked alike, and the
      letter date carried the wrong year (the 2024 statement is dated 2025).
- [x] Not done: the index rows (year, tags) as a filter. The index date is the
      letter's date, not the year the document is about, so a year filter
      would remove the right document (same 2024 statement).
- Left: Q21 (2024 vs 2025 statement, both pages name both years), Q24
  ("brutto" does not match "Bruttogehalt"; 4 letters of "brutto…" is too
  little to match safely).

### 7.5 `ask` command — done
- [x] `shoebox ask "<question>"` (`ask.rs`): the 8 best chunks of the hybrid
      search, numbered `[1]`…`[8]` with file name, page and date; they fit in
      `context_size: 16384`. New role `ANSWERER` in `roles.rs`.
- [x] Strict JSON answer, like `sort`: `{found, answer, sources: [{excerpt, quote}]}`.
- [x] Plain code checks the answer: each quote must be in the excerpt it names
      (ignoring case, spacing, umlaut spelling; at least 4 characters), and each
      number in the answer must be in a source that passed or in the question.
      Otherwise "Not found in your documents"; the model's unchecked answer is
      shown below it.
- [x] Tests with the fake model: the checks (wrong excerpt, invented number,
      other number formats, numbers from the question) and an `ask` flow.
- [x] Measured (`eval_rag.py --mode ask`, 27 questions, `qwen3:8b` + `bge-m3`,
      2026-11-11): **26 of 27** correct, a source on the right page for all 25
      answerable questions, about 7–10 s per question (`find`: 17 of 27 documents,
      and no answer). The first run gave 24 of 27:
      - Q11 "Bruttoarbeitslohn 2025" was rejected because "2025" was not on the
        source page; fixed: numbers from the question are allowed.
      - Q05 "Katze": the quote was right ("Für Hunde und Katzen ist die
        Zustimmung nötig"), the answer "Nein" was wrong. Correct in the second
        run: the model's conclusions vary between runs.
- [x] Second check (`VERIFIER` in `roles.rs`): the quote check proves that the
      quoted words are in the document, not that they answer the question. Q27
      "Hauptuntersuchung" (no document has it) was answered with the
      insurance's "Hauptfälligkeit: 01.01." with a real quote. Now a second
      model call sees only the question, the answer and the source excerpts and
      says whether they answer it; if not, "Not found". Three ideas measured
      (27 questions, 2026-11-11):

      | Variant | Correct | Q27 | Q05 | s / question |
      | --- | --- | --- | --- | --- |
      | `qwen3:8b`, no second check | 26 | wrong | ok | 10–18 |
      | stricter answerer prompt | 26 | wrong ("Hauptuntersuchung … 01.01.") | ok | 18 |
      | second check (kept) | 26 | **ok** | wrong | 26 |
      | second check + `qwen3:14b` | 26 | **ok** | wrong | 142 |

      The stricter prompt made Q27 worse (the model renamed the date), so it
      was dropped. The second check rejected no correct answer. `qwen3:14b`
      gives the same result and is far too slow on 16 GB RAM.
- Open: Q05 "Katze": the source says "Für Hunde und Katzen ist die Zustimmung
  nötig"; the answer "Nein" ignores the condition, and the second check accepts
  it. It varies between runs. The time per question also varies with the load
  of the machine (10 s in the first run, 18–26 s later).

### 7.6 Measure with the test data — done
- [x] `testdata/fake/questions.tsv`: 27 questions with known answers and source
      pages, e.g. "Wie hoch ist die Kaution?" → 2.340,00 EUR, `mietvertrag.pdf` p2.
      Made by `scripts/make_fake_docs.py`, which checks that each answer is on
      its page. Kinds: fact, synonym, number, year, summary, none.
- [x] `scripts/eval_rag.py`: fills `target/eval-box/`, checks that each answer
      is in the saved page text, asks every question, scores it per kind.
- [x] Baseline with today's `find` (index rows only, `qwen3:8b`, 2026-11-11):
      **17 of 27** correct documents. fact 7/12, synonym 4/6, number 1/3,
      year 2/3, summary 1/1, none 2/2; 4 extra documents returned.
      All 10 misses are facts that are not in the one-line summary
      (Katze, Steuernummer, Zählernummer, Urlaubstage, …): `find` then returns
      nothing. This is the gap that searching the page text must close.
- [x] Measure retrieval per page: `eval_rag.py --mode search` (recall@1/3/5,
      see 7.3 for the first numbers).
- [x] Measure answers (correct? source on the right page? wrong? missed?)
      separately from retrieval: `eval_rag.py --mode ask` (numbers in 7.5).

## Not planned for now

- `verify` and `split` commands
- Trash support outside macOS
- Moving the heuristics (blank-page threshold, duplicate similarity,
  counterpart matching) into `shoebox.yaml`
