use crate::models::ReviewerType;
use sha2::{Digest, Sha256};
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum StorageError {
    Io(std::io::Error),
    PathError(String),
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io: {e}"),
            Self::PathError(msg) => write!(f, "path: {msg}"),
        }
    }
}

impl From<std::io::Error> for StorageError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

/// Returns the base data directory for review-mcp.
pub fn data_dir() -> PathBuf {
    let base = std::env::var("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            PathBuf::from(home).join(".local/share")
        });
    base.join("review-mcp")
}

/// Returns the directory for a specific session.
pub fn session_dir(session_id: &str) -> PathBuf {
    data_dir().join("sessions").join(session_id)
}

/// Returns the directory for a specific round within a session.
pub fn round_dir(session_id: &str, round_number: i32) -> PathBuf {
    session_dir(session_id).join(format!("round_{round_number}"))
}

/// Ensures the round directory exists, creating it and parents as needed.
pub fn ensure_round_dir(session_id: &str, round_number: i32) -> Result<PathBuf, StorageError> {
    let dir = round_dir(session_id, round_number);
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

/// Returns the review file name for a given reviewer type and round.
pub fn review_file_name(reviewer: ReviewerType, round_number: i32) -> String {
    match reviewer {
        ReviewerType::Regular => format!("CODE_REVIEW_regular_r{round_number}.md"),
        ReviewerType::Harsh => format!("CODE_REVIEW_harsh_r{round_number}.md"),
        ReviewerType::Grounded => format!("GROUNDED_REVIEW_r{round_number}.md"),
    }
}

/// Atomically write review content to disk.
/// Returns (file_path, bytes_written, sha256_hash).
pub fn write_review_atomic(
    session_id: &str,
    round_number: i32,
    reviewer: ReviewerType,
    content: &str,
) -> Result<(PathBuf, usize, String), StorageError> {
    let dir = ensure_round_dir(session_id, round_number)?;
    let filename = review_file_name(reviewer, round_number);
    let target = dir.join(&filename);

    // Compute hash
    let bytes = content.as_bytes();
    let hash = Sha256::digest(bytes);
    let hash_hex = format!("sha256:{:x}", hash);

    // Write to temp file
    let temp_name = format!("{filename}.tmp.{}", uuid::Uuid::new_v4().simple());
    let temp_path = dir.join(&temp_name);

    let mut file = fs::File::create(&temp_path)?;
    file.write_all(bytes)?;
    file.sync_all()?;

    // Atomic rename
    fs::rename(&temp_path, &target)?;

    Ok((target, bytes.len(), hash_hex))
}

/// Read review file content from disk.
pub fn read_review_file(path: &Path) -> Result<String, StorageError> {
    if !path.exists() {
        return Err(StorageError::PathError(format!(
            "file not found: {}",
            path.display()
        )));
    }
    fs::read_to_string(path).map_err(StorageError::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_review_file_name_regular() {
        assert_eq!(
            review_file_name(ReviewerType::Regular, 1),
            "CODE_REVIEW_regular_r1.md"
        );
    }

    #[test]
    fn test_review_file_name_harsh() {
        assert_eq!(
            review_file_name(ReviewerType::Harsh, 3),
            "CODE_REVIEW_harsh_r3.md"
        );
    }

    #[test]
    fn test_review_file_name_grounded() {
        assert_eq!(
            review_file_name(ReviewerType::Grounded, 2),
            "GROUNDED_REVIEW_r2.md"
        );
    }

    #[test]
    fn test_write_and_read_review() {
        let tmp = tempfile::tempdir().unwrap();
        // Override data dir via env for this test
        std::env::set_var("XDG_DATA_HOME", tmp.path());

        let content = "# Review\n\nThis is a test review.";
        let (path, bytes, hash) =
            write_review_atomic("test-session-id", 1, ReviewerType::Regular, content).unwrap();

        assert!(path.exists());
        assert_eq!(bytes, content.len());
        assert!(hash.starts_with("sha256:"));

        let read_back = read_review_file(&path).unwrap();
        assert_eq!(read_back, content);

        // Verify no temp files left behind
        let dir = path.parent().unwrap();
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name().into_string().unwrap();
            assert!(
                !name.contains(".tmp."),
                "temp file left behind: {name}"
            );
        }
    }

    #[test]
    fn test_read_nonexistent_file() {
        let result = read_review_file(Path::new("/nonexistent/path.md"));
        assert!(matches!(result, Err(StorageError::PathError(_))));
    }

    #[test]
    fn test_write_review_hash_deterministic() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_DATA_HOME", tmp.path());

        let content = "deterministic content";
        let (_, _, hash1) =
            write_review_atomic("sess-a", 1, ReviewerType::Regular, content).unwrap();
        let (_, _, hash2) =
            write_review_atomic("sess-b", 1, ReviewerType::Regular, content).unwrap();

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_ensure_round_dir_creates_parents() {
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("XDG_DATA_HOME", tmp.path());

        let dir = ensure_round_dir("my-session", 5).unwrap();
        assert!(dir.exists());
        assert!(dir.ends_with("sessions/my-session/round_5"));
    }
}
