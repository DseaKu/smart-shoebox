//! `sort`: the AI puts the pages of one PDF into reading order.
//! It never adds, drops or splits pages; the result has exactly the same pages.

use crate::{
    Res,
    config::Config,
    llm::Llm,
    pages::{self, Page},
    pdfdoc, prompt, roles,
    scan::Scan,
};
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Pages that belong together (one letter, notice, …), in reading order.
/// The AI groups pages because that makes ordering easier; the output is still one PDF
/// with the groups one after another.
#[derive(Deserialize, JsonSchema, Clone, Debug, PartialEq)]
pub struct Group {
    #[serde(default)]
    pub title: String,
    pub pages: Vec<u32>,
    /// Printed page numbers that are not in the scan (e.g. page 6 was never scanned).
    #[serde(skip)]
    pub missing: Vec<u32>,
}

#[derive(Deserialize, JsonSchema)]
struct Plan {
    documents: Vec<Group>,
}

/// OCR if needed → AI orders the non-blank pages → I confirm or edit
/// → one PDF with all pages in the new order (blank pages at the end,
/// or left out with `delete_blank`). Returns the written file, or `None` if I said no.
pub async fn run(
    config: &Config,
    input: &Path,
    out: Option<&Path>,
    delete_blank: bool,
) -> Res<Option<PathBuf>> {
    if !input.is_file() {
        return Err(format!("{} does not exist", input.display()).into());
    }
    let out = pdfdoc::output_path(out, input, "sorted")?;

    // The result is built from the OCR'd copy when there is one, so it is
    // searchable and straightened, and `add` needs no second OCR.
    let scan = Scan::read(input, &config.ocr_language)?;
    let pages = &scan.pages;
    let content: Vec<u32> = pages.iter().filter(|p| p.is_content()).map(|p| p.number).collect();
    if content.is_empty() {
        return Err("no text found on any page".into());
    }

    eprintln!("Asking {} to sort {} pages…", config.model, content.len());
    let llm = Llm::new(config);
    let mut groups = ask_for_groups(&llm, pages, &content).await?;
    // The AI decides which pages belong together; printed page numbers decide
    // the order, because small models are unreliable at ordering by them.
    order_by_printed_numbers(&mut groups, pages);

    let Some(groups) = prompt::confirm_sort(groups, pages, delete_blank)? else {
        println!("Nothing written.");
        return Ok(None);
    };

    // Blank pages I did not place myself: ask whether to delete them,
    // unless `--delete-blank` already said so.
    let leftover_blank: Vec<String> = pages
        .iter()
        .filter(|p| p.blank && !groups.iter().any(|g| g.pages.contains(&p.number)))
        .map(|p| p.number.to_string())
        .collect();
    let delete_blank = delete_blank
        || (!leftover_blank.is_empty()
            && prompt::ask_yes_no(&format!(
                "Delete blank page{} {}? [y]es / [n]o (no: they go to the end): ",
                if leftover_blank.len() == 1 { "" } else { "s" },
                leftover_blank.join(", ")
            ))?);

    let order = final_order(&groups, pages, delete_blank);
    pdfdoc::extract_pages(scan.source(), &order, &out)?;
    println!("Wrote {} ({} pages)", out.display(), order.len());
    Ok(Some(out))
}

/// The result: the groups one after another, then any blank page not placed yet
/// (unless `delete_blank`). Pages placed by hand are always kept; duplicates
/// are left out unless they were placed by hand.
pub fn final_order(groups: &[Group], pages: &[Page], delete_blank: bool) -> Vec<u32> {
    let mut order: Vec<u32> = groups.iter().flat_map(|g| g.pages.iter().copied()).collect();
    let placed: HashSet<u32> = order.iter().copied().collect();
    order.extend(
        pages
            .iter()
            .filter(|p| p.blank && !delete_blank && !placed.contains(&p.number))
            .map(|p| p.number),
    );
    order
}

/// Orders each group by printed page numbers. Pages without a number fill the gaps
/// in the AI's order (often page 1, which has the letterhead but no "Seite 1");
/// any left over go to the end. Gaps that remain are recorded as missing pages.
/// A group with a repeated number holds more than one document, so it keeps the AI's order.
fn order_by_printed_numbers(groups: &mut [Group], pages: &[Page]) {
    let printed = |p: u32| pages[p as usize - 1].printed_number;
    for group in groups {
        let numbers: Vec<u32> = group.pages.iter().filter_map(|&p| printed(p).map(|x| x.n)).collect();
        let used: HashSet<u32> = numbers.iter().copied().collect();
        if numbers.is_empty() || used.len() != numbers.len() {
            continue;
        }
        let last = group
            .pages
            .iter()
            .filter_map(|&p| printed(p))
            .map(|x| x.total.unwrap_or(x.n).max(x.n))
            .max()
            .unwrap_or(0);
        let mut gaps = (1..=last).filter(|n| !used.contains(n));
        let mut extra = last;
        let mut keyed: Vec<(u32, u32)> = group
            .pages
            .iter()
            .map(|&p| {
                let key = match printed(p) {
                    Some(x) => x.n,
                    None => gaps.next().unwrap_or_else(|| {
                        extra += 1;
                        extra
                    }),
                };
                (key, p)
            })
            .collect();
        group.missing = gaps.collect();
        keyed.sort_by_key(|&(key, _)| key);
        group.pages = keyed.into_iter().map(|(_, p)| p).collect();
    }
}

/// Asks the model for a plan; retries once with the error, then falls back to
/// scan order so the work is never lost. The printed page numbers still order
/// the pages afterwards, so the fallback usually gives a good result too.
async fn ask_for_groups(llm: &Llm, pages: &[Page], content: &[u32]) -> Res<Vec<Group>> {
    let message = describe(pages, content.len());
    let (answer, error) = match attempt(llm, &message, content).await {
        Ok(groups) => return Ok(groups),
        Err(failure) => failure,
    };

    eprintln!("The model's sorting failed ({error}), asking again…");
    let retry = if answer.is_empty() {
        format!("{message}\n\nYour previous answer failed: {error}\nAnswer again, briefly, with valid JSON.")
    } else {
        format!(
            "{message}\n\nYour previous answer was invalid: {error}\nPrevious answer: {answer}\nAnswer again with valid JSON."
        )
    };
    match attempt(llm, &retry, content).await {
        Ok(groups) => Ok(groups),
        Err((_, e)) => {
            eprintln!("Still failing ({e}). Using scan order, ordered by printed page numbers; you can edit it.");
            Ok(vec![Group {
                title: String::new(),
                pages: content.to_vec(),
                missing: Vec::new(),
            }])
        }
    }
}

/// One request. On failure returns the answer (empty if there was none) and why.
/// An answer the model aborted, e.g. because it got stuck repeating itself,
/// counts as an invalid answer instead of ending the whole run.
async fn attempt(llm: &Llm, message: &str, content: &[u32]) -> Result<Vec<Group>, (String, String)> {
    let answer = llm
        .ask_structured::<Plan>(roles::SORTER, message)
        .await
        .map_err(|e| (String::new(), e.to_string()))?;
    parse_plan(&answer, content).map_err(|e| (answer, e))
}

/// The non-blank pages as the model sees them.
fn describe(pages: &[Page], count: usize) -> String {
    let mut out = format!(
        "The scan has {count} pages (blank pages and duplicate scans were already removed).\n\n"
    );
    for page in pages.iter().filter(|p| p.is_content()) {
        out.push_str(&format!("Page {}", page.number));
        if let Some(n) = &page.printed_number {
            out.push_str(&format!(" (printed page number: {n})"));
        }
        out.push_str(&format!(":\n{}\n\n", pages::excerpt(&page.text)));
    }
    out
}

/// Parses the model's JSON and checks that every page is used exactly once.
fn parse_plan(answer: &str, content: &[u32]) -> Result<Vec<Group>, String> {
    // Some models wrap the JSON in ``` fences or add a sentence; take the object only.
    let json = match (answer.find('{'), answer.rfind('}')) {
        (Some(start), Some(end)) if start < end => &answer[start..=end],
        _ => return Err("no JSON object in the answer".into()),
    };
    let plan: Plan = serde_json::from_str(json).map_err(|e| format!("not valid JSON: {e}"))?;
    let groups: Vec<Group> = plan.documents.into_iter().filter(|g| !g.pages.is_empty()).collect();
    validate(&groups, content, content)?;
    Ok(groups)
}

/// Checks a plan: only `allowed` pages, none twice, and every `required` page present.
pub fn validate(groups: &[Group], allowed: &[u32], required: &[u32]) -> Result<(), String> {
    if groups.is_empty() {
        return Err("no pages".into());
    }
    let mut seen = HashSet::new();
    for page in groups.iter().flat_map(|g| &g.pages) {
        if !allowed.contains(page) {
            return Err(format!("page {page} does not exist"));
        }
        if !seen.insert(*page) {
            return Err(format!("page {page} is used twice"));
        }
    }
    let missing: Vec<String> = required
        .iter()
        .filter(|p| !seen.contains(p))
        .map(u32::to_string)
        .collect();
    if !missing.is_empty() {
        return Err(format!("pages {} are missing", missing.join(", ")));
    }
    Ok(())
}

/// Parses a typed page order like `1,6,4,5,2`, optionally grouped with `;`: `1,6,4; 5,2`.
pub fn parse_groups(line: &str) -> Result<Vec<Group>, String> {
    line.split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| {
            let pages = part
                .split(',')
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(|p| p.parse::<u32>().map_err(|_| format!("'{p}' is not a page number")))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Group {
                title: String::new(),
                pages,
                missing: Vec::new(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(pages: &[u32]) -> Group {
        Group {
            title: String::new(),
            pages: pages.to_vec(),
            missing: Vec::new(),
        }
    }

    /// Distinct filler text per page, so pages are not detected as duplicates.
    fn page(i: usize, header: &str) -> String {
        format!("{header} Inhalt nummer{i} wort{i}a wort{i}b wort{i}c wort{i}d wort{i}e")
    }

    #[test]
    fn printed_numbers_decide_the_order_and_fill_gaps() {
        // Scan: p1 = "Seite 4", p2 = letterhead without number (really page 1),
        // p3 = "Seite 2", p4 = "Seite 3", p5 = "Seite 5"; nothing for page 6, then "Seite 7".
        let pages = pages::analyze(vec![
            page(1, "Seite 4"),
            page(2, "LEG Wohnen Postfach, Details auf der Seite 3"),
            page(3, "Seite 2"),
            page(4, "Seite 3"),
            page(5, "Seite 5"),
            page(6, "Seite 7"),
        ]);
        // The AI just returned scan order.
        let mut groups = vec![group(&[1, 2, 3, 4, 5, 6])];
        order_by_printed_numbers(&mut groups, &pages);
        assert_eq!(groups[0].pages, [2, 3, 4, 1, 5, 6]);
        assert_eq!(groups[0].missing, [6]);
    }

    #[test]
    fn repeated_numbers_keep_the_ai_order() {
        let pages = pages::analyze(vec![
            page(1, "Seite 2"),
            page(2, "Seite 1"),
            page(3, "Seite 1"),
        ]);
        let mut groups = vec![group(&[2, 1, 3])];
        order_by_printed_numbers(&mut groups, &pages);
        assert_eq!(groups[0].pages, [2, 1, 3]);
    }

    #[test]
    fn duplicates_are_left_out_unless_placed() {
        let text = page(1, "Seite 1");
        let pages = pages::analyze(vec![text.clone(), text, "".into()]);
        assert_eq!(pages[1].duplicate_of, Some(1));
        assert_eq!(final_order(&[group(&[1])], &pages, false), [1, 3]);
        assert_eq!(final_order(&[group(&[1, 2])], &pages, false), [1, 2, 3]);
    }

    #[test]
    fn schema_for_the_model_has_only_the_answer_fields() {
        let schema = serde_json::to_string(&schemars::schema_for!(Plan)).unwrap();
        assert!(schema.contains("documents") && schema.contains("pages") && schema.contains("title"));
        assert!(!schema.contains("missing"), "internal field leaked into the schema: {schema}");
    }

    #[test]
    fn accepts_a_complete_plan() {
        let answer = r#"```json
{"documents": [{"title": "Steuerbescheid", "pages": [3, 1]}, {"title": "Rechnung", "pages": [4]}]}
```"#;
        let groups = parse_plan(answer, &[1, 3, 4]).unwrap();
        assert_eq!(groups[0].pages, [3, 1]);
        assert_eq!(groups[1].title, "Rechnung");
    }

    #[test]
    fn rejects_bad_plans() {
        let content = [1, 2, 3];
        assert!(validate(&[group(&[1, 2])], &content, &content).unwrap_err().contains("missing"));
        assert!(validate(&[group(&[1, 2, 2, 3])], &content, &content).unwrap_err().contains("twice"));
        assert!(validate(&[group(&[1, 2, 3, 9])], &content, &content).unwrap_err().contains("9"));
        assert!(validate(&[], &content, &content).is_err());
        assert!(parse_plan("no json here", &content).is_err());
        assert!(parse_plan(r#"{"documents": "oops"}"#, &content).is_err());
    }

    #[test]
    fn edit_may_place_blank_pages_but_not_drop_content() {
        // Pages 1–4, page 2 is blank: it may be placed, but 1, 3 and 4 must all be there.
        let (all, content) = ([1, 2, 3, 4], [1, 3, 4]);
        assert!(validate(&[group(&[3, 2, 1, 4])], &all, &content).is_ok());
        assert!(validate(&[group(&[3, 1, 4])], &all, &content).is_ok());
        assert!(validate(&[group(&[3, 1])], &all, &content).is_err());
    }

    #[test]
    fn keeps_every_page_and_puts_unplaced_blanks_last() {
        let pages = pages::analyze(vec![
            "Seite eins mit genug Text darauf".into(),
            "".into(),
            "Seite drei mit genug Text darauf".into(),
        ]);
        assert_eq!(final_order(&[group(&[3, 1])], &pages, false), [3, 1, 2]);
        assert_eq!(final_order(&[group(&[3, 2, 1])], &pages, false), [3, 2, 1]);
        // With --delete-blank, unplaced blank pages are dropped, placed ones are kept.
        assert_eq!(final_order(&[group(&[3, 1])], &pages, true), [3, 1]);
        assert_eq!(final_order(&[group(&[3, 2, 1])], &pages, true), [3, 2, 1]);
    }

    #[test]
    fn parses_typed_page_order() {
        assert_eq!(parse_groups("3,1,2; 5, 4;").unwrap(), [group(&[3, 1, 2]), group(&[5, 4])]);
        assert_eq!(parse_groups("1,6,4,5,2").unwrap(), [group(&[1, 6, 4, 5, 2])]);
        assert!(parse_groups("1, x").is_err());
    }
}
