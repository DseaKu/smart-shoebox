# Test data

Documents to test `shoebox` without my private paperwork.
The PDFs are not in git (the root `.gitignore` blocks `*.pdf`). Only the notes are.

| Folder | What | Where it comes from |
| --- | --- | --- |
| `public/` | Real public samples: German e-invoices, US tax forms, German regulations | Downloaded; owner, license and URL per file in [`public/SOURCES.md`](public/SOURCES.md) |
| `fake/` | Invented private paperwork: rental contract, tax notice, power bill, insurance, employment contract, bank statement, split duplex scans | Made by [`scripts/make_fake_docs.py`](../scripts/make_fake_docs.py); all names, numbers and amounts are invented |

## Make the fake documents again

```sh
python3 scripts/make_fake_docs.py   # writes testdata/fake/*.pdf, expected.tsv, questions.tsv, eval_docs.txt
```

The output is the same on each run. `fake/expected.tsv` holds the correct answers:
document kind, date, page count, the `check` result and the expected counterpart.

## Test cases in `fake/`

- `scan_0001.pdf` + `scan_0002.pdf`: one 4-page statement, scanned duplex in two passes
  (fronts 1, 3; backs 4, 2 in reverse order). Each one must find the other.
- `scan_0003.pdf`: pages 2 and 4 of the same landlord's statement for the year before.
  It shares only the contract number, so it must not match `scan_0001.pdf`.
- `scan_0004.pdf` + `scan_0005.pdf`: 3-page employment contract; the back pass starts with a blank page.
- All other files are complete.
- `complete/`: the full versions of the split scans, for the questions. They are in a
  subfolder, so they do not change the counterpart results of the pile.

The fake PDFs have a text layer, so they do not test OCR.

## Questions: measure search and answers

`fake/questions.tsv` holds 27 questions with known answers, the document and the
page(s). The generator checks that each answer really is on that page.
Each question has a kind, to see which kind fails:

| Kind | What it tests |
| --- | --- |
| `fact` | the answer is on the page with the same words as the question |
| `synonym` | the question uses other words ("Was verdiene ich brutto?" → "Bruttogehalt") |
| `number` | an exact reference number (Steuernummer, meter number) |
| `year` | a similar document of another year must not be used |
| `summary` | the index summary from `add` may be wrong (it said "Nachzahlung" for a refund) |
| `none` | no document answers it; the correct answer is "not found" |

```sh
python3 scripts/eval_rag.py           # needs Ollama; first run adds the documents (slow)
python3 scripts/eval_rag.py --fresh --model qwen3:14b
python3 scripts/eval_rag.py --mode search   # correct page in the top 1, 3, 5 chunks? (needs bge-m3)
python3 scripts/eval_rag.py --mode search --method vector   # or keyword; default hybrid
```

The box is in `target/eval-box/`, the results in `target/eval-results/` (both ignored by git).
