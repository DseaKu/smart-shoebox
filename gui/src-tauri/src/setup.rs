//! The setup screen: is Ollama installed and running, and are the box's models
//! downloaded? Ollama itself is installed by the person (the official download
//! page); the app finds it, starts it and downloads the models.

use ollama_rs::Ollama;
use serde::Serialize;
use smart_shoebox::{Res, config::Config};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use tauri::{AppHandle, Emitter};
use tokio_stream::StreamExt;

/// The official download page of Ollama.
pub const DOWNLOAD_PAGE: &str = "https://ollama.com/download";

/// Local models for answers, with their download size and the RAM (GB) from
/// which they are suggested. The smallest one is the fallback.
const CHOICES: [(&str, &str, u32); 3] = [
    ("qwen3:4b", "2.5 GB", 0),
    ("qwen3:8b", "5.2 GB", 12),
    ("qwen3:14b", "9.3 GB", 32),
];

/// Download sizes of other models the app knows.
const SIZES: [(&str, &str); 1] = [("bge-m3", "1.2 GB")];

#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum OllamaState {
    /// Not found on this computer.
    Missing,
    /// Installed, but not answering.
    Stopped,
    Running,
}

#[derive(Serialize)]
pub struct ModelState {
    name: String,
    /// What the box uses it for: "answers" or "search".
    purpose: &'static str,
    installed: bool,
    /// The download size, if known ("5.2 GB").
    size: Option<&'static str>,
}

#[derive(Serialize)]
pub struct Choice {
    name: &'static str,
    size: &'static str,
}

#[derive(Serialize)]
pub struct SetupState {
    ollama: OllamaState,
    /// The models of the open box.
    models: Vec<ModelState>,
    /// Installed memory in GB, rounded; 0 if unknown.
    ram_gb: u32,
    /// The model for answers that suits this computer.
    suggested: &'static str,
    choices: Vec<Choice>,
    /// Ollama runs and every model of the box is downloaded.
    ready: bool,
}

#[derive(Serialize, Clone)]
struct PullProgress {
    model: String,
    status: String,
    completed: u64,
    total: u64,
}

/// The current state, for the box settings in `config`.
pub async fn state(config: &Config) -> SetupState {
    let installed = installed_models().await;
    let ollama = match &installed {
        Some(_) => OllamaState::Running,
        None if find_ollama().is_some() => OllamaState::Stopped,
        None => OllamaState::Missing,
    };
    let installed = installed.unwrap_or_default();
    let models: Vec<ModelState> = [(&config.model, "answers"), (&config.embed_model, "search")]
        .into_iter()
        .map(|(name, purpose)| ModelState {
            name: name.clone(),
            purpose,
            installed: installed.iter().any(|m| same_model(m, name)),
            size: size_of(name),
        })
        .collect();
    let ram_gb = ram_gb();
    SetupState {
        ready: ollama == OllamaState::Running && models.iter().all(|m| m.installed),
        ollama,
        models,
        ram_gb,
        suggested: suggested(ram_gb),
        choices: CHOICES
            .iter()
            .map(|&(name, size, _)| Choice { name, size })
            .collect(),
    }
}

/// Starts the installed Ollama and waits until it answers (at most 30 s).
pub async fn start() -> Res<()> {
    if installed_models().await.is_some() {
        return Ok(());
    }
    let launch = find_ollama().ok_or("Ollama is not installed.")?;
    let mut command = match &launch {
        Launch::MacApp(app) => {
            let mut c = Command::new("open");
            c.arg("-a").arg(app);
            c
        }
        Launch::Program(program) => Command::new(program),
        Launch::Serve(binary) => {
            let mut c = Command::new(binary);
            c.arg("serve");
            c
        }
    };
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window next to the app.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
        .spawn()
        .map_err(|e| format!("cannot start Ollama: {e}"))?;
    for _ in 0..60 {
        tokio::time::sleep(Duration::from_millis(500)).await;
        if installed_models().await.is_some() {
            return Ok(());
        }
    }
    Err("Ollama was started, but it does not answer. Try again, or restart the computer.".into())
}

/// Downloads `model` with Ollama and sends its progress as `pull-progress` events.
pub async fn pull(app: &AppHandle, model: &str) -> Res<()> {
    smart_shoebox::config::check_local(model)?;
    let mut stream = Ollama::default()
        .pull_model_stream(model.to_string(), false)
        .await
        .map_err(|e| format!("cannot download {model}: {e}"))?;
    while let Some(status) = stream.next().await {
        let status = status.map_err(|e| format!("cannot download {model}: {e}"))?;
        let _ = app.emit(
            "pull-progress",
            PullProgress {
                model: model.to_string(),
                status: status.message,
                completed: status.completed.unwrap_or(0),
                total: status.total.unwrap_or(0),
            },
        );
    }
    if !installed_models()
        .await
        .is_some_and(|names| names.iter().any(|m| same_model(m, model)))
    {
        return Err(format!("{model} was not downloaded completely. Try again.").into());
    }
    Ok(())
}

/// The installed model names, or `None` if Ollama does not answer.
async fn installed_models() -> Option<Vec<String>> {
    let ollama = Ollama::default();
    match tokio::time::timeout(Duration::from_secs(3), ollama.list_local_models()).await {
        Ok(Ok(models)) => Some(models.into_iter().map(|m| m.name).collect()),
        _ => None,
    }
}

/// `bge-m3` and `bge-m3:latest` are the same model; case does not matter.
fn same_model(a: &str, b: &str) -> bool {
    let full = |n: &str| {
        let n = n.trim().to_lowercase();
        if n.contains(':') {
            n
        } else {
            format!("{n}:latest")
        }
    };
    full(a) == full(b)
}

fn size_of(model: &str) -> Option<&'static str> {
    CHOICES
        .iter()
        .map(|&(name, size, _)| (name, size))
        .chain(SIZES)
        .find(|(name, _)| same_model(name, model))
        .map(|(_, size)| size)
}

/// The biggest model that fits the memory. Models need about their size in RAM,
/// plus the system and the app.
fn suggested(ram_gb: u32) -> &'static str {
    CHOICES
        .iter()
        .rev()
        .find(|&&(_, _, min)| ram_gb >= min)
        .map_or(CHOICES[0].0, |&(name, _, _)| name)
}

fn ram_gb() -> u32 {
    let mut system = sysinfo::System::new();
    system.refresh_memory();
    let gb = system.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
    gb.round() as u32
}

/// How to start an installed Ollama.
#[derive(Debug, PartialEq)]
enum Launch {
    /// The macOS app (`Ollama.app`), which starts the server itself.
    MacApp(PathBuf),
    /// The Windows tray program (`ollama app.exe`), which starts the server itself.
    Program(PathBuf),
    /// The plain `ollama` program: run `ollama serve`.
    Serve(PathBuf),
}

/// The installed Ollama, from the usual install places, then the PATH.
fn find_ollama() -> Option<Launch> {
    let mut places = Vec::new();
    if cfg!(target_os = "macos") {
        places.push(Launch::MacApp("/Applications/Ollama.app".into()));
        if let Some(home) = dirs::home_dir() {
            places.push(Launch::MacApp(home.join("Applications/Ollama.app")));
        }
        for dir in ["/opt/homebrew/bin", "/usr/local/bin"] {
            places.push(Launch::Serve(Path::new(dir).join("ollama")));
        }
    } else if cfg!(windows) {
        if let Some(local) = dirs::data_local_dir() {
            let dir = local.join("Programs").join("Ollama");
            places.push(Launch::Program(dir.join("ollama app.exe")));
            places.push(Launch::Serve(dir.join("ollama.exe")));
        }
    } else {
        for dir in ["/usr/local/bin", "/usr/bin"] {
            places.push(Launch::Serve(Path::new(dir).join("ollama")));
        }
        if let Some(home) = dirs::home_dir() {
            places.push(Launch::Serve(home.join(".local/bin/ollama")));
        }
    }
    let program = if cfg!(windows) {
        "ollama.exe"
    } else {
        "ollama"
    };
    if let Some(path) = std::env::var_os("PATH") {
        places.extend(std::env::split_paths(&path).map(|dir| Launch::Serve(dir.join(program))));
    }
    places.into_iter().find(|launch| match launch {
        Launch::MacApp(p) => p.is_dir(),
        Launch::Program(p) | Launch::Serve(p) => p.is_file(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_names_match_with_and_without_tag() {
        assert!(same_model("bge-m3", "bge-m3:latest"));
        assert!(same_model("Qwen3:8B", "qwen3:8b"));
        assert!(!same_model("qwen3:8b", "qwen3:4b"));
    }

    #[test]
    fn suggestion_follows_the_memory() {
        assert_eq!(suggested(0), "qwen3:4b");
        assert_eq!(suggested(8), "qwen3:4b");
        assert_eq!(suggested(16), "qwen3:8b");
        assert_eq!(suggested(64), "qwen3:14b");
    }

    #[test]
    fn known_sizes() {
        assert_eq!(size_of("bge-m3:latest"), Some("1.2 GB"));
        assert_eq!(size_of("qwen3:8b"), Some("5.2 GB"));
        assert_eq!(size_of("llama3"), None);
    }
}
