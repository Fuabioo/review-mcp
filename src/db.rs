use crate::models::{
    now_iso8601, Review, ReviewType, ReviewerType, Round, RoundOutcome, Session, SessionStatus,
    Signal, SignalType,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::fmt;
use std::path::PathBuf;

// --- Error type ---

#[derive(Debug)]
pub enum DbError {
    NotFound(String),
    Conflict(String),
    Sqlite(rusqlite::Error),
    Internal(String),
}

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(msg) => write!(f, "not found: {msg}"),
            Self::Conflict(msg) => write!(f, "conflict: {msg}"),
            Self::Sqlite(e) => write!(f, "sqlite: {e}"),
            Self::Internal(msg) => write!(f, "internal: {msg}"),
        }
    }
}

impl std::error::Error for DbError {}

impl From<rusqlite::Error> for DbError {
    fn from(e: rusqlite::Error) -> Self {
        // Detect UNIQUE constraint violations
        if let rusqlite::Error::SqliteFailure(ref err, _) = e {
            if err.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE {
                return Self::Conflict(e.to_string());
            }
        }
        Self::Sqlite(e)
    }
}

// --- Database ---

pub struct Db {
    conn: Connection,
}

impl Db {
    /// Open (or create) the database at the XDG data directory.
    pub fn open() -> Result<Self, DbError> {
        let path = Self::db_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                DbError::Internal(format!("failed to create data dir {}: {e}", parent.display()))
            })?;
        }
        let conn = Connection::open(&path)?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    /// Open an in-memory database (for testing).
    #[cfg(test)]
    pub fn open_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    fn db_path() -> Result<PathBuf, DbError> {
        let data_dir = std::env::var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                PathBuf::from(home).join(".local/share")
            });
        Ok(data_dir.join("review-mcp").join("reviews.db"))
    }

    fn init(&self) -> Result<(), DbError> {
        self.conn.execute_batch("PRAGMA journal_mode=WAL;")?;
        self.conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        self.migrate()?;
        Ok(())
    }

    fn migrate(&self) -> Result<(), DbError> {
        self.conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                target_path TEXT NOT NULL,
                review_type TEXT NOT NULL CHECK(review_type IN ('code','plan','manuscript','architecture','custom')),
                status TEXT NOT NULL DEFAULT 'active' CHECK(status IN ('active','completed','abandoned')),
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS rounds (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id),
                round_number INTEGER NOT NULL,
                outcome TEXT CHECK(outcome IN ('approved','rejected','conditional')),
                outcome_comment TEXT,
                created_at TEXT NOT NULL,
                completed_at TEXT,
                UNIQUE(session_id, round_number)
            );

            CREATE TABLE IF NOT EXISTS reviews (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                round_id INTEGER NOT NULL REFERENCES rounds(id),
                reviewer_type TEXT NOT NULL CHECK(reviewer_type IN ('regular','harsh','grounded')),
                file_path TEXT NOT NULL,
                content_hash TEXT NOT NULL,
                bytes_written INTEGER NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(round_id, reviewer_type)
            );

            CREATE TABLE IF NOT EXISTS signals (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT NOT NULL REFERENCES sessions(id),
                signal_type TEXT NOT NULL CHECK(signal_type IN ('addressed','acknowledged','needs_revision')),
                source_label TEXT NOT NULL,
                comment TEXT,
                created_at TEXT NOT NULL
            );",
        )?;
        Ok(())
    }

    // --- Sessions ---

    pub fn create_session(
        &self,
        target_path: &str,
        review_type: ReviewType,
    ) -> Result<Session, DbError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_iso8601();
        self.conn.execute(
            "INSERT INTO sessions (id, target_path, review_type, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'active', ?4, ?5)",
            params![id, target_path, review_type.to_string(), now, now],
        )?;
        self.get_session(&id)
    }

    pub fn get_session(&self, id: &str) -> Result<Session, DbError> {
        self.conn
            .query_row(
                "SELECT id, target_path, review_type, status, created_at, updated_at
                 FROM sessions WHERE id = ?1",
                params![id],
                |row| Ok(row_to_session(row)),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    DbError::NotFound(format!("session {id}"))
                }
                other => DbError::from(other),
            })
    }

    pub fn find_active_session_by_target(
        &self,
        target_path: &str,
    ) -> Result<Option<Session>, DbError> {
        self.conn
            .query_row(
                "SELECT id, target_path, review_type, status, created_at, updated_at
                 FROM sessions WHERE target_path = ?1 AND status = 'active'
                 ORDER BY created_at DESC LIMIT 1",
                params![target_path],
                |row| Ok(row_to_session(row)),
            )
            .optional()
            .map_err(DbError::from)
    }

    pub fn list_sessions(
        &self,
        limit: u32,
        offset: u32,
        review_type: Option<ReviewType>,
        status: Option<SessionStatus>,
    ) -> Result<Vec<Session>, DbError> {
        let mut sql = String::from(
            "SELECT id, target_path, review_type, status, created_at, updated_at FROM sessions",
        );
        let mut conditions: Vec<String> = Vec::new();
        if let Some(rt) = &review_type {
            conditions.push(format!("review_type = '{rt}'"));
        }
        if let Some(st) = &status {
            conditions.push(format!("status = '{st}'"));
        }
        if !conditions.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&conditions.join(" AND "));
        }
        sql.push_str(" ORDER BY created_at DESC");
        sql.push_str(&format!(" LIMIT {limit} OFFSET {offset}"));

        let mut stmt = self.conn.prepare(&sql)?;
        let sessions = stmt
            .query_map([], |row| Ok(row_to_session(row)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(sessions)
    }

    #[allow(dead_code)] // Will be used when session complete/abandon tool is added
    pub fn update_session_status(
        &self,
        id: &str,
        status: SessionStatus,
    ) -> Result<(), DbError> {
        let now = now_iso8601();
        let rows = self.conn.execute(
            "UPDATE sessions SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status.to_string(), now, id],
        )?;
        if rows == 0 {
            return Err(DbError::NotFound(format!("session {id}")));
        }
        Ok(())
    }

    // --- Rounds ---

    pub fn create_round(&self, session_id: &str) -> Result<Round, DbError> {
        // Verify session exists
        let _ = self.get_session(session_id)?;

        let next_number: i32 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(round_number), 0) + 1 FROM rounds WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )?;

        let now = now_iso8601();
        self.conn.execute(
            "INSERT INTO rounds (session_id, round_number, created_at) VALUES (?1, ?2, ?3)",
            params![session_id, next_number, now],
        )?;

        let id = self.conn.last_insert_rowid();
        // Update session updated_at
        self.conn.execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            params![now, session_id],
        )?;

        Ok(Round {
            id,
            session_id: session_id.to_string(),
            round_number: next_number,
            outcome: None,
            outcome_comment: None,
            created_at: now,
            completed_at: None,
        })
    }

    pub fn get_round(&self, session_id: &str, round_number: i32) -> Result<Round, DbError> {
        self.conn
            .query_row(
                "SELECT id, session_id, round_number, outcome, outcome_comment, created_at, completed_at
                 FROM rounds WHERE session_id = ?1 AND round_number = ?2",
                params![session_id, round_number],
                |row| Ok(row_to_round(row)),
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    DbError::NotFound(format!("round {round_number} in session {session_id}"))
                }
                other => DbError::from(other),
            })
    }

    pub fn get_latest_round(&self, session_id: &str) -> Result<Option<Round>, DbError> {
        self.conn
            .query_row(
                "SELECT id, session_id, round_number, outcome, outcome_comment, created_at, completed_at
                 FROM rounds WHERE session_id = ?1
                 ORDER BY round_number DESC LIMIT 1",
                params![session_id],
                |row| Ok(row_to_round(row)),
            )
            .optional()
            .map_err(DbError::from)
    }

    pub fn set_round_outcome(
        &self,
        session_id: &str,
        round_number: i32,
        outcome: RoundOutcome,
        comment: Option<&str>,
    ) -> Result<Round, DbError> {
        let now = now_iso8601();
        let rows = self.conn.execute(
            "UPDATE rounds SET outcome = ?1, outcome_comment = ?2, completed_at = ?3
             WHERE session_id = ?4 AND round_number = ?5",
            params![outcome.to_string(), comment, now, session_id, round_number],
        )?;
        if rows == 0 {
            return Err(DbError::NotFound(format!(
                "round {round_number} in session {session_id}"
            )));
        }
        // Update session updated_at
        self.conn.execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            params![now, session_id],
        )?;
        self.get_round(session_id, round_number)
    }

    pub fn get_rounds_for_session(&self, session_id: &str) -> Result<Vec<Round>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, round_number, outcome, outcome_comment, created_at, completed_at
             FROM rounds WHERE session_id = ?1 ORDER BY round_number ASC",
        )?;
        let rounds = stmt
            .query_map(params![session_id], |row| Ok(row_to_round(row)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rounds)
    }

    // --- Reviews ---

    pub fn create_review(
        &self,
        round_id: i64,
        reviewer_type: ReviewerType,
        file_path: &str,
        content_hash: &str,
        bytes_written: i64,
    ) -> Result<Review, DbError> {
        let now = now_iso8601();
        self.conn.execute(
            "INSERT INTO reviews (round_id, reviewer_type, file_path, content_hash, bytes_written, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![round_id, reviewer_type.to_string(), file_path, content_hash, bytes_written, now],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(Review {
            id,
            round_id,
            reviewer_type,
            file_path: file_path.to_string(),
            content_hash: content_hash.to_string(),
            bytes_written,
            created_at: now,
        })
    }

    pub fn get_review(
        &self,
        round_id: i64,
        reviewer_type: ReviewerType,
    ) -> Result<Option<Review>, DbError> {
        self.conn
            .query_row(
                "SELECT id, round_id, reviewer_type, file_path, content_hash, bytes_written, created_at
                 FROM reviews WHERE round_id = ?1 AND reviewer_type = ?2",
                params![round_id, reviewer_type.to_string()],
                |row| Ok(row_to_review(row)),
            )
            .optional()
            .map_err(DbError::from)
    }

    pub fn get_reviews_for_round(&self, round_id: i64) -> Result<Vec<Review>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, round_id, reviewer_type, file_path, content_hash, bytes_written, created_at
             FROM reviews WHERE round_id = ?1 ORDER BY created_at ASC",
        )?;
        let reviews = stmt
            .query_map(params![round_id], |row| Ok(row_to_review(row)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(reviews)
    }

    // --- Signals ---

    pub fn create_signal(
        &self,
        session_id: &str,
        signal_type: SignalType,
        source_label: &str,
        comment: Option<&str>,
    ) -> Result<Signal, DbError> {
        // Verify session exists
        let _ = self.get_session(session_id)?;

        let now = now_iso8601();
        self.conn.execute(
            "INSERT INTO signals (session_id, signal_type, source_label, comment, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_id, signal_type.to_string(), source_label, comment, now],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(Signal {
            id,
            session_id: session_id.to_string(),
            signal_type,
            source_label: source_label.to_string(),
            comment: comment.map(String::from),
            created_at: now,
        })
    }

    pub fn get_signals(&self, session_id: &str) -> Result<Vec<Signal>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, signal_type, source_label, comment, created_at
             FROM signals WHERE session_id = ?1 ORDER BY created_at ASC",
        )?;
        let signals = stmt
            .query_map(params![session_id], |row| Ok(row_to_signal(row)))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(signals)
    }

    // --- Prune ---

    /// Delete sessions (and their rounds, reviews, signals) older than `before` (ISO 8601).
    /// Returns the list of pruned session IDs so the caller can clean up files.
    pub fn prune_sessions_before(&self, before: &str) -> Result<Vec<String>, DbError> {
        // Collect session IDs to prune
        let mut stmt = self.conn.prepare(
            "SELECT id FROM sessions WHERE created_at < ?1",
        )?;
        let ids: Vec<String> = stmt
            .query_map(params![before], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;

        if ids.is_empty() {
            return Ok(ids);
        }

        // Delete in dependency order: reviews → rounds → signals → sessions
        // We use subqueries so it's all one transaction worth of work
        self.conn.execute(
            "DELETE FROM reviews WHERE round_id IN (
                SELECT r.id FROM rounds r
                JOIN sessions s ON r.session_id = s.id
                WHERE s.created_at < ?1
            )",
            params![before],
        )?;
        self.conn.execute(
            "DELETE FROM rounds WHERE session_id IN (
                SELECT id FROM sessions WHERE created_at < ?1
            )",
            params![before],
        )?;
        self.conn.execute(
            "DELETE FROM signals WHERE session_id IN (
                SELECT id FROM sessions WHERE created_at < ?1
            )",
            params![before],
        )?;
        self.conn.execute(
            "DELETE FROM sessions WHERE created_at < ?1",
            params![before],
        )?;

        Ok(ids)
    }

    /// Get the number of rounds for a session (used by audit).
    pub fn count_rounds(&self, session_id: &str) -> Result<i32, DbError> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM rounds WHERE session_id = ?1",
                params![session_id],
                |row| row.get(0),
            )
            .map_err(DbError::from)
    }
}

// --- Row mapping helpers ---

fn row_to_session(row: &rusqlite::Row<'_>) -> Session {
    Session {
        id: row.get_unwrap(0),
        target_path: row.get_unwrap(1),
        review_type: row.get_unwrap::<_, String>(2).parse().unwrap_or(ReviewType::Custom),
        status: row.get_unwrap::<_, String>(3).parse().unwrap_or(SessionStatus::Active),
        created_at: row.get_unwrap(4),
        updated_at: row.get_unwrap(5),
    }
}

fn row_to_round(row: &rusqlite::Row<'_>) -> Round {
    Round {
        id: row.get_unwrap(0),
        session_id: row.get_unwrap(1),
        round_number: row.get_unwrap(2),
        outcome: row
            .get_unwrap::<_, Option<String>>(3)
            .and_then(|s| s.parse().ok()),
        outcome_comment: row.get_unwrap(4),
        created_at: row.get_unwrap(5),
        completed_at: row.get_unwrap(6),
    }
}

fn row_to_review(row: &rusqlite::Row<'_>) -> Review {
    Review {
        id: row.get_unwrap(0),
        round_id: row.get_unwrap(1),
        reviewer_type: row.get_unwrap::<_, String>(2).parse().unwrap_or(ReviewerType::Regular),
        file_path: row.get_unwrap(3),
        content_hash: row.get_unwrap(4),
        bytes_written: row.get_unwrap(5),
        created_at: row.get_unwrap(6),
    }
}

fn row_to_signal(row: &rusqlite::Row<'_>) -> Signal {
    Signal {
        id: row.get_unwrap(0),
        session_id: row.get_unwrap(1),
        signal_type: row.get_unwrap::<_, String>(2).parse().unwrap_or(SignalType::Addressed),
        source_label: row.get_unwrap(3),
        comment: row.get_unwrap(4),
        created_at: row.get_unwrap(5),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Db {
        Db::open_memory().expect("failed to open test db")
    }

    #[test]
    fn test_create_and_get_session() {
        let db = test_db();
        let session = db.create_session("/tmp/test.rs", ReviewType::Code).unwrap();
        assert_eq!(session.target_path, "/tmp/test.rs");
        assert_eq!(session.review_type, ReviewType::Code);
        assert_eq!(session.status, SessionStatus::Active);

        let fetched = db.get_session(&session.id).unwrap();
        assert_eq!(fetched.id, session.id);
    }

    #[test]
    fn test_get_session_not_found() {
        let db = test_db();
        let result = db.get_session("nonexistent");
        assert!(matches!(result, Err(DbError::NotFound(_))));
    }

    #[test]
    fn test_find_active_session_by_target() {
        let db = test_db();
        let session = db.create_session("/tmp/test.rs", ReviewType::Code).unwrap();

        let found = db.find_active_session_by_target("/tmp/test.rs").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, session.id);

        let not_found = db.find_active_session_by_target("/tmp/other.rs").unwrap();
        assert!(not_found.is_none());
    }

    #[test]
    fn test_list_sessions() {
        let db = test_db();
        db.create_session("/a.rs", ReviewType::Code).unwrap();
        db.create_session("/b.rs", ReviewType::Plan).unwrap();
        db.create_session("/c.rs", ReviewType::Code).unwrap();

        let all = db.list_sessions(10, 0, None, None).unwrap();
        assert_eq!(all.len(), 3);

        let code_only = db.list_sessions(10, 0, Some(ReviewType::Code), None).unwrap();
        assert_eq!(code_only.len(), 2);

        let with_limit = db.list_sessions(1, 0, None, None).unwrap();
        assert_eq!(with_limit.len(), 1);

        let with_offset = db.list_sessions(10, 2, None, None).unwrap();
        assert_eq!(with_offset.len(), 1);
    }

    #[test]
    fn test_update_session_status() {
        let db = test_db();
        let session = db.create_session("/tmp/test.rs", ReviewType::Code).unwrap();
        db.update_session_status(&session.id, SessionStatus::Completed).unwrap();

        let updated = db.get_session(&session.id).unwrap();
        assert_eq!(updated.status, SessionStatus::Completed);
    }

    #[test]
    fn test_create_round_auto_increment() {
        let db = test_db();
        let session = db.create_session("/tmp/test.rs", ReviewType::Code).unwrap();

        let r1 = db.create_round(&session.id).unwrap();
        assert_eq!(r1.round_number, 1);

        let r2 = db.create_round(&session.id).unwrap();
        assert_eq!(r2.round_number, 2);

        let r3 = db.create_round(&session.id).unwrap();
        assert_eq!(r3.round_number, 3);
    }

    #[test]
    fn test_get_round() {
        let db = test_db();
        let session = db.create_session("/tmp/test.rs", ReviewType::Code).unwrap();
        db.create_round(&session.id).unwrap();

        let round = db.get_round(&session.id, 1).unwrap();
        assert_eq!(round.round_number, 1);
        assert!(round.outcome.is_none());
    }

    #[test]
    fn test_get_latest_round() {
        let db = test_db();
        let session = db.create_session("/tmp/test.rs", ReviewType::Code).unwrap();

        let none = db.get_latest_round(&session.id).unwrap();
        assert!(none.is_none());

        db.create_round(&session.id).unwrap();
        db.create_round(&session.id).unwrap();

        let latest = db.get_latest_round(&session.id).unwrap().unwrap();
        assert_eq!(latest.round_number, 2);
    }

    #[test]
    fn test_set_round_outcome() {
        let db = test_db();
        let session = db.create_session("/tmp/test.rs", ReviewType::Code).unwrap();
        db.create_round(&session.id).unwrap();

        let round = db
            .set_round_outcome(&session.id, 1, RoundOutcome::Approved, Some("looks good"))
            .unwrap();
        assert_eq!(round.outcome, Some(RoundOutcome::Approved));
        assert_eq!(round.outcome_comment.as_deref(), Some("looks good"));
        assert!(round.completed_at.is_some());
    }

    #[test]
    fn test_create_review() {
        let db = test_db();
        let session = db.create_session("/tmp/test.rs", ReviewType::Code).unwrap();
        let round = db.create_round(&session.id).unwrap();

        let review = db
            .create_review(round.id, ReviewerType::Regular, "/path/review.md", "sha256:abc", 1234)
            .unwrap();
        assert_eq!(review.reviewer_type, ReviewerType::Regular);
        assert_eq!(review.bytes_written, 1234);
    }

    #[test]
    fn test_duplicate_review_conflict() {
        let db = test_db();
        let session = db.create_session("/tmp/test.rs", ReviewType::Code).unwrap();
        let round = db.create_round(&session.id).unwrap();

        db.create_review(round.id, ReviewerType::Regular, "/a.md", "sha256:a", 100)
            .unwrap();
        let result = db.create_review(round.id, ReviewerType::Regular, "/b.md", "sha256:b", 200);
        assert!(matches!(result, Err(DbError::Conflict(_))));
    }

    #[test]
    fn test_get_review() {
        let db = test_db();
        let session = db.create_session("/tmp/test.rs", ReviewType::Code).unwrap();
        let round = db.create_round(&session.id).unwrap();

        let none = db.get_review(round.id, ReviewerType::Regular).unwrap();
        assert!(none.is_none());

        db.create_review(round.id, ReviewerType::Regular, "/a.md", "sha256:a", 100)
            .unwrap();

        let some = db.get_review(round.id, ReviewerType::Regular).unwrap();
        assert!(some.is_some());
        assert_eq!(some.unwrap().file_path, "/a.md");
    }

    #[test]
    fn test_get_reviews_for_round() {
        let db = test_db();
        let session = db.create_session("/tmp/test.rs", ReviewType::Code).unwrap();
        let round = db.create_round(&session.id).unwrap();

        db.create_review(round.id, ReviewerType::Regular, "/a.md", "sha256:a", 100)
            .unwrap();
        db.create_review(round.id, ReviewerType::Harsh, "/b.md", "sha256:b", 200)
            .unwrap();

        let reviews = db.get_reviews_for_round(round.id).unwrap();
        assert_eq!(reviews.len(), 2);
    }

    #[test]
    fn test_create_and_get_signals() {
        let db = test_db();
        let session = db.create_session("/tmp/test.rs", ReviewType::Code).unwrap();

        db.create_signal(&session.id, SignalType::Addressed, "worker-1", Some("done"))
            .unwrap();
        db.create_signal(&session.id, SignalType::Acknowledged, "orchestrator", None)
            .unwrap();

        let signals = db.get_signals(&session.id).unwrap();
        assert_eq!(signals.len(), 2);
        assert_eq!(signals[0].signal_type, SignalType::Addressed);
        assert_eq!(signals[0].source_label, "worker-1");
        assert_eq!(signals[0].comment.as_deref(), Some("done"));
        assert_eq!(signals[1].signal_type, SignalType::Acknowledged);
    }

    #[test]
    fn test_signal_nonexistent_session() {
        let db = test_db();
        let result = db.create_signal("nonexistent", SignalType::Addressed, "worker", None);
        assert!(matches!(result, Err(DbError::NotFound(_))));
    }

    #[test]
    fn test_get_rounds_for_session() {
        let db = test_db();
        let session = db.create_session("/tmp/test.rs", ReviewType::Code).unwrap();
        db.create_round(&session.id).unwrap();
        db.create_round(&session.id).unwrap();

        let rounds = db.get_rounds_for_session(&session.id).unwrap();
        assert_eq!(rounds.len(), 2);
        assert_eq!(rounds[0].round_number, 1);
        assert_eq!(rounds[1].round_number, 2);
    }

    #[test]
    fn test_count_rounds() {
        let db = test_db();
        let session = db.create_session("/tmp/test.rs", ReviewType::Code).unwrap();
        assert_eq!(db.count_rounds(&session.id).unwrap(), 0);

        db.create_round(&session.id).unwrap();
        db.create_round(&session.id).unwrap();
        assert_eq!(db.count_rounds(&session.id).unwrap(), 2);
    }
}
