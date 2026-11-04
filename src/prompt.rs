use crate::{
    Res,
    entry::Entry,
    pages::{self, Page},
    sort::{self, Group},
};
use std::io::{self, Write};

/// Shows the new page order and asks me to accept, edit or reject it.
/// Returns `None` if I reject it.
pub fn confirm_sort(mut groups: Vec<Group>, pages: &[Page]) -> Res<Option<Vec<Group>>> {
    let all: Vec<u32> = pages.iter().map(|p| p.number).collect();
    let content: Vec<u32> = pages.iter().filter(|p| p.is_content()).map(|p| p.number).collect();
    loop {
        println!();
        for (i, group) in groups.iter().enumerate() {
            let title = if group.title.is_empty() { "(no title)" } else { &group.title };
            println!("Document {}: {title}", i + 1);
            for &n in &group.pages {
                let page = &pages[n as usize - 1];
                let printed = page
                    .printed_number
                    .as_ref()
                    .map(|p| format!("[{p}] "))
                    .unwrap_or_default();
                println!("  page {n:>3}  {printed}{}", pages::preview(&page.text, 70));
            }
            if !group.missing.is_empty() {
                let missing: Vec<String> = group.missing.iter().map(u32::to_string).collect();
                println!("  warning: printed page {} is not in the scan", missing.join(", "));
            }
        }
        let unplaced = |p: &&Page| !groups.iter().any(|g| g.pages.contains(&p.number));
        let blank_at_end: Vec<String> = pages
            .iter()
            .filter(unplaced)
            .filter(|p| p.blank)
            .map(|p| p.number.to_string())
            .collect();
        if !blank_at_end.is_empty() {
            println!("Blank pages, moved to the end: {}", blank_at_end.join(", "));
        }
        let duplicates: Vec<String> = pages
            .iter()
            .filter(unplaced)
            .filter_map(|p| p.duplicate_of.map(|o| format!("{} (same as {o})", p.number)))
            .collect();
        if !duplicates.is_empty() {
            println!("Duplicate scans, left out: {}", duplicates.join(", "));
        }
        println!();

        match ask("Write the PDF in this order? [y]es / [e]dit / [n]o: ")?.as_deref() {
            Some("y" | "yes") => return Ok(Some(groups)),
            Some("n" | "no") | None => return Ok(None),
            Some("e" | "edit") => {
                println!("Type the new page order, e.g. 1,6,4,5,2 (';' groups pages: 1,6,4; 5,2).");
                println!("Every page must be listed, except blank pages (they go to the end)");
                println!("and duplicate scans (left out unless you list them).");
                let Some(line) = ask("> ")? else { return Ok(None) };
                match sort::parse_groups(&line)
                    .and_then(|g| sort::validate(&g, &all, &content).map(|_| g))
                {
                    Ok(edited) => groups = edited,
                    Err(e) => println!("Not changed: {e}"),
                }
            }
            Some(_) => println!("Please answer y, e or n."),
        }
    }
}

/// Shows the entry and asks me to accept, edit or reject it.
/// Returns `None` if I reject it.
pub fn confirm(mut entry: Entry) -> Res<Option<Entry>> {
    loop {
        println!();
        println!("  date:    {}", entry.date);
        println!("  tags:    {}", entry.tags);
        println!("  summary: {}", entry.summary);
        if !entry.has_valid_date() {
            println!("  (warning: date is not in YYYY-MM-DD format)");
        }
        println!();

        match ask("Add this entry? [y]es / [e]dit / [n]o: ")?.as_deref() {
            Some("y" | "yes") => return Ok(Some(entry)),
            Some("n" | "no") | None => return Ok(None),
            Some("e" | "edit") => {
                println!("Press Enter to keep a value.");
                entry = Entry {
                    date: edit("date", &entry.date)?,
                    tags: edit("tags", &entry.tags)?,
                    summary: edit("summary", &entry.summary)?,
                }
                .normalized();
            }
            Some(_) => println!("Please answer y, e or n."),
        }
    }
}

/// Reads one trimmed, lowercased answer. `None` means stdin was closed.
fn ask(prompt: &str) -> Res<Option<String>> {
    print!("{prompt}");
    io::stdout().flush()?;
    let mut line = String::new();
    if io::stdin().read_line(&mut line)? == 0 {
        return Ok(None);
    }
    Ok(Some(line.trim().to_lowercase()))
}

/// Asks for a new value; an empty answer keeps the current one.
fn edit(label: &str, current: &str) -> Res<String> {
    print!("{label} [{current}]: ");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    let line = line.trim();
    Ok(if line.is_empty() { current.to_string() } else { line.to_string() })
}
