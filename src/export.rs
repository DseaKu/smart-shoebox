use crate::{Res, hash, index::Row, naming, store, workspace::Workspace};
use std::fs;
use std::path::{Path, PathBuf};

pub enum Copied {
    New(PathBuf),
    AlreadyThere(PathBuf),
}

/// Copies the stored document into `dir` under a readable name, e.g.
/// `2026-03-14_steuerbescheid-finanzamt-muenchen-nachzahlung.pdf`.
/// Never overwrites: if the name is taken by a different file, `-2`, `-3`, … is
/// appended; if the identical file is already there, nothing is copied.
pub fn copy_to(ws: &Workspace, row: &Row, dir: &Path) -> Res<Copied> {
    let src = store::path(ws, &row.hash);
    if !src.exists() {
        return Err(format!(
            "{} is in the index but missing from the store",
            src.display()
        )
        .into());
    }
    fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;

    let stem = file_stem(row);
    let mut dest = dir.join(format!("{stem}.pdf"));
    let mut n = 2;
    while dest.exists() {
        if hash::sha256_file(&dest)? == row.hash {
            return Ok(Copied::AlreadyThere(dest));
        }
        dest = dir.join(format!("{stem}-{n}.pdf"));
        n += 1;
    }
    fs::copy(&src, &dest).map_err(|e| format!("cannot copy to {}: {e}", dest.display()))?;
    Ok(Copied::New(dest))
}

/// `<date>_<summary-slug>`, falling back to tags and then to the hash.
fn file_stem(row: &Row) -> String {
    naming::stem(&row.date, &[&row.summary, &row.tags.replace(',', " ")])
        .unwrap_or_else(|| row.hash.chars().take(12).collect())
}
