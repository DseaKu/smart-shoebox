# Test data

Documents to test `shoebox` without my private paperwork.
The PDFs are not in git (the root `.gitignore` blocks `*.pdf`). Only the notes are.

| Folder | What | Where it comes from |
| --- | --- | --- |
| `public/` | Real public samples: German e-invoices, US tax forms, German regulations | Downloaded; owner, license and URL per file in [`public/SOURCES.md`](public/SOURCES.md) |
| `fake/` | Invented private paperwork: rental contract, tax notice, power bill, insurance, employment contract, bank statement, split duplex scans | Made by [`scripts/make_fake_docs.py`](../scripts/make_fake_docs.py); all names, numbers and amounts are invented |

## Make the fake documents again

```sh
python3 scripts/make_fake_docs.py          # writes testdata/fake/*.pdf and expected.tsv
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

The fake PDFs have a text layer, so they do not test OCR.
