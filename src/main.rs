use clap::{Parser, Subcommand};
use smart_shoebox::{
    Res, add, complete, config, find, llm::Llm, merge, prompt, rename, sort, workspace,
    workspace::Workspace,
};
use std::path::PathBuf;
use std::process::exit;

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

/// Finds the box, loads its settings and sets up the model from them.
fn open_box() -> Res<(Workspace, config::Config, Llm)> {
    let ws = Workspace::locate()?;
    let config = config::load(&ws.config_path())?;
    let llm = Llm::new(&config);
    Ok((ws, config, llm))
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
            let (ws, config, llm) = open_box()?;
            add::run(&config, &ws, &llm, &file).await
        }
        Command::Find { question, to } => {
            let (ws, config, llm) = open_box()?;
            find::run(&config, &ws, &llm, &question.join(" "), to.as_deref()).await
        }
        Command::Name { files } => {
            let (ws, config, llm) = open_box()?;
            rename::run(&config, &ws, &llm, &files).await
        }
        Command::Merge { files, out } => merge::run(&files, out.as_deref()),
        Command::Sort {
            file,
            out,
            delete_blank,
        } => {
            let (_ws, config, llm) = open_box()?;
            sort::run(&config, &llm, &file, out.as_deref(), delete_blank)
                .await
                .map(|_| ())
        }
        Command::Check { file, search_in } => {
            let (ws, config, llm) = open_box()?;
            complete::check(&config, &ws, &llm, &file, search_in.as_deref()).await
        }
        Command::Counterpart { file, search_in } => {
            let (_ws, config, llm) = open_box()?;
            complete::counterpart(&config, &llm, &file, search_in.as_deref()).await
        }
    }
}
