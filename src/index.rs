use crate::entry::{Entry, clean};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;

const HEADER: &str = "hash\tdate\toriginal_name\ttags\tsummary";

/// Returns the contents of the index, or an empty string if it does not exist yet.
pub fn read(path: &Path) -> crate::Res<String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
        Err(e) => Err(format!("cannot read {}: {e}", path.display()).into()),
    }
}

/// Creates an index with only the header row, unless one exists already.
pub fn create_if_missing(path: &Path) -> crate::Res<()> {
    if !path.exists() {
        fs::write(path, format!("{HEADER}\n"))
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    Ok(())
}

/// One document row of the index.
#[derive(Clone)]
pub struct Row {
    pub hash: String,
    pub date: String,
    pub tags: String,
    pub summary: String,
    pub original_name: String,
}

/// Parses all document rows.
pub fn parse(index: &str) -> Vec<Row> {
    rows(index)
        .map(|line| {
            let mut cols = line.split('\t').map(str::to_string);
            let mut next = || cols.next().unwrap_or_default();
            let (hash, date, original_name, tags, summary) =
                (next(), next(), next(), next(), next());
            Row {
                hash,
                date,
                tags,
                summary,
                original_name,
            }
        })
        .collect()
}

/// True if the index already has a row with this hash.
pub fn contains(index: &str, hash: &str) -> bool {
    rows(index).any(|row| row.split('\t').next() == Some(hash))
}

/// True if the index has no document rows yet.
pub fn is_empty(index: &str) -> bool {
    rows(index).next().is_none()
}

/// Collects the distinct tags from the `tags` column.
pub fn tags(index: &str) -> Vec<String> {
    let mut tags: Vec<String> = rows(index)
        .filter_map(|line| line.split('\t').nth(3))
        .flat_map(|field| field.split(','))
        .map(|tag| tag.trim().to_string())
        .filter(|tag| !tag.is_empty())
        .collect();
    tags.sort();
    tags.dedup();
    tags
}

/// Appends one row, writing the header first if the file is new or empty.
pub fn append(path: &Path, hash: &str, original_name: &str, entry: &Entry) -> crate::Res<()> {
    let is_new = fs::metadata(path).map(|m| m.len() == 0).unwrap_or(true);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    if is_new {
        writeln!(file, "{HEADER}")?;
    }
    writeln!(
        file,
        "{hash}\t{}\t{}\t{}\t{}",
        entry.date,
        clean(original_name),
        entry.tags,
        entry.summary
    )?;
    Ok(())
}

/// Document rows: every non-empty line after the header.
fn rows(index: &str) -> impl Iterator<Item = &str> {
    index.lines().skip(1).filter(|line| !line.trim().is_empty())
}
