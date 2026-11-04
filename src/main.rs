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
mod search;
mod sort;
mod store;
mod trash;
mod workspace;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::exit;
use workspace::Workspace;

type Res<T> = Result<T, Box<dyn std::error::Error>>;

/// Smart Shoebox: the shoebox for your paperwork, except you can actually find things in it.
#[derive(Parser)]
#[command(
    name = "shoebox",
    version,
    after_help = "add, find, name, sort, check and counterpart use the nearest box: the current \
folder or one above it, or its box/ subfolder. Set SHOEBOX_DIR to use one somewhere else."
)]
struct Cli {
    /// Answer yes to every confirmation (skips my review)
    #[arg(short = 'y', long = "yes", global = true)]
    yes: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Create a box: settings, index and store (existing files are kept)
    Init {
        /// Where to create it
        #[arg(default_value = workspace::DEFAULT_DIR)]
        folder: PathBuf,
    },
    /// Add a PDF: AI proposes date, tags and summary, I confirm, it is stored
    Add {
        /// The PDF to add
        #[arg(value_name = "PDF")]
        file: PathBuf,
    },
    /// Find documents by asking a question in plain language
    Find {
        /// The question, e.g. "Wo ist mein Mietvertrag?" (quotes are optional)
        #[arg(required = true, num_args = 1..)]
        question: Vec<String>,
        /// Copy the matches into this folder under readable names
        #[arg(long, value_name = "FOLDER")]
        to: Option<PathBuf>,
    },
    /// Rename PDFs in place to a readable name like 2026-03-14_steuerbescheid-2025.pdf
    Name {
        /// One or more PDFs
        #[arg(required = true, num_args = 1.., value_name = "PDF")]
        files: Vec<PathBuf>,
    },
    /// Append PDFs, in the given order, into one PDF
    Merge {
        /// Two or more PDFs, in the order they should appear
        #[arg(required = true, num_args = 2.., value_name = "PDF")]
        files: Vec<PathBuf>,
        /// Output file (default: <first>_merged.pdf)
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,
    },
    /// Put the pages of one PDF into reading order
    Sort {
        /// The scan to sort
        #[arg(value_name = "PDF")]
        file: PathBuf,
        /// Output file (default: <file>_sorted.pdf)
        #[arg(long, value_name = "FILE")]
        out: Option<PathBuf>,
        /// Delete blank pages without asking
        #[arg(long)]
        delete_blank: bool,
    },
    /// Check a PDF: complete pages, readable name, already in the box; offers fixes
    Check {
        /// The PDF to check
        #[arg(value_name = "PDF")]
        file: PathBuf,
        /// Where to look for a counterpart (default: the file's folder)
        #[arg(long = "in", value_name = "FOLDER")]
        search_in: Option<PathBuf>,
    },
    /// Look for the missing pages of a PDF in another PDF, then merge and sort
    Counterpart {
        /// The incomplete PDF
        #[arg(value_name = "PDF")]
        file: PathBuf,
        /// Where to look (default: the file's folder)
        #[arg(long = "in", value_name = "FOLDER")]
        search_in: Option<PathBuf>,
    },
}

/// Finds the box and loads its settings.
fn open_box() -> Res<(Workspace, config::Config)> {
    let ws = Workspace::locate()?;
    let config = config::load(&ws.config_path())?;
    Ok((ws, config))
}

#[tokio::main]
async fn main() {
    // Invalid arguments make clap print a message (with suggestions) and exit with code 2.
    let cli = Cli::parse();
    if cli.yes {
        prompt::assume_yes();
    }
    if let Err(e) = run(cli.command).await {
        eprintln!("error: {e}");
        exit(1);
    }
}

async fn run(command: Command) -> Res<()> {
    match command {
        Command::Init { folder } => {
            let ws = Workspace::init(&folder)?;
            println!("Shoebox ready in {}/", ws.root().display());
            println!("  settings:  {}", ws.config_path().display());
            println!("  index:     {}", ws.index_path().display());
            println!("  documents: {}/", ws.store_dir().display());
            Ok(())
        }
        Command::Add { file } => {
            let (ws, config) = open_box()?;
            commands::add(&config, &ws, &file).await
        }
        Command::Find { question, to } => {
            let (ws, config) = open_box()?;
            commands::find(&config, &ws, &question.join(" "), to.as_deref()).await
        }
        Command::Name { files } => {
            let (ws, config) = open_box()?;
            rename::run(&config, &ws, &files).await
        }
        Command::Merge { files, out } => commands::merge(&files, out.as_deref()),
        Command::Sort { file, out, delete_blank } => {
            let (_ws, config) = open_box()?;
            sort::run(&config, &file, out.as_deref(), delete_blank).await.map(|_| ())
        }
        Command::Check { file, search_in } => {
            let (ws, config) = open_box()?;
            complete::check(&config, &ws, &file, search_in.as_deref()).await
        }
        Command::Counterpart { file, search_in } => {
            let (_ws, config) = open_box()?;
            complete::counterpart(&config, &file, search_in.as_deref()).await
        }
    }
}
