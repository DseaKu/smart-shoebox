//! `shoebox-gui`: the window, as its own program (built with the `gui` feature).
//!
//! Started from a terminal it uses the box the command line would use. Started
//! by a double click (as `Smart Shoebox.app`) there is no meaningful current
//! folder, so it uses the box chosen last time, or asks for one once.

use rfd::{MessageButtons, MessageDialog, MessageDialogResult, MessageLevel};
use smart_shoebox::{Res, config, gui, workspace::Workspace};
use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    add_tool_folders_to_path();
    if let Err(e) = run() {
        eprintln!("error: {e}");
        MessageDialog::new()
            .set_level(MessageLevel::Error)
            .set_title("Smart Shoebox")
            .set_description(e.to_string())
            .show();
        std::process::exit(1);
    }
}

fn run() -> Res<()> {
    let Some(ws) = find_box()? else {
        return Ok(());
    };
    remember(&ws);
    let config = config::load(&ws.config_path())?;
    gui::run(ws, config)
}

/// From a terminal: `SHOEBOX_DIR` or the current folder and above, like `shoebox`.
/// Otherwise the box used last time, or the one I pick now.
/// `None` if I cancel.
fn find_box() -> Res<Option<Workspace>> {
    if env::var_os("SHOEBOX_DIR").is_some() {
        return Workspace::locate().map(Some);
    }
    if let Ok(ws) = Workspace::locate() {
        return Ok(Some(ws));
    }
    if let Some(ws) = remembered() {
        return Ok(Some(ws));
    }
    choose_box()
}

/// Asks for a folder; if it holds no box, offers to create one there.
fn choose_box() -> Res<Option<Workspace>> {
    MessageDialog::new()
        .set_title("Smart Shoebox")
        .set_description(
            "Choose the folder of your shoebox (the one with shoebox.yaml), \
             or an empty folder to create a new one.",
        )
        .show();
    let Some(dir) = rfd::FileDialog::new()
        .set_title("Choose your shoebox")
        .pick_folder()
    else {
        return Ok(None);
    };
    if let Some(ws) = Workspace::open(&dir) {
        return Ok(Some(ws));
    }
    let create = MessageDialog::new()
        .set_title("Smart Shoebox")
        .set_description(format!(
            "There is no shoebox in {}. Create one here?",
            dir.display()
        ))
        .set_buttons(MessageButtons::YesNo)
        .show();
    if create == MessageDialogResult::Yes {
        Workspace::init(&dir).map(Some)
    } else {
        Ok(None)
    }
}

/// Where the last box is remembered: a one-line file next to other app data.
fn memory_file() -> Option<PathBuf> {
    let home = env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/Smart Shoebox/box-path"))
}

fn remembered() -> Option<Workspace> {
    let text = fs::read_to_string(memory_file()?).ok()?;
    Workspace::open(&PathBuf::from(text.trim()))
}

/// Best effort: if it cannot be saved, I am simply asked again next time.
fn remember(ws: &Workspace) {
    let Some(file) = memory_file() else {
        return;
    };
    if let Some(dir) = file.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let root = fs::canonicalize(ws.root()).unwrap_or_else(|_| ws.root().to_path_buf());
    let _ = fs::write(file, root.to_string_lossy().as_bytes());
}

/// Apps started from Finder get a minimal PATH without Homebrew, so `ocrmypdf`
/// would not be found. Adds the usual Homebrew folders if they are missing.
fn add_tool_folders_to_path() {
    let current = env::var_os("PATH").unwrap_or_default();
    let mut paths: Vec<PathBuf> = env::split_paths(&current).collect();
    for dir in ["/opt/homebrew/bin", "/usr/local/bin"] {
        let dir = PathBuf::from(dir);
        if dir.is_dir() && !paths.contains(&dir) {
            paths.push(dir);
        }
    }
    if let Ok(joined) = env::join_paths(paths) {
        // SAFETY: called first thing in `main`, before any other thread exists.
        unsafe { env::set_var("PATH", joined) };
    }
}
