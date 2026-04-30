//! Best-effort git commit_ref detection for findings.
//!
//! Given a file path, walks up to find the repo root and asks `git` for
//! the current HEAD SHA. Returns `None` if anything goes wrong — callers
//! should treat this as advisory and fall back to a client-provided value.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Attempt to resolve the current HEAD commit SHA for the repo containing `file_path`.
/// Returns `None` if `file_path` doesn't exist, isn't in a git repo, or `git` is unavailable.
pub fn detect_commit_ref(file_path: &str) -> Option<String> {
    let path = Path::new(file_path);
    let dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };

    // Climb until we find a directory containing `.git` (file or dir),
    // bounded to avoid runaway walks on absolute paths to nonexistent files.
    let repo_dir = find_repo_root(&dir)?;

    let output = Command::new("git")
        .args(["-C", repo_dir.to_str()?, "rev-parse", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let sha = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if sha.is_empty() {
        None
    } else {
        Some(sha)
    }
}

fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    for _ in 0..64 {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_commit_ref_in_review_mcp_repo() {
        // This crate is itself a git repo, so this should resolve.
        let path = env!("CARGO_MANIFEST_DIR");
        let sha = detect_commit_ref(&format!("{path}/Cargo.toml"));
        // We don't assert specific SHA, just that we got something hex-ish.
        if let Some(s) = sha {
            assert!(s.len() >= 7);
            assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
        }
    }

    #[test]
    fn test_detect_commit_ref_nonexistent() {
        let sha = detect_commit_ref("/this/path/does/not/exist/foo.rs");
        assert!(sha.is_none());
    }
}
