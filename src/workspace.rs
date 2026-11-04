//! The user space ("box"): where my documents, index and settings live,
//! separate from the source code in the repo.

use crate::{Res, index};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// The settings file; it also marks a folder as a box.
pub const CONFIG_FILE: &str = "shoebox.yaml";
/// Default folder name of the box, e.g. inside the repo. Found automatically
/// from its parent folder, so `shoebox` also works from the repo root.
pub const DEFAULT_DIR: &str = "box";
const INDEX_FILE: &str = "index.tsv";
const STORE_DIR: &str = "store";
/// Points to a box anywhere on disk and overrides the search.
const ENV_VAR: &str = "SHOEBOX_DIR";
/// Default settings for `shoebox init`.
const CONFIG_TEMPLATE: &str = include_str!("../shoebox.example.yaml");

pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// `SHOEBOX_DIR` if set; otherwise the nearest folder, from the current one
    /// upwards, that contains `shoebox.yaml` itself or in a `box/` subfolder.
    pub fn locate() -> Res<Self> {
        if let Some(dir) = env::var_os(ENV_VAR) {
            let root = PathBuf::from(dir);
            if !root.join(CONFIG_FILE).is_file() {
                return Err(format!(
                    "{ENV_VAR} is set to {}, but there is no {CONFIG_FILE} in it",
                    root.display()
                )
                .into());
            }
            return Ok(Self { root });
        }
        let cwd = env::current_dir()?;
        find_root(&cwd).map(|root| Self { root }).ok_or_else(|| {
            format!(
                "no shoebox found in {} or above. Create one with: shoebox init [folder]",
                cwd.display()
            )
            .into()
        })
    }

    /// Creates a box: `shoebox.yaml` with default settings, `store/` and `index.tsv`.
    /// Files that already exist are left untouched, so running it twice is safe.
    pub fn init(dir: &Path) -> Res<Self> {
        let ws = Self {
            root: dir.to_path_buf(),
        };
        fs::create_dir_all(ws.store_dir())
            .map_err(|e| format!("cannot create {}: {e}", ws.store_dir().display()))?;
        if !ws.config_path().exists() {
            fs::write(ws.config_path(), CONFIG_TEMPLATE)?;
        }
        index::create_if_missing(&ws.index_path())?;
        Ok(ws)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn config_path(&self) -> PathBuf {
        self.root.join(CONFIG_FILE)
    }

    pub fn index_path(&self) -> PathBuf {
        self.root.join(INDEX_FILE)
    }

    pub fn store_dir(&self) -> PathBuf {
        self.root.join(STORE_DIR)
    }
}

/// Walks up from `start`; at each level checks the folder itself and its `box/`.
fn find_root(start: &Path) -> Option<PathBuf> {
    start.ancestors().find_map(|dir| {
        [dir.to_path_buf(), dir.join(DEFAULT_DIR)]
            .into_iter()
            .find(|candidate| candidate.join(CONFIG_FILE).is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_box_from_repo_and_from_inside() {
        let repo = env::temp_dir().join(format!("shoebox-test-ws-{}", std::process::id()));
        let _ = fs::remove_dir_all(&repo);
        fs::create_dir_all(repo.join("src")).unwrap();
        let ws = Workspace::init(&repo.join(DEFAULT_DIR)).unwrap();
        let boxdir = ws.root().to_path_buf();

        assert!(ws.config_path().is_file() && ws.store_dir().is_dir() && ws.index_path().is_file());
        assert_eq!(find_root(&repo), Some(boxdir.clone())); // repo root
        assert_eq!(find_root(&repo.join("src")), Some(boxdir.clone())); // source folder
        assert_eq!(find_root(&boxdir.join("store")), Some(boxdir.clone())); // inside the box
        assert_eq!(find_root(&env::temp_dir()), None);

        // init again keeps my settings.
        fs::write(ws.config_path(), "model: mine\n").unwrap();
        Workspace::init(&boxdir).unwrap();
        assert_eq!(
            fs::read_to_string(ws.config_path()).unwrap(),
            "model: mine\n"
        );

        fs::remove_dir_all(&repo).unwrap();
    }
}
