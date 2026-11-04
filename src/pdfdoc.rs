//! Page-level PDF editing with lopdf: merging scans and writing a chosen set of pages.

use crate::Res;
use lopdf::{Document, Object, ObjectId, dictionary};
use std::path::{Path, PathBuf};

/// Page attributes a page may inherit from its parent nodes in the page tree.
/// They are copied onto each page before the tree is rebuilt, so nothing gets lost.
const INHERITABLE: [&[u8]; 4] = [b"Resources", b"MediaBox", b"CropBox", b"Rotate"];

/// Merges the PDFs into one file, keeping input order and page order.
pub fn merge(inputs: &[PathBuf], out: &Path) -> Res<()> {
    let mut merged = Document::with_version("1.5");
    let mut pages = Vec::new();
    let mut next_id = 1;
    for path in inputs {
        let mut doc = load(path)?;
        doc.renumber_objects_with(next_id);
        next_id = doc.max_id + 1;
        let ids: Vec<ObjectId> = doc.get_pages().into_values().collect();
        flatten_inherited(&mut doc, &ids)?;
        pages.extend(ids);
        merged.objects.extend(doc.objects);
    }
    merged.max_id = next_id;
    assemble(&mut merged, &pages)?;
    save(&mut merged, out)
}

/// Writes a new PDF with only the given pages of `src` (1-based), in exactly that order.
/// Number of pages in a PDF.
pub fn page_count(path: &Path) -> Res<usize> {
    Ok(load(path)?.get_pages().len())
}

pub fn extract_pages(src: &Path, page_numbers: &[u32], out: &Path) -> Res<()> {
    let mut doc = load(src)?;
    let all = doc.get_pages();
    let ids = page_numbers
        .iter()
        .map(|n| {
            all.get(n)
                .copied()
                .ok_or_else(|| format!("page {n} does not exist in {}", src.display()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    flatten_inherited(&mut doc, &ids)?;
    assemble(&mut doc, &ids)?;
    save(&mut doc, out)
}

/// Where to write a result: the explicit `--out` path (which must not exist yet),
/// or `<input-stem>_<suffix>.pdf` next to the input, with `-2`, `-3`, … if taken.
pub fn output_path(explicit: Option<&Path>, input: &Path, suffix: &str) -> Res<PathBuf> {
    if let Some(path) = explicit {
        if path.exists() {
            return Err(format!("{} already exists", path.display()).into());
        }
        return Ok(path.to_path_buf());
    }
    // An empty parent (input in the current folder) joins to a plain file name.
    let dir = input.parent().unwrap_or(Path::new(""));
    let stem = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "scan".to_string());
    Ok(crate::naming::free_path(dir, &format!("{stem}_{suffix}")))
}

fn load(path: &Path) -> Res<Document> {
    crate::pdf::guarded(path, || Document::load(path))
}

fn save(doc: &mut Document, out: &Path) -> Res<()> {
    doc.save(out)
        .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
    Ok(())
}

/// Copies inherited attributes from the page tree onto each page itself.
fn flatten_inherited(doc: &mut Document, pages: &[ObjectId]) -> Res<()> {
    for &id in pages {
        let mut inherited = Vec::new();
        let page = doc.get_dictionary(id)?;
        for key in INHERITABLE {
            if page.has(key) {
                continue;
            }
            let mut parent = page.get(b"Parent").and_then(Object::as_reference).ok();
            // The depth limit protects against broken files with a cycle in the tree.
            for _ in 0..64 {
                let Some(parent_id) = parent else { break };
                let Ok(node) = doc.get_dictionary(parent_id) else {
                    break;
                };
                if let Ok(value) = node.get(key) {
                    inherited.push((key, value.clone()));
                    break;
                }
                parent = node.get(b"Parent").and_then(Object::as_reference).ok();
            }
        }
        let page = doc.get_dictionary_mut(id)?;
        for (key, value) in inherited {
            page.set(key.to_vec(), value);
        }
    }
    Ok(())
}

/// Replaces the page tree and catalog with a flat tree of exactly `pages`,
/// then drops every object that is no longer referenced.
fn assemble(doc: &mut Document, pages: &[ObjectId]) -> Res<()> {
    let pages_id = doc.new_object_id();
    for &id in pages {
        doc.get_dictionary_mut(id)?.set("Parent", pages_id);
    }
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => pages.iter().map(|&id| Object::Reference(id)).collect::<Vec<_>>(),
            "Count" => pages.len() as u32,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    doc.prune_objects();
    doc.renumber_objects();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::Stream;
    use lopdf::content::{Content, Operation};

    fn temp(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("shoebox-test-{}-{name}.pdf", std::process::id()))
    }

    /// A PDF with one page per label. Resources and MediaBox sit only on the
    /// Pages node, so the pages inherit them, which is what real scans often do.
    fn fixture(path: &Path, labels: &[&str]) {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font_id = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Courier",
        });
        let resources_id =
            doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font_id } });
        let mut kids: Vec<Object> = Vec::new();
        for label in labels {
            let content = Content {
                operations: vec![
                    Operation::new("BT", vec![]),
                    Operation::new("Tf", vec!["F1".into(), 24.into()]),
                    Operation::new("Td", vec![100.into(), 600.into()]),
                    Operation::new("Tj", vec![Object::string_literal(*label)]),
                    Operation::new("ET", vec![]),
                ],
            };
            let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
            let page_id = doc.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
            });
            kids.push(page_id.into());
        }
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => labels.len() as u32,
                "Resources" => resources_id,
                "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
            }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog_id);
        doc.save(path).unwrap();
    }

    fn page_texts(path: &Path) -> Vec<String> {
        pdf_extract::extract_text_by_pages(path)
            .unwrap()
            .into_iter()
            .map(|t| t.trim().to_string())
            .collect()
    }

    #[test]
    fn merge_then_reorder_and_subset() {
        let (a, b, merged, out) = (temp("a"), temp("b"), temp("merged"), temp("out"));
        fixture(&a, &["Alpha", "Bravo"]);
        fixture(&b, &["Charlie"]);

        merge(&[a.clone(), b.clone()], &merged).unwrap();
        assert_eq!(page_texts(&merged), ["Alpha", "Bravo", "Charlie"]);

        extract_pages(&merged, &[3, 1], &out).unwrap();
        assert_eq!(page_texts(&out), ["Charlie", "Alpha"]);

        // Inherited attributes survived the rebuild of the page tree.
        let doc = Document::load(&out).unwrap();
        for id in doc.get_pages().values() {
            let page = doc.get_dictionary(*id).unwrap();
            assert!(page.has(b"MediaBox") && page.has(b"Resources"));
        }

        for p in [a, b, merged, out] {
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn missing_page_is_an_error() {
        let (src, out) = (temp("src"), temp("missing-out"));
        fixture(&src, &["Alpha"]);
        assert!(extract_pages(&src, &[2], &out).is_err());
        let _ = std::fs::remove_file(src);
    }
}
