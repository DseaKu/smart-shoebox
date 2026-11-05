//! End-to-end tests of the commands with a fake model and generated PDFs.
//! They run without Ollama and without OCR (the test PDFs contain real text).

use crate::testutil::{FakeModel, TestBox, pdf};
use crate::{add, complete, find, index, prompt, rename, sort, store};
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
async fn check_reports_and_offers_a_readable_name() {
    let t = TestBox::new("check");
    let fronts = t.dir.join("IMG_0001.pdf");
    pdf(
        &fronts,
        &[
            "Seite 1 von 3\nNebenkostenabrechnung LEG vom 29.10.2025",
            "Seite 3 von 3\nGuthaben und Zahlungen",
        ],
    );
    let model = FakeModel::new(|role, _| match role {
        "namer" => r#"{"date": "2025-10-29", "title": "Nebenkostenabrechnung LEG"}"#.to_string(),
        other => panic!("unexpected role {other}"),
    });

    let diagnosis = complete::diagnose(&t.config, &t.ws, &fronts).unwrap();
    assert_eq!(diagnosis.report.missing, [2]);
    assert!(!diagnosis.readable_name && !diagnosis.in_box);

    prompt::script(&[
        "y", // give it a readable name?
        "y", // rename?
    ]);
    complete::check(&t.config, &t.ws, &model, &fronts)
        .await
        .unwrap();
    assert_eq!(t.files(), ["2025-10-29_nebenkostenabrechnung-leg.pdf"]);
    assert_eq!(model.calls(), ["namer"]);
}
