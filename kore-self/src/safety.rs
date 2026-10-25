//! Safety helpers shared by every module: data root, path confinement,
//! char-boundary truncation, SQL literal escaping, bearer-token checks,
//! bind-address policy and private file permissions.
//!
//! Nothing here is clever on purpose: small, auditable, tested.

use std::net::{IpAddr, SocketAddr};
use std::path::{Component, Path, PathBuf};

/// Environment variable that overrides the data root.
pub const HOME_ENV: &str = "KORE_SELF_HOME";
/// Environment variable holding the API bearer token.
pub const TOKEN_ENV: &str = "KORE_SELF_TOKEN";
/// Maximum accepted length of an owner name.
pub const MAX_OWNER_LEN: usize = 64;

// ─── Data root ───────────────────────────────────────────────────────────────

fn real_home_dir() -> PathBuf {
    std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

/// The one data root: `$KORE_SELF_HOME` if set and non-empty, else `~/.kore-self`.
///
/// In test builds the env var MUST be set (see `test_support::init`) and must
/// not point at the real `~/.kore-self`; otherwise this panics, so a test can
/// never touch real user data by accident.
pub fn data_root() -> PathBuf {
    let from_env = std::env::var(HOME_ENV).ok().filter(|v| !v.trim().is_empty());
    let root = match from_env {
        Some(v) => PathBuf::from(v),
        None => real_home_dir().join(".kore-self"),
    };
    #[cfg(test)]
    {
        let real = real_home_dir().join(".kore-self");
        assert!(
            std::env::var(HOME_ENV).is_ok() && root != real,
            "isolation guard: tests must call safety::test_support::init() before touching the data root"
        );
    }
    root
}

// ─── Owner names and paths ───────────────────────────────────────────────────

/// Strictly validate an owner name (used as a directory name).
pub fn validate_owner(owner: &str) -> Result<(), String> {
    if owner.is_empty() {
        return Err("owner name is empty".into());
    }
    if owner.chars().count() > MAX_OWNER_LEN {
        return Err(format!("owner name longer than {MAX_OWNER_LEN} characters"));
    }
    if owner == "." || owner == ".." || owner.starts_with('.') || owner.ends_with('.') {
        return Err("owner name may not start or end with '.'".into());
    }
    if owner.contains("..") {
        return Err("owner name may not contain '..'".into());
    }
    if !owner.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.') {
        return Err("owner name may only contain letters, digits, '_', '-' and '.'".into());
    }
    Ok(())
}

/// Directory of an owner. Invalid names are mapped to a harmless placeholder
/// (defence in depth: entry points call `validate_owner` and reject first).
pub fn owner_dir(owner: &str) -> PathBuf {
    let name = if validate_owner(owner).is_ok() { owner } else { "_invalid_owner" };
    data_root().join(name)
}

/// Resolve a user-supplied path so that it is guaranteed to lie inside
/// `data_root()/<owner>/`.
///
/// * relative paths are taken relative to the owner directory;
/// * absolute paths are accepted only if they already point inside it;
/// * `..` components, NUL bytes, and symlinks that escape are rejected.
///
/// The target does not need to exist (for writes); its deepest existing
/// ancestor is canonicalised.
pub fn safe_path(owner: &str, user_path: &str) -> Result<PathBuf, String> {
    validate_owner(owner)?;
    let p = user_path.trim();
    if p.is_empty() {
        return Err("empty path".into());
    }
    if p.contains('\0') {
        return Err("path contains NUL".into());
    }
    let dir = owner_dir(owner);
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create owner dir: {e}"))?;
    let base = dir.canonicalize().map_err(|e| format!("cannot resolve owner dir: {e}"))?;

    let raw = Path::new(p);
    if raw.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err("path may not contain '..'".into());
    }
    let joined = if raw.is_absolute() { raw.to_path_buf() } else { base.join(raw) };

    // Walk up to the deepest existing ancestor (symlink_metadata also sees dangling links).
    let mut existing = joined.clone();
    let mut tail: Vec<std::ffi::OsString> = Vec::new();
    while std::fs::symlink_metadata(&existing).is_err() {
        match (existing.file_name().map(|n| n.to_os_string()), existing.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name);
                existing = parent.to_path_buf();
            }
            _ => return Err("path cannot be resolved".into()),
        }
    }
    let mut resolved = existing
        .canonicalize()
        .map_err(|e| format!("path cannot be resolved (dangling link?): {e}"))?;
    for part in tail.iter().rev() {
        resolved.push(part);
    }
    if !resolved.starts_with(&base) {
        return Err(format!("path escapes the data directory of owner '{owner}'"));
    }
    // Files the user edits by hand (ingest allow-list, redaction patterns, tag rules) are
    // never reachable through tool paths, for reading or for writing.
    if resolved.parent() == Some(base.as_path()) {
        let n = resolved.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        if n.starts_with("ingest_roots") || n.starts_with("redact_patterns") || n.starts_with("tag_rules") || is_hand_edited_name(&n) {
            return Err("that file is reserved: edit it by hand, tools may not read or write it".into());
        }
    }
    Ok(resolved)
}

/// Files in the owner directory that only the human edits or creates by hand:
/// the chat policy, the thinker command and the kill switch.
pub const HAND_EDITED_FILES: [&str; 3] = ["autonomy_policy.txt", "thinker.txt", "KILL"];

/// True if a lower-cased file name (as found in the owner directory) is one of
/// `HAND_EDITED_FILES` or a variant that Windows would resolve to it (trailing
/// dots or spaces, an NTFS stream suffix) or an editor backup of the policy files.
pub fn is_hand_edited_name(lower: &str) -> bool {
    let base = lower.split(':').next().unwrap_or("").trim_end_matches(|c| c == '.' || c == ' ');
    base == "kill" || base.starts_with("autonomy_policy") || base.starts_with("thinker")
}

/// Like `safe_path` but returns a user-presentable error value for tool results.
pub fn safe_path_or_err(owner: &str, user_path: &str) -> Result<PathBuf, String> {
    safe_path(owner, user_path).map_err(|e| format!("Rejected path '{}': {e}", trunc(user_path, 80)))
}

// ─── Text helpers ────────────────────────────────────────────────────────────

/// Truncate to at most `max_bytes` bytes, never splitting a UTF-8 character.
pub fn trunc(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// The date part (`YYYY-MM-DD`) of an ISO timestamp, never panicking.
pub fn date_part(ts: &str) -> &str {
    ts.get(..10).unwrap_or(ts)
}

/// Quote a string as a SQL literal: single quotes doubled, NUL bytes dropped.
///
/// Note: the current KORE SQL lexer has no `''` escape, so a value containing a
/// quote makes the statement fail to parse (fail closed: no rows, no injection).
pub fn sql_lit(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        match c {
            '\'' => out.push_str("''"),
            '\0' => {}
            c => out.push(c),
        }
    }
    out.push('\'');
    out
}

/// True if the SQL statement reads files from disk (`COPY ...`, `LOAD TABLE ...`).
/// Such statements are refused over the network and in tools; file loading goes
/// through `safe_path` instead.
pub fn sql_reads_files(sql: &str) -> bool {
    let upper = sql.trim_start().to_ascii_uppercase();
    upper.starts_with("COPY") || upper.starts_with("LOAD")
}

// ─── Network policy ──────────────────────────────────────────────────────────

/// Constant-time byte comparison (length is not hidden, content is).
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    let mut diff = (a.len() ^ b.len()) as u64;
    let n = a.len().max(b.len());
    for i in 0..n {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        diff |= (x ^ y) as u64;
    }
    diff == 0
}

/// Check an `Authorization` header value against the expected token.
pub fn bearer_ok(header_value: Option<&str>, token: &str) -> bool {
    let Some(v) = header_value else { return false };
    let v = v.trim();
    let Some((scheme, rest)) = v.split_once(' ') else { return false };
    scheme.eq_ignore_ascii_case("bearer") && ct_eq(rest.trim().as_bytes(), token.as_bytes())
}

/// Find a header (case-insensitive) in a raw HTTP request head.
pub fn http_header<'a>(req: &'a str, name: &str) -> Option<&'a str> {
    let head = req.split("\r\n\r\n").next().unwrap_or(req);
    head.lines().skip(1).find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim())
    })
}

/// Minimum accepted token length when a token is mandatory.
pub const MIN_TOKEN_LEN: usize = 16;

/// Decide the listen address.
///
/// Default is `127.0.0.1:<port>`. Any other address needs an explicit
/// `--bind` AND a token (>= 16 chars) in `token`.
pub fn resolve_bind(bind: Option<&str>, port: u16, token: Option<&str>) -> Result<SocketAddr, String> {
    let Some(b) = bind else {
        return Ok(SocketAddr::from(([127, 0, 0, 1], port)));
    };
    let addr: SocketAddr = if let Ok(sa) = b.parse::<SocketAddr>() {
        sa
    } else if let Ok(ip) = b.parse::<IpAddr>() {
        SocketAddr::new(ip, port)
    } else {
        return Err(format!("--bind '{b}' is not an IP address"));
    };
    if !addr.ip().is_loopback() {
        match token {
            Some(t) if t.len() >= MIN_TOKEN_LEN => {}
            _ => {
                return Err(format!(
                    "refusing to bind {addr}: a non-loopback address requires the {TOKEN_ENV} \
                     environment variable (at least {MIN_TOKEN_LEN} characters)"
                ))
            }
        }
    }
    Ok(addr)
}

/// The configured token, if any (empty counts as unset).
pub fn env_token() -> Option<String> {
    std::env::var(TOKEN_ENV).ok().filter(|t| !t.is_empty())
}

/// Value of `--bind <addr>` (or `--bind=<addr>`) in the CLI args.
pub fn bind_arg(args: &[String]) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--bind" {
            return it.next().cloned();
        }
        if let Some(v) = a.strip_prefix("--bind=") {
            return Some(v.to_string());
        }
    }
    None
}

// ─── File permissions ────────────────────────────────────────────────────────

/// Restrict a file to its owner (0600) on platforms that support it.
/// Data is NOT encrypted at rest; this only limits other local users.
pub fn set_private(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
}

/// Create a directory tree restricted to its owner (0700) where supported.
pub fn create_private_dir(path: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700));
    }
    Ok(())
}

/// Write a file and make it private (0600 where supported).
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        create_private_dir(parent)?;
    }
    std::fs::write(path, bytes)?;
    set_private(path);
    Ok(())
}

// ─── Test support ────────────────────────────────────────────────────────────

#[cfg(test)]
pub mod test_support {
    use std::path::PathBuf;
    use std::sync::OnceLock;

    static ROOT: OnceLock<PathBuf> = OnceLock::new();

    /// Point the data root at a fresh temp directory (once per test process).
    /// Call at the start of every test that touches persistence.
    pub fn init() -> PathBuf {
        ROOT.get_or_init(|| {
            // best-effort cleanup of stale roots from earlier runs (older than 1 hour)
            if let Ok(rd) = std::fs::read_dir(std::env::temp_dir()) {
                for e in rd.flatten() {
                    let stale = e.file_name().to_string_lossy().starts_with("kore-self-test-")
                        && e.metadata().and_then(|m| m.modified()).ok()
                            .and_then(|t| t.elapsed().ok())
                            .map_or(false, |d| d.as_secs() > 3600);
                    if stale { let _ = std::fs::remove_dir_all(e.path()); }
                }
            }
            let dir = tempfile::Builder::new()
                .prefix("kore-self-test-")
                .tempdir()
                .expect("temp dir")
                .keep();
            std::env::set_var(super::HOME_ENV, &dir);
            dir
        })
        .clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_root_is_isolated_in_tests() {
        let root = test_support::init();
        assert_eq!(data_root(), root);
        assert_ne!(data_root(), real_home_dir().join(".kore-self"));
    }

    #[test]
    fn owner_validation() {
        for bad in ["", "..", ".", "a/b", "a\\b", "../x", "x/..", "C:evil", ".hidden", "a b", "a\0b", &"x".repeat(65)] {
            assert!(validate_owner(bad).is_err(), "should reject {bad:?}");
        }
        for good in ["arun", "peer", "alice-1", "bob_2", "a.b", "日本語"] {
            assert!(validate_owner(good).is_ok(), "should accept {good:?}");
        }
    }

    #[test]
    fn safe_path_confines_to_owner_dir() {
        let root = test_support::init();
        let ok = safe_path("sp-owner", "notes/a.kore").unwrap();
        assert!(ok.starts_with(root.canonicalize().unwrap().join("sp-owner")));
        // absolute path inside the owner dir is fine
        let inside = ok.to_string_lossy().to_string();
        assert_eq!(safe_path("sp-owner", &inside).unwrap(), ok);
        // traversal
        assert!(safe_path("sp-owner", "../other/x.kore").is_err());
        assert!(safe_path("sp-owner", "a/../../x").is_err());
        // absolute outside
        let outside = std::env::temp_dir().join("evil.kore");
        assert!(safe_path("sp-owner", &outside.to_string_lossy()).is_err());
        #[cfg(windows)]
        assert!(safe_path("sp-owner", "C:\\Windows\\system32\\x").is_err());
        #[cfg(unix)]
        assert!(safe_path("sp-owner", "/etc/passwd").is_err());
        // bad owner / empty
        assert!(safe_path("../x", "a").is_err());
        assert!(safe_path("sp-owner", "  ").is_err());
        assert!(safe_path("sp-owner", "a\0b").is_err());
    }

    #[test]
    fn safe_path_rejects_symlink_escape() {
        let root = test_support::init();
        let outside = tempfile::tempdir().unwrap();
        let owner_dir = root.join("sp-link");
        std::fs::create_dir_all(&owner_dir).unwrap();
        let link = owner_dir.join("escape");
        #[cfg(unix)]
        let made = std::os::unix::fs::symlink(outside.path(), &link).is_ok();
        #[cfg(windows)]
        let made = std::os::windows::fs::symlink_dir(outside.path(), &link).is_ok();
        if !made {
            eprintln!("symlink creation not permitted on this platform/user; skipping");
            return;
        }
        assert!(safe_path("sp-link", "escape/file.kore").is_err());
        assert!(safe_path("sp-link", "escape").is_err());
    }

    #[test]
    fn trunc_is_char_boundary_safe() {
        let s = "héllo 😀 wörld 日本語テキスト";
        for n in 0..s.len() + 3 {
            let t = trunc(s, n);
            assert!(t.len() <= n.max(0));
            assert!(s.starts_with(t));
        }
        assert_eq!(trunc("😀😀", 5), "😀");
        assert_eq!(trunc("日本語", 4), "日");
        assert_eq!(trunc("日本語", 2), "");
        assert_eq!(trunc("abc", 10), "abc");
        assert_eq!(date_part("2026-10-18T10:00:00Z"), "2026-10-18");
        assert_eq!(date_part("日本"), "日本");
        assert_eq!(date_part("😀😀😀😀"), "😀😀😀😀");
    }

    #[test]
    fn sql_lit_escapes_quotes() {
        assert_eq!(sql_lit("plain"), "'plain'");
        assert_eq!(sql_lit("o'brien"), "'o''brien'");
        let inj = "x' OR '1'='1'; DROP TABLE memories; --";
        let lit = sql_lit(inj);
        assert_eq!(lit, "'x'' OR ''1''=''1''; DROP TABLE memories; --'");
        assert_eq!(sql_lit("a\0b"), "'ab'");
        // every inner quote is doubled: stripping '' pairs leaves only the delimiters
        let inner = &lit[1..lit.len() - 1];
        assert!(!inner.replace("''", "").contains('\''));
    }

    #[test]
    fn bind_policy() {
        assert_eq!(resolve_bind(None, 8080, None).unwrap().to_string(), "127.0.0.1:8080");
        assert!(resolve_bind(Some("127.0.0.1"), 9, None).is_ok());
        assert!(resolve_bind(Some("::1"), 9, None).is_ok());
        assert!(resolve_bind(Some("0.0.0.0"), 9, None).is_err());
        assert!(resolve_bind(Some("0.0.0.0"), 9, Some("short")).is_err());
        assert!(resolve_bind(Some("192.168.1.5:99"), 9, None).is_err());
        assert!(resolve_bind(Some("not-an-ip"), 9, Some("0123456789abcdef0123")).is_err());
        let ok = resolve_bind(Some("0.0.0.0"), 9, Some("0123456789abcdef0123")).unwrap();
        assert_eq!(ok.port(), 9);
    }

    #[test]
    fn bearer_checks() {
        assert!(bearer_ok(Some("Bearer secret"), "secret"));
        assert!(bearer_ok(Some("bearer   secret "), "secret"));
        assert!(!bearer_ok(Some("Bearer secreT"), "secret"));
        assert!(!bearer_ok(Some("Bearer secret2"), "secret"));
        assert!(!bearer_ok(Some("Basic secret"), "secret"));
        assert!(!bearer_ok(Some("secret"), "secret"));
        assert!(!bearer_ok(None, "secret"));
        assert!(ct_eq(b"", b""));
        assert!(!ct_eq(b"a", b""));
        let req = "GET / HTTP/1.1\r\nHost: x\r\nauthorization: Bearer tok\r\n\r\nauthorization: Bearer body";
        assert_eq!(http_header(req, "Authorization"), Some("Bearer tok"));
        assert_eq!(http_header("GET / HTTP/1.1\r\n\r\n", "Authorization"), None);
    }

    #[test]
    fn bind_arg_parsing() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(bind_arg(&a(&["x", "o", "api", "--bind", "0.0.0.0"])), Some("0.0.0.0".into()));
        assert_eq!(bind_arg(&a(&["x", "--bind=10.0.0.1"])), Some("10.0.0.1".into()));
        assert_eq!(bind_arg(&a(&["x", "o", "api"])), None);
    }
}
