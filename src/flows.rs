//! End-to-end tests of the commands with a fake model and generated PDFs.
//! They run without Ollama and without OCR (the test PDFs contain real text).

use crate::testutil::{FakeModel, TestBox, pdf};
use crate::{add, chunk, complete, find, index, pagetext, prompt, rename, sort, store, wizard};
use std::fs;

const TAX_NOTICE: &str =
    "Finanzamt Muenchen\nSteuerbescheid 2025 vom 14.03.2026\nNachzahlung 312 EUR";

fn entry_json(date: &str, tags: &[&str], summary: &str) -> String {
    let tags: Vec<String> = tags.iter().map(|t| format!("\"{t}\"")).collect();
    format!(
        r#"{{"date": "{date}", "tags": [{}], "summary": "{summary}"}}"#,
        tags.join(", ")
    )
}

/// Text of each page of a PDF, trimmed.
fn page_texts(path: &std::path::Path) -> Vec<String> {
    crate::pdf::text_by_pages(path)
        .unwrap()
        .into_iter()
        .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect()
}

#[tokio::test]
async fn add_stores_and_indexes_a_document_once() {
    let t = TestBox::new("add");
    let file = t.dir.join("bescheid.pdf");
    pdf(&file, &[TAX_NOTICE]);
    let model = FakeModel::new(|role, _| match role {
        "extractor" => entry_json(
            "2026-03-14",
            &["Steuer", "finanzamt"],
            "Steuerbescheid 2025",
        ),
        other => panic!("unexpected role {other}"),
    });

    prompt::script(&["y"]);
    add::run(&t.config, &t.ws, &model, &file).await.unwrap();

    let rows = index::parse(&index::read(&t.ws.index_path()).unwrap());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].date, "2026-03-14");
    assert_eq!(rows[0].tags, "steuer,finanzamt");
    assert_eq!(rows[0].original_name, "bescheid.pdf");
    assert!(store::path(&t.ws, &rows[0].hash).is_file());
    let saved = pagetext::load(&t.ws, &rows[0].hash).unwrap().unwrap();
    assert_eq!(saved.len(), 1);
    assert!(saved[0].contains("Steuerbescheid 2025"), "{saved:?}");
    assert_eq!(
        model.calls(),
        ["extractor"],
        "the reviewer is off by default"
    );

    // The same file again: nothing is asked, nothing changes.
    prompt::script(&[]);
    add::run(&t.config, &t.ws, &model, &file).await.unwrap();
    assert_eq!(
        index::parse(&index::read(&t.ws.index_path()).unwrap()).len(),
        1
    );
    assert_eq!(model.calls().len(), 1);
}

#[tokio::test]
async fn add_with_review_uses_the_reviewed_entry() {
    let mut t = TestBox::new("add-review");
    t.config.review = true;
    let file = t.dir.join("bescheid.pdf");
    pdf(&file, &[TAX_NOTICE]);
    let model = FakeModel::new(|role, _| match role {
        "extractor" => entry_json("2026-03-14", &["steuern", "tax"], "Bescheid"),
        "reviewer" => entry_json("2026-03-14", &["steuer"], "Steuerbescheid 2025"),
        other => panic!("unexpected role {other}"),
    });

    prompt::script(&["y"]);
    add::run(&t.config, &t.ws, &model, &file).await.unwrap();

    let rows = index::parse(&index::read(&t.ws.index_path()).unwrap());
    assert_eq!(rows[0].tags, "steuer");
    assert_eq!(model.calls(), ["extractor", "reviewer"]);
}

#[tokio::test]
async fn add_survives_a_model_without_usable_answers() {
    let t = TestBox::new("add-broken-model");
    let file = t.dir.join("bescheid.pdf");
    pdf(&file, &[TAX_NOTICE]);
    let model = FakeModel::new(|_, _| "I am not JSON".to_string());

    // The model fails twice; I get empty fields and decline.
    prompt::script(&["n"]);
    add::run(&t.config, &t.ws, &model, &file).await.unwrap();

    assert!(index::parse(&index::read(&t.ws.index_path()).unwrap()).is_empty());
    assert_eq!(
        model.calls(),
        ["extractor", "extractor"],
        "one retry, then give up"
    );
    assert!(
        fs::read_dir(t.ws.store_dir()).unwrap().next().is_none(),
        "nothing stored"
    );
}

#[tokio::test]
async fn find_copies_the_matching_document_under_a_readable_name() {
    let t = TestBox::new("find");
    let model = FakeModel::new(|role, _| match role {
        "extractor" => entry_json("2026-03-14", &["steuer"], "Steuerbescheid 2025 Finanzamt"),
        "finder" => r#"{"matches": [1, 99]}"#.to_string(),
        other => panic!("unexpected role {other}"),
    });
    let file = t.dir.join("bescheid.pdf");
    pdf(&file, &[TAX_NOTICE]);
    prompt::script(&["y"]);
    add::run(&t.config, &t.ws, &model, &file).await.unwrap();

    let out = t.dir.join("out");
    find::run(
        &t.config,
        &t.ws,
        &model,
        "Wo ist mein Steuerbescheid?",
        Some(&out),
    )
    .await
    .unwrap();

    let copied: Vec<String> = fs::read_dir(&out)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    // Row 99 does not exist and is ignored.
    assert_eq!(copied, ["2026-03-14_steuerbescheid-2025-finanzamt.pdf"]);
}

#[tokio::test]
async fn name_renames_in_place() {
    let t = TestBox::new("name");
    let file = t.dir.join("scan_001.pdf");
    pdf(&file, &[TAX_NOTICE]);
    let model = FakeModel::new(|role, _| match role {
        "namer" => {
            r#"{"date": "2026-03-14", "title": "Steuerbescheid 2025 Finanzamt"}"#.to_string()
        }
        other => panic!("unexpected role {other}"),
    });

    prompt::script(&["y"]);
    rename::run(&t.config, &t.ws, &model, std::slice::from_ref(&file))
        .await
        .unwrap();

    assert_eq!(t.files(), ["2026-03-14_steuerbescheid-2025-finanzamt.pdf"]);
}

#[tokio::test]
async fn sort_orders_by_printed_numbers_even_if_the_model_does_not() {
    let t = TestBox::new("sort");
    let file = t.dir.join("scan.pdf");
    pdf(
        &file,
        &[
            "Seite 2 von 3\nBerechnung der Einkommensteuer",
            "Seite 3 von 3\nRechtsbehelfsbelehrung und Gruss",
            "Seite 1 von 3\nFinanzamt Muenchen Steuerbescheid",
        ],
    );
    // The model just returns scan order.
    let model = FakeModel::new(|role, _| match role {
        "sorter" => {
            r#"{"documents": [{"title": "Steuerbescheid", "pages": [1, 2, 3]}]}"#.to_string()
        }
        other => panic!("unexpected role {other}"),
    });

    prompt::script(&["y"]);
    let out = sort::run(&t.config, &model, &file, None, false)
        .await
        .unwrap()
        .unwrap();

    let texts = page_texts(&out);
    assert!(texts[0].starts_with("Seite 1 von 3"), "{texts:?}");
    assert!(texts[1].starts_with("Seite 2 von 3"), "{texts:?}");
    assert!(texts[2].starts_with("Seite 3 von 3"), "{texts:?}");
    assert_eq!(page_texts(&file).len(), 3, "the original is unchanged");
}

#[tokio::test]
async fn check_finds_the_counterpart_merges_sorts_and_names_the_result() {
    let t = TestBox::new("check");
    let reference = "Vertragsnummer 1051.00012.06 vom 29.10.2025";
    let fronts = t.dir.join("fronts.pdf");
    let backs = t.dir.join("backs.pdf");
    pdf(
        &fronts,
        &[
            &format!("Seite 1 von 3\nNebenkostenabrechnung LEG\n{reference}"),
            &format!("Seite 3 von 3\nGuthaben und Zahlungen\n{reference}"),
        ],
    );
    pdf(
        &backs,
        &[&format!(
            "Seite 2 von 3\nHeizkosten und Wasser\n{reference}"
        )],
    );
    let model = FakeModel::new(|role, _| match role {
        "sorter" => r#"{"documents": [{"title": "Abrechnung", "pages": [1, 2, 3]}]}"#.to_string(),
        "namer" => r#"{"date": "2025-10-29", "title": "Nebenkostenabrechnung LEG"}"#.to_string(),
        other => panic!("unexpected role {other}"),
    });

    prompt::script(&[
        "y", // search for the counterpart?
        "y", // merge and sort with backs.pdf?
        "y", // write the PDF in this order?
        "n", // delete the originals?
        "y", // give the result a readable name?
        "y", // rename?
    ]);
    complete::check(&t.config, &t.ws, &model, &fronts, None)
        .await
        .unwrap();

    let result = "2025-10-29_nebenkostenabrechnung-leg.pdf";
    assert_eq!(
        t.files(),
        [
            "2025-10-29_nebenkostenabrechnung-leg.pdf",
            "backs.pdf",
            "fronts.pdf"
        ]
    );
    let texts = page_texts(&t.dir.join(result));
    assert!(texts[0].starts_with("Seite 1 von 3"), "{texts:?}");
    assert!(texts[1].starts_with("Seite 2 von 3"), "{texts:?}");
    assert!(texts[2].starts_with("Seite 3 von 3"), "{texts:?}");
    assert_eq!(model.calls(), ["sorter", "namer"]);
}

#[test]
fn counterpart_search_and_merge_work_without_questions() {
    // The GUI's path: search, show candidates, merge the chosen one.
    let t = TestBox::new("counterpart-gui");
    let reference = "Kundennummer 4711.0815 vom 03.11.2026";
    let fronts = t.dir.join("fronts.pdf");
    let backs = t.dir.join("backs.pdf");
    let other = t.dir.join("other.pdf");
    pdf(
        &fronts,
        &[
            &format!("Seite 1 von 3\nStromrechnung\n{reference}"),
            &format!("Seite 3 von 3\nZahlungen\n{reference}"),
        ],
    );
    pdf(&backs, &[&format!("Seite 2 von 3\nVerbrauch\n{reference}")]);
    pdf(&other, &["Seite 2 von 3\nEin anderer Brief 1234.5678"]);

    let diagnosis = complete::diagnose(&t.config, &t.ws, &fronts).unwrap();
    assert!(diagnosis.report.is_incomplete());
    let dir = complete::search_dir(&fronts, None);
    let candidates =
        complete::search_counterparts(&t.config, &fronts, &diagnosis.scan, &diagnosis.report, &dir)
            .unwrap()
            .candidates;
    assert_eq!(candidates.len(), 1, "other.pdf shares no references");
    assert_eq!(candidates[0].path, backs);
    assert_eq!(candidates[0].fills, [2]);
    assert!(candidates[0].still.is_empty());

    let (merged, out) = complete::merge_with(&fronts, &diagnosis.scan, &candidates[0]).unwrap();
    assert_eq!(page_texts(merged.path()).len(), 3);
    assert_eq!(out.file_name().unwrap(), "fronts_complete.pdf");
}

#[tokio::test]
async fn wizard_completes_names_and_adds_a_pile_and_leaves_the_rest_to_me() {
    let t = TestBox::new("wizard");
    let pile = t.dir.join("pile");
    fs::create_dir_all(&pile).unwrap();
    let reference = "Vertragsnummer 1051.00012.06 vom 29.10.2025";
    // Fronts and backs of a two-sided letter, scanned in two passes.
    pdf(
        &pile.join("IMG_0001.pdf"),
        &[
            &format!("Seite 1 von 3\nNebenkostenabrechnung LEG\n{reference}"),
            &format!("Seite 3 von 3\nGuthaben und Zahlungen\n{reference}"),
        ],
    );
    pdf(
        &pile.join("IMG_0002.pdf"),
        &[&format!(
            "Seite 2 von 3\nHeizkosten und Wasser\n{reference}"
        )],
    );
    // A complete one-page letter, and an identical copy of it.
    pdf(&pile.join("IMG_0003.pdf"), &[TAX_NOTICE]);
    fs::copy(pile.join("IMG_0003.pdf"), pile.join("IMG_0003 copy.pdf")).unwrap();
    // Page 2 of something whose page 1 is not in the pile.
    pdf(
        &pile.join("IMG_0004.pdf"),
        &["Seite 2 von 2\nStadtwerke Abschlag 4711.0815 vom 03.11.2026"],
    );

    let model = FakeModel::new(|role, message| match role {
        "sorter" => r#"{"documents": [{"title": "Abrechnung", "pages": [1, 2, 3]}]}"#.to_string(),
        "cataloguer" if message.contains("Nebenkosten") => r#"{"date": "2025-10-29",
            "title": "Nebenkostenabrechnung LEG", "tags": ["wohnung"],
            "summary": "Nebenkostenabrechnung 2024 der LEG, Guthaben"}"#
            .to_string(),
        "cataloguer" => r#"{"date": "2026-03-14", "title": "Steuerbescheid 2025",
            "tags": ["steuer"], "summary": "Steuerbescheid 2025, Nachzahlung 312 EUR"}"#
            .to_string(),
        other => panic!("unexpected role {other}"),
    });

    prompt::script(&[]); // it must not ask anything
    let mut log = Vec::new();
    let summary = wizard::handle(
        &t.config,
        &t.ws,
        &model,
        std::slice::from_ref(&pile),
        &mut |line| log.push(line),
    )
    .await
    .unwrap();
    let lines: Vec<String> = summary.items.iter().map(|i| i.line()).collect();

    let rows = index::parse(&index::read(&t.ws.index_path()).unwrap());
    let mut summaries: Vec<&str> = rows.iter().map(|r| r.summary.as_str()).collect();
    summaries.sort();
    assert_eq!(
        summaries,
        [
            "Nebenkostenabrechnung 2024 der LEG, Guthaben",
            "Steuerbescheid 2025, Nachzahlung 312 EUR"
        ],
        "{lines:#?}"
    );
    let completed = pile.join("2025-10-29_nebenkostenabrechnung-leg.pdf");
    let texts = page_texts(&completed);
    assert!(texts[0].starts_with("Seite 1 von 3"), "{texts:?}");
    assert!(texts[1].starts_with("Seite 2 von 3"), "{texts:?}");
    assert!(texts[2].starts_with("Seite 3 von 3"), "{texts:?}");
    assert!(pile.join("2026-03-14_steuerbescheid-2025.pdf").is_file());
    // Each added document has its page text; the completed one in sorted order.
    for row in &rows {
        let saved = pagetext::load(&t.ws, &row.hash).unwrap().unwrap();
        if row.summary.starts_with("Nebenkosten") {
            assert_eq!(saved.len(), 3, "{saved:?}");
            assert!(saved[1].contains("Seite 2 von 3"), "{saved:?}");
        } else {
            assert!(saved[0].contains("Steuerbescheid"), "{saved:?}");
        }
    }

    let needs_you: Vec<&String> = lines
        .iter()
        .filter(|l| l.starts_with("needs you"))
        .collect();
    assert_eq!(needs_you.len(), 1, "{lines:#?}");
    assert!(needs_you[0].contains("IMG_0004.pdf"), "{lines:#?}");
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("skipped: IMG_0003") && l.contains(": identical copy of")),
        "{lines:#?}"
    );
    // The two scans of the letter can go; their pages are in the box.
    let mut spare = summary.spare_originals();
    spare.sort();
    assert_eq!(
        spare,
        [pile.join("IMG_0001.pdf"), pile.join("IMG_0002.pdf")]
    );
    assert!(log.len() > lines.len(), "progress is logged as it goes");
    assert_eq!(
        model.calls(),
        ["sorter", "cataloguer", "cataloguer"],
        "one request per document for name and entry"
    );
}

#[tokio::test]
async fn text_fills_in_the_page_text_of_documents_added_before() {
    let t = TestBox::new("text-backfill");
    let file = t.dir.join("vertrag.pdf");
    pdf(
        &file,
        &[
            "Seite 1 von 2\nMietvertrag",
            "Seite 2 von 2\nKaution 2340 EUR",
        ],
    );
    let model = FakeModel::new(|_, _| entry_json("2024-02-12", &["wohnung"], "Mietvertrag"));
    prompt::script(&["y"]);
    add::run(&t.config, &t.ws, &model, &file).await.unwrap();
    let hash = index::parse(&index::read(&t.ws.index_path()).unwrap())[0]
        .hash
        .clone();

    // As if it had been added before page texts were saved.
    fs::remove_file(pagetext::path(&t.ws, &hash)).unwrap();
    pagetext::backfill(&t.config, &t.ws, false).unwrap();
    let saved = pagetext::load(&t.ws, &hash).unwrap().unwrap();
    assert_eq!(saved.len(), 2);
    assert!(saved[1].contains("Kaution"), "{saved:?}");

    // Nothing left to do; the file stays as it is.
    let before = fs::metadata(pagetext::path(&t.ws, &hash))
        .unwrap()
        .modified()
        .unwrap();
    pagetext::backfill(&t.config, &t.ws, false).unwrap();
    let after = fs::metadata(pagetext::path(&t.ws, &hash))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(before, after);

    // A missing PDF is an error for that document, not a crash.
    fs::remove_file(pagetext::path(&t.ws, &hash)).unwrap();
    fs::remove_file(store::path(&t.ws, &hash)).unwrap();
    let err = pagetext::backfill(&t.config, &t.ws, false)
        .unwrap_err()
        .to_string();
    assert!(err.contains("1 documents could not be read"), "{err}");
}

#[tokio::test]
async fn chunks_of_the_box_come_from_the_saved_page_text() {
    let t = TestBox::new("chunks");
    let file = t.dir.join("vertrag.pdf");
    pdf(
        &file,
        &[
            "Seite 1 von 2\nMietvertrag",
            "Seite 2 von 2\nKaution 2340 EUR",
        ],
    );
    let model = FakeModel::new(|_, _| entry_json("2024-02-12", &["wohnung"], "Mietvertrag"));
    prompt::script(&["y"]);
    add::run(&t.config, &t.ws, &model, &file).await.unwrap();
    let hash = index::parse(&index::read(&t.ws.index_path()).unwrap())[0]
        .hash
        .clone();

    let found = chunk::of_box(&t.ws).unwrap();
    assert!(found.without_text.is_empty());
    let pages: Vec<u32> = found.chunks.iter().map(|c| c.page).collect();
    assert_eq!(pages, [1, 2]);
    assert!(found.chunks[1].text.contains("Kaution 2340 EUR"));
    assert!(found.chunks.iter().all(|c| c.hash == hash));

    // A document without page text is reported, not skipped silently.
    fs::remove_file(pagetext::path(&t.ws, &hash)).unwrap();
    let found = chunk::of_box(&t.ws).unwrap();
    assert!(found.chunks.is_empty());
    assert_eq!(found.without_text, [hash]);
}
