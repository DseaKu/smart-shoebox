/// The fields the AI proposes and I confirm before a row goes into `index.tsv`.
#[derive(Default, Clone)]
pub struct Entry {
    pub date: String,
    pub tags: String,
    pub summary: String,
}

impl Entry {
    /// Parses the three-line format from the roles:
    /// `date: …`, `tags: …`, `summary: …`. Returns `None` if a line is missing.
    pub fn parse(text: &str) -> Option<Entry> {
        let (mut date, mut tags, mut summary) = (None, None, None);
        for line in text.lines() {
            let line = line.trim().trim_start_matches(['-', '*', ' ']);
            let Some((key, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim().trim_matches('*').trim().to_string();
            match key.trim().trim_matches('*').to_lowercase().as_str() {
                "date" => date = Some(value),
                "tags" => tags = Some(value),
                "summary" => summary = Some(value),
                _ => {}
            }
        }
        Some(
            Entry {
                date: date?,
                tags: tags?,
                summary: summary?,
            }
            .normalized(),
        )
    }

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
        let b = self.date.as_bytes();
        b.len() == 10
            && b[4] == b'-'
            && b[7] == b'-'
            && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
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
