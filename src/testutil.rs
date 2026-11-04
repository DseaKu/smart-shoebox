//! Test helpers: a fake model, generated PDFs and a temporary box.

use crate::{Res, config::Config, llm::Model, roles, workspace::Workspace};
use lopdf::content::{Content, Operation};
use lopdf::{Document, Object, Stream, dictionary};
use schemars::JsonSchema;
use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};

/// A model with fixed answers. `answer(role, message)` gets the role's short
/// name ("extractor", "reviewer", "namer", "finder", "sorter") and returns the
/// reply. Every call is recorded in `calls`.
type Answerer = Box<dyn Fn(&str, &str) -> String>;

pub struct FakeModel {
    answer: Answerer,
    pub calls: RefCell<Vec<&'static str>>,
}

impl FakeModel {
    pub fn new(answer: impl Fn(&str, &str) -> String + 'static) -> Self {
        Self {
            answer: Box::new(answer),
            calls: RefCell::new(Vec::new()),
        }
    }

    pub fn calls(&self) -> Vec<&'static str> {
        self.calls.borrow().clone()
    }
}

impl Model for FakeModel {
    fn name(&self) -> &str {
        "fake"
    }

    async fn ask_structured<T: JsonSchema>(&self, role: &str, message: &str) -> Res<String> {
        let name = role_name(role);
        self.calls.borrow_mut().push(name);
        Ok((self.answer)(name, message))
    }
}

fn role_name(role: &str) -> &'static str {
    [
        (roles::EXTRACTOR, "extractor"),
        (roles::REVIEWER, "reviewer"),
        (roles::NAMER, "namer"),
        (roles::FINDER, "finder"),
        (roles::SORTER, "sorter"),
    ]
    .iter()
    .find(|(text, _)| *text == role)
    .map(|(_, name)| *name)
    .unwrap_or("unknown")
}

/// Writes a PDF with one page per entry; `\n` starts a new line on the page.
/// The text is real text (Courier), so no OCR is needed to read it.
pub fn pdf(path: &Path, pages: &[&str]) {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier",
    });
    let mut kids: Vec<Object> = Vec::new();
    for text in pages {
        let mut operations = vec![
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec!["F1".into(), 12.into()]),
            Operation::new("Td", vec![50.into(), 780.into()]),
        ];
        for (i, line) in text.lines().enumerate() {
            if i > 0 {
                operations.push(Operation::new("Td", vec![0.into(), (-16).into()]));
            }
            operations.push(Operation::new("Tj", vec![Object::string_literal(line)]));
        }
        operations.push(Operation::new("ET", vec![]));
        let content_id = doc.add_object(Stream::new(
            dictionary! {},
            Content { operations }.encode().unwrap(),
        ));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
            "Resources" => dictionary! { "Font" => dictionary! { "F1" => font_id } },
            "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
        });
        kids.push(page_id.into());
    }
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages", "Kids" => kids, "Count" => pages.len() as u32,
        }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    doc.save(path).unwrap();
}

/// A fresh folder with a box in `box/`, deleted again when the test ends.
pub struct TestBox {
    /// The folder around the box, for input PDFs.
    pub dir: PathBuf,
    pub ws: Workspace,
    pub config: Config,
}

impl TestBox {
    pub fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("shoebox-flow-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let ws = Workspace::init(&dir.join("box")).unwrap();
        let config = Config {
            model: "fake".to_string(),
            context_size: 16384,
            ocr_language: "deu".to_string(),
            review: false,
        };
        Self { dir, ws, config }
    }

    /// Names of the files directly in `dir` (not in the box), sorted.
    pub fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }
}

impl Drop for TestBox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}
