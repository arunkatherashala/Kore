//! The World T sandbox on disk (tool layer, outside `being/`).
//!
//! The pool of generated data files of one world is written to a directory under the owner's data
//! root (`<data root>/<owner>/world_t/seed_<seed>/`), and the being's engine reads them back through
//! `DirSandbox`. Every path goes through `safety::safe_path` (no `..`, no absolute path outside the
//! owner directory, no symlink escape); file names are checked to be plain names. Nothing is written
//! anywhere else, and nothing is read from anywhere else: the data is generated, never copied from the
//! user's files.

use crate::being::world_t::engine::Sandbox;
use crate::being::world_t::world::WorldT;
use crate::safety;
use std::path::PathBuf;

/// Largest file the sandbox hands to a tool.
pub const MAX_FILE: u64 = 4 * 1024 * 1024;

pub fn sandbox_rel(seed: u64) -> String {
    format!("world_t/seed_{seed}")
}

fn plain_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 80 || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.') || name.starts_with('.') {
        return Err(format!("not a plain sandbox file name: {name:?}"));
    }
    Ok(())
}

fn path_of(owner: &str, rel: &str, name: &str) -> Result<PathBuf, String> {
    plain_name(name)?;
    safety::safe_path(owner, &format!("{rel}/{name}"))
}

/// Write every file of the world's pool into the sandbox directory (idempotent: same bytes). Returns
/// the number of files and bytes written.
pub fn write_pool(owner: &str, w: &WorldT) -> Result<(usize, usize), String> {
    let rel = sandbox_rel(w.cfg.seed);
    let dir = safety::safe_path(owner, &rel)?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create the sandbox: {e}"))?;
    let mut bytes = 0;
    for f in &w.files {
        let p = path_of(owner, &rel, &f.name)?;
        if std::fs::read(&p).map_or(true, |b| b != f.bytes) {
            std::fs::write(&p, &f.bytes).map_err(|e| format!("cannot write {}: {e}", f.name))?;
        }
        bytes += f.bytes.len();
    }
    Ok((w.files.len(), bytes))
}

/// Remove the files this module wrote for a world (and the directory if it is then empty).
pub fn remove_pool(owner: &str, w: &WorldT) -> Result<(), String> {
    let rel = sandbox_rel(w.cfg.seed);
    for f in &w.files {
        let p = path_of(owner, &rel, &f.name)?;
        if p.exists() {
            std::fs::remove_file(&p).map_err(|e| format!("cannot remove {}: {e}", f.name))?;
        }
    }
    let dir = safety::safe_path(owner, &rel)?;
    let _ = std::fs::remove_dir(&dir);
    Ok(())
}

/// Reads sandbox files through `safe_path`. Each file is read from disk once per `DirSandbox` and then kept in
/// memory (the pool does not change during a run; a file read per step was the measurement's main cost).
pub struct DirSandbox {
    pub owner: String,
    pub rel: String,
    read_once: std::cell::RefCell<std::collections::HashMap<String, Vec<u8>>>,
}

impl DirSandbox {
    pub fn new(owner: &str, seed: u64) -> DirSandbox {
        DirSandbox { owner: owner.to_string(), rel: sandbox_rel(seed), read_once: Default::default() }
    }
}

impl Sandbox for DirSandbox {
    fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        plain_name(name)?;
        if let Some(b) = self.read_once.borrow().get(name) {
            return Ok(b.clone());
        }
        let b = self.read_disk(name)?;
        self.read_once.borrow_mut().insert(name.to_string(), b.clone());
        Ok(b)
    }
}

impl DirSandbox {
    fn read_disk(&self, name: &str) -> Result<Vec<u8>, String> {
        let p = path_of(&self.owner, &self.rel, name)?;
        let m = std::fs::metadata(&p).map_err(|e| format!("{name}: {e}"))?;
        if !m.is_file() || m.len() > MAX_FILE {
            return Err(format!("{name}: not a file of at most {MAX_FILE} bytes"));
        }
        std::fs::read(&p).map_err(|e| format!("{name}: {e}"))
    }
}
