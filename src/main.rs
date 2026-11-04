mod commands;
mod config;
mod entry;
mod export;
mod hash;
mod index;
mod llm;
mod pages;
mod pdf;
mod pdfdoc;
mod pipeline;
mod prompt;
mod roles;
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
    eprintln!("       {program} merge <a.pdf> <b.pdf>... [--out <file.pdf>]");
    eprintln!("       {program} sort <scan.pdf> [--out <file.pdf>]");
    eprintln!();
    eprintln!("add, find and sort use the nearest shoebox: the current folder or one above it,");
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
    let args: Vec<String> = env::args().collect();
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
        Some("merge") => {
            let (files, out) = files_and_out(program, &args[2..]);
            if files.len() < 2 {
                usage(program);
            }
            commands::merge(&files, out.as_deref())
        }
        Some("sort") => {
            let (files, out) = files_and_out(program, &args[2..]);
            let [file] = files.as_slice() else {
                usage(program);
            };
            let (_ws, config) = open_box()?;
            sort::run(&config, file, out.as_deref()).await
        }
        _ => usage(program),
    }
}
