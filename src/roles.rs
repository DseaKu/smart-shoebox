//! System prompts for the AI steps described in the README.

/// `add`, step 1: proposes an index entry from the document text.
pub const EXTRACTOR: &str = "\
You catalogue private paperwork such as tax notices, contracts, insurance papers and invoices.
From the document text, propose one index entry.

Rules:
- date is the date of the document itself (issue date, contract date), not today's date.
- tags are 2 to 4 short, general, lowercase words, e.g. steuer, finanzamt, versicherung, wohnung, vertrag, rechnung, gesundheit, auto, bank. No names, places, amounts or dates in tags; those belong in the summary.
- Reuse tags from the list of existing tags whenever one fits. Create a new tag only if none fit.
- summary is one line in the language of the document, with the key facts: what kind of document, who issued it, amounts, periods.
- Use only facts that appear in the text.
Answer only with JSON in this form:
{\"date\": \"YYYY-MM-DD\", \"tags\": [\"tag1\", \"tag2\"], \"summary\": \"one line\"}";

/// `add`, step 2: checks the proposal against the document before I confirm it.
pub const REVIEWER: &str = "\
You check a proposed index entry (as JSON) for a private paperwork archive against the document text.
Verify that:
- the date is the date of the document itself and is correct,
- the summary matches the text and contains no invented facts,
- the tags reuse existing tags where one fits, instead of near-duplicates (e.g. steuer vs. steuern vs. tax).
Answer only with the corrected entry as JSON in the same form:
{\"date\": \"YYYY-MM-DD\", \"tags\": [\"tag1\", \"tag2\"], \"summary\": \"one line\"}";

/// `name`: proposes the date and a short title for a file name.
pub const NAMER: &str = r#"You name the files of a private paperwork archive.
From the start of the document text, propose the date and a short title.

Rules:
- date is the date of the document itself (issue date, letter date) as YYYY-MM-DD, not today's date. If there is none, write: unknown
- title says what kind of document it is and who it is from, in the language of the document, e.g. "Steuerbescheid 2025 Finanzamt München", "Mietvertrag LEG Wohnung", "Rechnung Zahnarzt".
- title has 3 to 6 words. No amounts, no names of the recipient, no account, customer or contract numbers.
Answer only with JSON in this form:
{"date": "YYYY-MM-DD", "title": "3 to 6 words"}"#;

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
pub const FINDER: &str = r#"You help find documents in a private paperwork archive.
You get rows of the index and a question. Each row starts with its number:
#12 | date | tags | summary | original file name
Answer with the numbers of the rows that match the question, best match first. Match by meaning, not only by exact words (e.g. "rental contract" matches "Mietvertrag Wohnung").
If several rows could match, list them all. If nothing matches, answer with an empty list. Never invent numbers.
Answer only with JSON in this form: {"matches": [12, 4]}"#;
