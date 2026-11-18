//! "Deleting" originals by moving them to the Trash (macOS, Linux) or the Recycle Bin
//! (Windows), so they can still be restored.

use crate::Res;
use std::path::{Path, PathBuf};

/// Moves the file to the system's Trash. Refuses (instead of deleting for good)
/// if the system has no Trash for it, for example on some network drives.
pub fn move_to_trash(path: &Path) -> Res<()> {
    if !path.is_file() {
        return Err(format!("{} is not a file", path.display()).into());
    }
    #[allow(unused_mut)]
    let mut context = ::trash::TrashContext::default();
    // The Finder method asks for permission to control Finder and plays a sound;
    // NSFileManager needs neither.
    #[cfg(target_os = "macos")]
    {
        use ::trash::macos::{DeleteMethod, TrashContextExtMacos};
        context.set_delete_method(DeleteMethod::NsFileManager);
    }
    context
        .delete(path)
        .map_err(|e| format!("cannot move it to the Trash, so it was kept ({e})").into())
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
