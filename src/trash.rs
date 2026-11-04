//! "Deleting" originals by moving them to the macOS Trash, so they can still be restored.

use crate::Res;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// Moves the file into `~/.Trash`, renaming it if a file of that name is already there.
/// Refuses (instead of deleting for good) if there is no Trash folder.
pub fn move_to_trash(path: &Path) -> Res<PathBuf> {
    let trash = env::var_os("HOME")
        .map(|home| PathBuf::from(home).join(".Trash"))
        .filter(|dir| dir.is_dir())
        .ok_or("no Trash folder (~/.Trash) found, so the file was kept")?;
    let name = path
        .file_name()
        .ok_or_else(|| format!("{} is not a file", path.display()))?;
    let mut dest = trash.join(name);
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let mut n = 2;
    while dest.exists() {
        dest = trash.join(format!("{stem} {n}.pdf"));
        n += 1;
    }
    // A rename fails across drives (e.g. a file on a USB stick); copy and remove instead.
    if fs::rename(path, &dest).is_err() {
        fs::copy(path, &dest).map_err(|e| format!("cannot move {} to the Trash: {e}", path.display()))?;
        fs::remove_file(path).map_err(|e| format!("cannot remove {}: {e}", path.display()))?;
    }
    Ok(dest)
}

/// Asks once whether to move all `files` to the Trash, and does it.
pub fn offer(files: &[PathBuf]) -> Res<()> {
    let names: Vec<String> = files.iter().map(|f| f.display().to_string()).collect();
    let question = format!(
        "Delete the original file{} {}? They go to the Trash. [y]es / [n]o: ",
        if files.len() == 1 { "" } else { "s" },
        names.join(", ")
    );
    if !crate::prompt::ask_yes_no(&question)? {
        return Ok(());
    }
    for file in files {
        match move_to_trash(file) {
            Ok(_) => println!("Moved {} to the Trash.", file.display()),
            Err(e) => eprintln!("error: {}: {e}", file.display()),
        }
    }
    Ok(())
}
