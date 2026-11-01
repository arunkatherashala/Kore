//! Stand-in for `src/authorize.rs` inside the measurement example (that file needs the whole log tool chain).
//! Only `perm_problem` is used by the advisor command; this is the same function, same body.
use std::path::Path;

pub fn perm_problem(path: &Path) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(m) = std::fs::metadata(path) {
            if m.permissions().mode() & 0o022 != 0 {
                return Some(format!("{} is group- or world-writable; run chmod go-w on it", path.display()));
            }
        }
    }
    let _ = path;
    None
}
