//! Per-user Claude working-environment path confinement (design
//! 2026-06-09). Fail closed: any missing/invalid/unprovable input rejects.
//! Pure helpers (no I/O) live here with the one filesystem resolver
//! (resolve_user_cwd) so the security logic is unit-testable.

use std::path::{Path, PathBuf};

/// Why a per-user cwd could not be resolved. Every variant means "reject,
/// do not spawn" — there is no fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    BadUser(String),
    BadPath(String),
    Escape(String),
    Io(String),
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ResolveError::BadUser(m) => write!(f, "bad user id: {m}"),
            ResolveError::BadPath(m) => write!(f, "bad path: {m}"),
            ResolveError::Escape(m) => write!(f, "path escapes user dir: {m}"),
            ResolveError::Io(m) => write!(f, "io: {m}"),
        }
    }
}

/// PURE: require the env base. REQUIRED — no default. Trims; Err naming the
/// var when None/empty/whitespace. Mirrors worker_bind_addr's contract.
pub fn require_user_env_base(raw: Option<String>) -> Result<PathBuf, String> {
    match raw.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        Some(v) => Ok(PathBuf::from(v)),
        None => Err(
            "LLM_CHAT_USER_ENV_BASE must be set (no default) — the per-user \
             Claude environment root".to_string(),
        ),
    }
}

fn valid_user_id(user_id: &str) -> bool {
    !user_id.is_empty()
        && user_id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// PURE (no I/O): validate `user_id` and the client `subpath`, and return the
/// LEXICAL candidate `base/user_id/<components…>`. Rejects an empty/illegal
/// user id, and any subpath that is absolute or contains `..`, `.`, an empty
/// component, `\`, `:`, or NUL. None/empty subpath → the user root.
pub fn confine_path(
    base: &Path,
    user_id: &str,
    subpath: Option<&str>,
) -> Result<PathBuf, ResolveError> {
    if !valid_user_id(user_id) {
        return Err(ResolveError::BadUser(format!("{user_id:?}")));
    }
    let mut out = base.join(user_id);
    let raw = subpath.unwrap_or("").trim();
    if raw.is_empty() {
        return Ok(out);
    }
    if raw.starts_with('/') {
        return Err(ResolveError::BadPath("absolute path not allowed".into()));
    }
    for comp in raw.split('/') {
        if comp.is_empty() || comp == "." || comp == ".." {
            return Err(ResolveError::BadPath(format!("illegal component {comp:?}")));
        }
        if comp.contains('\\') || comp.contains(':') || comp.contains('\0') {
            return Err(ResolveError::BadPath(format!("illegal char in {comp:?}")));
        }
        out.push(comp);
    }
    Ok(out)
}

/// Create + confine the per-user cwd, returning the LEXICAL confined path
/// (not the verbatim canonical form, so claude gets a clean cwd). The
/// canonical form is used only to PROVE confinement (defends against
/// symlinks/races the lexical check can't see). Fail closed on any error.
pub fn resolve_user_cwd(
    base: &Path,
    user_id: &str,
    subpath: Option<&str>,
) -> Result<PathBuf, ResolveError> {
    let candidate = confine_path(base, user_id, subpath)?;
    std::fs::create_dir_all(&candidate)
        .map_err(|e| ResolveError::Io(format!("create {}: {e}", candidate.display())))?;
    let real = candidate
        .canonicalize()
        .map_err(|e| ResolveError::Escape(format!("canonicalize candidate: {e}")))?;
    let root = base
        .join(user_id)
        .canonicalize()
        .map_err(|e| ResolveError::Escape(format!("canonicalize root: {e}")))?;
    if !real.starts_with(&root) {
        return Err(ResolveError::Escape(format!(
            "{} not under {}", real.display(), root.display()
        )));
    }
    Ok(candidate)
}

/// The open-command gate: a user id is MANDATORY (no fallback). None/empty →
/// reject. Otherwise resolve + confine.
pub fn open_cwd(
    base: &Path,
    user_id: Option<&str>,
    subpath: Option<&str>,
) -> Result<PathBuf, ResolveError> {
    let uid = user_id.unwrap_or("").trim();
    if uid.is_empty() {
        return Err(ResolveError::BadUser(
            "per-user environment requires an authenticated user id".into(),
        ));
    }
    resolve_user_cwd(base, uid, subpath)
}

/// One entry in a box listing. `path` is RELATIVE to the box root,
/// '/'-separated. Symlinks are reported (`dir=false`) but never descended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub path: String,
    pub dir: bool,
    pub size: u64,
}

/// Recursively list the user's box (`{base}/{user_id}`), CONFINED. Bounded by
/// `max_depth` (directory nesting) and `max_entries` (total). Does NOT follow
/// symlinks — a symlink is listed (`dir=false`) but never descended, so the
/// walk can never escape the box even if a symlink points outside it. Entries
/// are sorted by path. Fails closed via `resolve_user_cwd` (mandatory user id;
/// traversal/escape rejected; box auto-created).
pub fn list_box_tree(
    base: &Path,
    user_id: Option<&str>,
    max_depth: usize,
    max_entries: usize,
) -> Result<(Vec<DirEntry>, bool), ResolveError> {
    let uid = user_id.unwrap_or("").trim();
    if uid.is_empty() {
        return Err(ResolveError::BadUser(
            "per-user environment requires an authenticated user id".into(),
        ));
    }
    let root = resolve_user_cwd(base, uid, None)?; // confined + created box root
    let mut out = Vec::new();
    let mut truncated = false;
    walk_box(&root, &root, 0, max_depth, max_entries, &mut out, &mut truncated);
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((out, truncated))
}

/// Read-only, NON-CREATING variant of `list_box_tree` for the admin Console
/// view: if the user's box does not exist yet, return an empty listing WITHOUT
/// creating it (viewing must never mutate the filesystem). Same confinement and
/// symlink-safety as `list_box_tree` (a symlink is listed but never descended,
/// so the walk can't escape the box). Fails closed on an invalid user id.
pub fn list_box_readonly(
    base: &Path,
    user_id: Option<&str>,
    max_depth: usize,
    max_entries: usize,
) -> Result<(Vec<DirEntry>, bool), ResolveError> {
    let uid = user_id.unwrap_or("").trim();
    if !valid_user_id(uid) {
        return Err(ResolveError::BadUser(format!("{uid:?}")));
    }
    let root_lexical = base.join(uid);
    // Non-creating: no box on disk → no sandbox yet (do NOT create one).
    if !root_lexical.exists() {
        return Ok((Vec::new(), false));
    }
    // Canonicalize the root to prove it resolves before walking (mirrors
    // resolve_user_cwd minus the create); the walk never follows symlinks.
    let root = root_lexical
        .canonicalize()
        .map_err(|e| ResolveError::Escape(format!("canonicalize root: {e}")))?;
    let mut out = Vec::new();
    let mut truncated = false;
    walk_box(&root, &root, 0, max_depth, max_entries, &mut out, &mut truncated);
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((out, truncated))
}

fn walk_box(
    root: &Path,
    dir: &Path,
    depth: usize,
    max_depth: usize,
    max_entries: usize,
    out: &mut Vec<DirEntry>,
    truncated: &mut bool,
) {
    let rd = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return,
    };
    let mut paths: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    paths.sort();
    for path in paths {
        if out.len() >= max_entries {
            *truncated = true;
            return;
        }
        // symlink_metadata: never follow a symlink (it could point outside the
        // box). It is listed as a non-dir entry and never descended.
        let md = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let is_dir = md.is_dir() && !md.file_type().is_symlink();
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        out.push(DirEntry { path: rel, dir: is_dir, size: if is_dir { 0 } else { md.len() } });
        if is_dir {
            if depth + 1 < max_depth {
                walk_box(root, &path, depth + 1, max_depth, max_entries, out, truncated);
            } else if std::fs::read_dir(&path).map(|mut r| r.next().is_some()).unwrap_or(false) {
                *truncated = true; // deeper entries exist but the depth cap hides them
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use std::fs;

    #[test]
    fn require_base_rejects_missing() {
        assert!(require_user_env_base(None).unwrap_err().contains("LLM_CHAT_USER_ENV_BASE"));
        assert!(require_user_env_base(Some("   ".into())).unwrap_err().contains("LLM_CHAT_USER_ENV_BASE"));
    }
    #[test]
    fn require_base_trims_and_accepts() {
        assert_eq!(require_user_env_base(Some("  /srv/envs  ".into())).unwrap(), PathBuf::from("/srv/envs"));
    }

    fn base() -> PathBuf { PathBuf::from("/srv/envs") }

    #[test]
    fn confine_none_subpath_is_user_root() {
        assert_eq!(confine_path(&base(), "u1", None).unwrap(), PathBuf::from("/srv/envs/u1"));
        assert_eq!(confine_path(&base(), "u1", Some("")).unwrap(), PathBuf::from("/srv/envs/u1"));
    }
    #[test]
    fn confine_nested_service_subpath() {
        assert_eq!(
            confine_path(&base(), "311867081814147073", Some("crm/acct-42")).unwrap(),
            PathBuf::from("/srv/envs/311867081814147073/crm/acct-42"),
        );
    }
    #[test]
    fn confine_rejects_bad_user() {
        assert!(matches!(confine_path(&base(), "", None), Err(ResolveError::BadUser(_))));
        assert!(matches!(confine_path(&base(), "..", None), Err(ResolveError::BadUser(_))));
        assert!(matches!(confine_path(&base(), "a/b", None), Err(ResolveError::BadUser(_))));
        assert!(matches!(confine_path(&base(), "a b", None), Err(ResolveError::BadUser(_))));
    }
    #[test]
    fn confine_rejects_traversal_and_absolute() {
        assert!(matches!(confine_path(&base(), "u1", Some("../x")), Err(ResolveError::BadPath(_))));
        assert!(matches!(confine_path(&base(), "u1", Some("a/../../b")), Err(ResolveError::BadPath(_))));
        assert!(matches!(confine_path(&base(), "u1", Some("/etc")), Err(ResolveError::BadPath(_))));
        assert!(matches!(confine_path(&base(), "u1", Some("a/./b")), Err(ResolveError::BadPath(_))));
    }
    #[test]
    fn confine_rejects_windows_and_nul() {
        assert!(matches!(confine_path(&base(), "u1", Some("a\\b")), Err(ResolveError::BadPath(_))));
        assert!(matches!(confine_path(&base(), "u1", Some("C:")), Err(ResolveError::BadPath(_))));
        assert!(matches!(confine_path(&base(), "u1", Some("a\0b")), Err(ResolveError::BadPath(_))));
    }

    #[test]
    fn resolve_creates_and_returns_confined_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let p = resolve_user_cwd(tmp.path(), "u1", Some("svc/a")).unwrap();
        assert!(p.is_dir(), "dir auto-created");
        assert!(p.ends_with("u1/svc/a") || p.ends_with("u1\\svc\\a"));
        assert!(p.starts_with(tmp.path().join("u1")));
    }

    #[test]
    fn resolve_rejects_traversal() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(resolve_user_cwd(tmp.path(), "u1", Some("../escape")), Err(ResolveError::BadPath(_))));
    }

    #[cfg(unix)]
    #[test]
    fn resolve_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        let user_root = tmp.path().join("u1");
        fs::create_dir_all(&user_root).unwrap();
        symlink(&outside, user_root.join("link")).unwrap();
        let err = resolve_user_cwd(tmp.path(), "u1", Some("link")).unwrap_err();
        assert!(matches!(err, ResolveError::Escape(_)), "got {err:?}");
    }

    #[test]
    fn open_cwd_rejects_missing_user() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(open_cwd(tmp.path(), None, Some("svc")), Err(ResolveError::BadUser(_))));
        assert!(matches!(open_cwd(tmp.path(), Some(""), Some("svc")), Err(ResolveError::BadUser(_))));
    }

    #[test]
    fn open_cwd_ok_for_valid_user() {
        let tmp = tempfile::tempdir().unwrap();
        let p = open_cwd(tmp.path(), Some("u1"), None).unwrap();
        assert!(p.is_dir());
    }

    #[test]
    fn list_box_tree_lists_confined_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("u1");
        std::fs::create_dir_all(root.join("projects/sub")).unwrap();
        std::fs::write(root.join("todo.md"), b"hello").unwrap();
        std::fs::write(root.join("projects/main.rs"), b"fn main(){}").unwrap();
        let (entries, truncated) = list_box_tree(tmp.path(), Some("u1"), 8, 1000).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"todo.md"));
        assert!(paths.contains(&"projects"));
        assert!(paths.contains(&"projects/main.rs"));
        assert!(paths.contains(&"projects/sub"));
        assert!(!truncated);
        let todo = entries.iter().find(|e| e.path == "todo.md").unwrap();
        assert_eq!(todo.size, 5);
        assert!(!todo.dir);
        assert!(entries.iter().find(|e| e.path == "projects").unwrap().dir);
    }

    #[test]
    fn list_box_tree_rejects_missing_user() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(list_box_tree(tmp.path(), None, 8, 1000), Err(ResolveError::BadUser(_))));
        assert!(matches!(list_box_tree(tmp.path(), Some(""), 8, 1000), Err(ResolveError::BadUser(_))));
    }

    #[test]
    fn list_box_tree_depth_cap_truncates() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("u1");
        std::fs::create_dir_all(root.join("a/b/c")).unwrap();
        std::fs::write(root.join("a/b/c/deep.txt"), b"x").unwrap();
        // depth 1: top-level "a" listed, but its contents are not descended.
        let (entries, truncated) = list_box_tree(tmp.path(), Some("u1"), 1, 1000).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"a"));
        assert!(!paths.contains(&"a/b"));
        assert!(truncated);
    }

    #[cfg(unix)]
    #[test]
    fn list_box_tree_does_not_follow_symlink_escape() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), b"top secret").unwrap();
        let root = tmp.path().join("u1");
        std::fs::create_dir_all(&root).unwrap();
        symlink(&outside, root.join("link")).unwrap();
        let (entries, _) = list_box_tree(tmp.path(), Some("u1"), 8, 1000).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"link")); // the symlink itself is listed
        assert!(!paths.iter().any(|p| p.contains("secret"))); // but NOT followed
        assert!(!entries.iter().find(|e| e.path == "link").unwrap().dir);
    }

    #[test]
    fn list_box_readonly_lists_existing_box() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("u1");
        std::fs::create_dir_all(root.join("projects/sub")).unwrap();
        std::fs::write(root.join("todo.md"), b"hello").unwrap();
        let (entries, truncated) = list_box_readonly(tmp.path(), Some("u1"), 8, 1000).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"todo.md"));
        assert!(paths.contains(&"projects"));
        assert!(paths.contains(&"projects/sub"));
        assert!(!truncated);
        assert_eq!(entries.iter().find(|e| e.path == "todo.md").unwrap().size, 5);
    }

    #[test]
    fn list_box_readonly_absent_box_is_empty_and_not_created() {
        let tmp = tempfile::tempdir().unwrap();
        let (entries, truncated) = list_box_readonly(tmp.path(), Some("ghost"), 8, 1000).unwrap();
        assert!(entries.is_empty());
        assert!(!truncated);
        assert!(!tmp.path().join("ghost").exists(), "viewing must NOT create the box");
    }

    #[test]
    fn list_box_readonly_rejects_bad_user() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(list_box_readonly(tmp.path(), None, 8, 1000), Err(ResolveError::BadUser(_))));
        assert!(matches!(list_box_readonly(tmp.path(), Some(""), 8, 1000), Err(ResolveError::BadUser(_))));
        assert!(matches!(list_box_readonly(tmp.path(), Some("a/b"), 8, 1000), Err(ResolveError::BadUser(_))));
    }

    #[cfg(unix)]
    #[test]
    fn list_box_readonly_does_not_follow_symlink_escape() {
        use std::os::unix::fs::symlink;
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("outside");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), b"top secret").unwrap();
        let root = tmp.path().join("u1");
        std::fs::create_dir_all(&root).unwrap();
        symlink(&outside, root.join("link")).unwrap();
        let (entries, _) = list_box_readonly(tmp.path(), Some("u1"), 8, 1000).unwrap();
        let paths: Vec<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        assert!(paths.contains(&"link"));
        assert!(!paths.iter().any(|p| p.contains("secret")));
    }
}
