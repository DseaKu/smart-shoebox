mod commands;
mod complete;
mod config;
mod entry;
mod export;
mod hash;
mod index;
mod llm;
mod naming;
mod pages;
mod pdf;
mod pdfdoc;
mod pipeline;
mod prompt;
mod rename;
mod roles;
mod scan;
mod sort;
mod store;
mod workspace;

use std::env;
use std::path::{Path, PathBuf};
use std::process::exit;
use workspace::Workspace;

type Res<T> = Result<T, Box<dyn std::error::Error>>;

fn usage(program: &str) -> ! {
    eprintln!("Usage: {program} init [folder]          (default: {})", workspace::DEFAULT_DIR);
    eprintln!("       {program} add <file.pdf>");
    eprintln!("       {program} find \"<question>\" [--to <folder>]");
    eprintln!("       {program} name <file.pdf>...");
    eprintln!("       {program} merge <a.pdf> <b.pdf>... [--out <file.pdf>]");
    eprintln!("       {program} sort <scan.pdf> [--out <file.pdf>] [--delete-blank]");
    eprintln!("       {program} check <scan.pdf> [--in <folder>]");
    eprintln!("       {program} counterpart <scan.pdf> [--in <folder>]");
    eprintln!();
    eprintln!("Options: -y, --yes   answer yes to every confirmation (skips my review)");
    eprintln!();
    eprintln!("add, find, name, sort, check and counterpart use the nearest shoebox: the current folder or one above it,");
    eprintln!("or its {}/ subfolder. Set SHOEBOX_DIR to use one somewhere else.", workspace::DEFAULT_DIR);
    exit(1);
}

/// Finds the box and loads its settings.
fn open_box() -> Res<(Workspace, config::Config)> {
    let ws = Workspace::locate()?;
    let config = config::load(&ws.config_path())?;
    Ok((ws, config))
}

/// Splits arguments into input files and an optional `--out <file>`.
fn files_and_out(program: &str, args: &[String]) -> (Vec<PathBuf>, Option<PathBuf>) {
    let mut files = Vec::new();
    let mut out = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "--out" {
            let Some(path) = rest.next() else {
                usage(program);
            };
            out = Some(PathBuf::from(path));
        } else {
            files.push(PathBuf::from(arg));
        }
    }
    (files, out)
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("error: {e}");
        exit(1);
    }
}

async fn run() -> Res<()> {
    let mut args: Vec<String> = env::args().collect();
    // `-y` / `--yes` may appear anywhere; remove it before reading the command.
    let count = args.len();
    args.retain(|arg| arg != "-y" && arg != "--yes");
    if args.len() != count {
        prompt::assume_yes();
    }
    // Only the binary's name, not the full path it was started from.
    let program = args
        .first()
        .and_then(|arg| Path::new(arg).file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("shoebox");

    match args.get(1).map(String::as_str) {
        Some("init") => {
            let dir = args.get(2).map_or(workspace::DEFAULT_DIR, String::as_str);
            let ws = Workspace::init(Path::new(dir))?;
            println!("Shoebox ready in {}/", ws.root().display());
            println!("  settings:  {}", ws.config_path().display());
            println!("  index:     {}", ws.index_path().display());
            println!("  documents: {}/", ws.store_dir().display());
            Ok(())
        }
        Some("add") => {
            let Some(file) = args.get(2) else {
                usage(program);
            };
            let (ws, config) = open_box()?;
            commands::add(&config, &ws, Path::new(file)).await
        }
        Some("find") => {
            let mut to = None;
            let mut words = Vec::new();
            let mut rest = args[2..].iter();
            while let Some(arg) = rest.next() {
                if arg == "--to" {
                    let Some(dir) = rest.next() else {
                        usage(program);
                    };
                    to = Some(PathBuf::from(dir));
                } else {
                    words.push(arg.as_str());
                }
            }
            let question = words.join(" ");
            if question.trim().is_empty() {
                usage(program);
            }
            let (ws, config) = open_box()?;
            commands::find(&config, &ws, &question, to.as_deref()).await
        }
        Some("name") => {
            let files: Vec<PathBuf> = args[2..].iter().map(PathBuf::from).collect();
            if files.is_empty() {
                usage(program);
            }
            let (ws, config) = open_box()?;
            rename::run(&config, &ws, &files).await
        }
        Some("merge") => {
            let (files, out) = files_and_out(program, &args[2..]);
            if files.len() < 2 {
                usage(program);
            }
            commands::merge(&files, out.as_deref())
        }
        Some("sort") => {
            let mut rest = args[2..].to_vec();
            let count = rest.len();
            rest.retain(|arg| arg != "--delete-blank");
            let delete_blank = rest.len() != count;
            let (files, out) = files_and_out(program, &rest);
            let [file] = files.as_slice() else {
                usage(program);
            };
            let (_ws, config) = open_box()?;
            sort::run(&config, file, out.as_deref(), delete_blank).await.map(|_| ())
        }
        Some(command @ ("check" | "counterpart")) => {
            let (file, search_in) = file_and_in(program, &args[2..]);
            let (_ws, config) = open_box()?;
            if command == "check" {
                complete::check(&config, &file, search_in.as_deref()).await
            } else {
                complete::counterpart(&config, &file, search_in.as_deref()).await
            }
        }
        _ => usage(program),
    }
}

/// Exactly one file, plus an optional `--in <folder>` to search in.
fn file_and_in(program: &str, args: &[String]) -> (PathBuf, Option<PathBuf>) {
    let mut files = Vec::new();
    let mut search_in = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if arg == "--in" {
            let Some(dir) = rest.next() else {
                usage(program);
            };
            search_in = Some(PathBuf::from(dir));
        } else {
            files.push(PathBuf::from(arg));
        }
    }
    let [file] = files.as_slice() else {
        usage(program);
    };
    (file.clone(), search_in)
}
