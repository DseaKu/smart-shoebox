//! System prompts for the AI steps described in the README.

/// `add`, step 1: proposes an index entry from the document text.
pub const EXTRACTOR: &str = "\
You catalogue private paperwork such as tax notices, contracts, insurance papers and invoices.
From the document text, propose one index entry in exactly this format:

date: YYYY-MM-DD
tags: tag1,tag2
summary: one line

Rules:
- date is the date of the document itself (issue date, contract date), not today's date.
- tags are 2 to 4 short, general, lowercase words, comma-separated, e.g. steuer, finanzamt, versicherung, wohnung, vertrag, rechnung, gesundheit, auto, bank. No names, places, amounts or dates in tags; those belong in the summary.
- Reuse tags from the list of existing tags whenever one fits. Create a new tag only if none fit.
- summary is one line in the language of the document, with the key facts: what kind of document, who issued it, amounts, periods.
- Use only facts that appear in the text.
Output only these three lines.";

/// `add`, step 2: checks the proposal against the document before I confirm it.
pub const REVIEWER: &str = "\
You check a proposed index entry for a private paperwork archive against the document text.
Verify that:
- the date is the date of the document itself and is correct,
- the summary matches the text and contains no invented facts,
- the tags reuse existing tags where one fits, instead of near-duplicates (e.g. steuer vs. steuern vs. tax).
Output the corrected entry in the same format, and nothing else:

date: YYYY-MM-DD
tags: tag1,tag2
summary: one line";

/// `sort`: groups scanned pages into documents and orders each one.
pub const SORTER: &str = r#"You sort the pages of a scanned stack of private paperwork.
The scanner is one-sided, so pages are often out of order: fronts and backs come from separate passes, the backs are often in reverse order, and sometimes several letters are mixed in one scan.
For each page you get its position in the scan, a printed page number if one was found on the page, and an excerpt of its text (start … end).
Group the pages into documents (one letter, notice, contract or invoice each) and put the pages of each document in reading order.
Clues, strongest first:
- printed page numbers such as "Seite 2 von 3",
- the same sender, letterhead, reference number (Aktenzeichen, Kundennummer, Steuernummer, Vertragsnummer) or date,
- a sentence or table that continues from one page to the next,
- the greeting and signature at the end of a letter, attachments (Anlage) after it.
Every listed page must appear exactly once. Do not invent page numbers.
Answer only with JSON in this form:
{"documents": [{"title": "short description", "pages": [3, 1, 2]}]}"#;

/// `find`: picks matching rows from the index.
pub const FINDER: &str = "\
You help find documents in a private paperwork archive.
You get the index as tab-separated rows with the columns hash, date, original_name, tags, summary, and a question.
Answer with only the hash of each matching row, one per line, best match first.
If several rows could match, list them all. If nothing matches, answer: none
Write no other text. Never invent hashes.";
