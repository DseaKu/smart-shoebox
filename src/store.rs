use crate::workspace::Workspace;
use std::fs;
use std::path::{Path, PathBuf};

/// Where a document with this hash lives: `<box>/store/<hash>.pdf`.
pub fn path(ws: &Workspace, hash: &str) -> PathBuf {
    ws.store_dir().join(format!("{hash}.pdf"))
}

/// Copies the file into the store, creating `store/` if needed.
/// Copies to a temporary name first, so an interrupted copy never looks like a finished one.
pub fn copy(ws: &Workspace, src: &Path, hash: &str) -> crate::Res<PathBuf> {
    let dir = ws.store_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let dest = path(ws, hash);
    if !dest.exists() {
        let tmp = dest.with_extension("pdf.part");
        fs::copy(src, &tmp).map_err(|e| format!("cannot copy into {}: {e}", dir.display()))?;
        fs::rename(&tmp, &dest)?;
    }
    Ok(dest)
}
