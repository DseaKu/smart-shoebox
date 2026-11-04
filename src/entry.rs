/// The fields the AI proposes and I confirm before a row goes into `index.tsv`.
#[derive(Default, Clone)]
pub struct Entry {
    pub date: String,
    pub tags: String,
    pub summary: String,
}

impl Entry {
    /// Lowercase, comma-separated tags without spaces, and no tabs or newlines anywhere,
    /// so the entry is always exactly one valid TSV row.
    pub fn normalized(self) -> Entry {
        let tags = self
            .tags
            .split(',')
            .map(|t| t.trim().to_lowercase())
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join(",");
        Entry {
            date: clean(&self.date),
            tags: clean(&tags),
            summary: clean(&self.summary),
        }
    }

    /// True if the date looks like YYYY-MM-DD.
    pub fn has_valid_date(&self) -> bool {
        crate::naming::is_iso_date(&self.date)
    }
}

/// Replaces tabs and line breaks with spaces and trims.
pub fn clean(value: &str) -> String {
    value
        .replace(['\t', '\n', '\r'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
