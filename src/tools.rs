use crate::db::{Db, DbError};
use crate::mcp::{tool_result_error, tool_result_text};
use crate::models::{ReviewType, ReviewerType, RoundOutcome, SessionStatus, SignalType};
use crate::storage;
use serde_json::Value;
use std::path::PathBuf;
use std::str::FromStr;

/// Return JSON schemas for all MCP tools.
pub fn list_tools() -> Value {
    serde_json::json!([
        {
            "name": "session_create",
            "description": "Create a new review session for tracking multi-round reviews. Returns session UUID and round 1.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "target_path": {
                        "type": "string",
                        "description": "Absolute path to the artifact being reviewed"
                    },
                    "review_type": {
                        "type": "string",
                        "enum": ["code", "plan", "manuscript", "architecture", "custom"],
                        "description": "Type of review"
                    }
                },
                "required": ["target_path", "review_type"]
            }
        },
        {
            "name": "session_get",
            "description": "Get session info by ID or find active session by target_path. At least one parameter required.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Session UUID"
                    },
                    "target_path": {
                        "type": "string",
                        "description": "Find active session for this target path"
                    }
                }
            }
        },
        {
            "name": "round_start",
            "description": "Begin a new review round for a session. Auto-detects the next round number. Returns round info and file paths.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Session UUID"
                    }
                },
                "required": ["session_id"]
            }
        },
        {
            "name": "review_write",
            "description": "Write review content to a specific slot (regular/harsh/grounded) for a round. Content is written atomically.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Session UUID"
                    },
                    "round": {
                        "type": "integer",
                        "description": "Round number"
                    },
                    "reviewer": {
                        "type": "string",
                        "enum": ["regular", "harsh", "grounded"],
                        "description": "Reviewer type"
                    },
                    "content": {
                        "type": "string",
                        "description": "Full review content (markdown)"
                    }
                },
                "required": ["session_id", "round", "reviewer", "content"]
            }
        },
        {
            "name": "review_read",
            "description": "Read a review. Defaults to the latest round if round is not specified.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Session UUID"
                    },
                    "round": {
                        "type": "integer",
                        "description": "Round number (defaults to latest)"
                    },
                    "reviewer": {
                        "type": "string",
                        "enum": ["regular", "harsh", "grounded"],
                        "description": "Reviewer type"
                    }
                },
                "required": ["session_id", "reviewer"]
            }
        },
        {
            "name": "round_status",
            "description": "Get completion status of a round — which reviews exist and the round outcome.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Session UUID"
                    },
                    "round": {
                        "type": "integer",
                        "description": "Round number (defaults to latest)"
                    }
                },
                "required": ["session_id"]
            }
        },
        {
            "name": "round_set_outcome",
            "description": "Mark a round as approved, rejected, or conditional.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Session UUID"
                    },
                    "round": {
                        "type": "integer",
                        "description": "Round number"
                    },
                    "outcome": {
                        "type": "string",
                        "enum": ["approved", "rejected", "conditional"],
                        "description": "Round outcome"
                    },
                    "comment": {
                        "type": "string",
                        "description": "Optional comment explaining the outcome"
                    }
                },
                "required": ["session_id", "round", "outcome"]
            }
        },
        {
            "name": "session_signal",
            "description": "Send a cross-session signal (e.g., from worker agent to orchestrator).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Session UUID"
                    },
                    "signal_type": {
                        "type": "string",
                        "enum": ["addressed", "acknowledged", "needs_revision"],
                        "description": "Signal type"
                    },
                    "source_label": {
                        "type": "string",
                        "description": "Identifier for the sender (e.g. 'orchestrator', 'worker-regular')"
                    },
                    "comment": {
                        "type": "string",
                        "description": "Optional comment"
                    }
                },
                "required": ["session_id", "signal_type", "source_label"]
            }
        },
        {
            "name": "session_signals",
            "description": "Read all signals for a session, ordered chronologically.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Session UUID"
                    }
                },
                "required": ["session_id"]
            }
        },
        {
            "name": "session_list",
            "description": "List review sessions with optional filters, ordered by most recent first.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "limit": {
                        "type": "integer",
                        "description": "Max results (default 20)"
                    },
                    "offset": {
                        "type": "integer",
                        "description": "Offset for pagination (default 0)"
                    },
                    "review_type": {
                        "type": "string",
                        "enum": ["code", "plan", "manuscript", "architecture", "custom"],
                        "description": "Filter by review type"
                    },
                    "status": {
                        "type": "string",
                        "enum": ["active", "completed", "abandoned"],
                        "description": "Filter by session status"
                    }
                }
            }
        }
    ])
}

/// Dispatch a tool call to the appropriate handler.
pub fn call_tool(db: &Db, name: &str, args: Value) -> Value {
    match name {
        "session_create" => handle_session_create(db, &args),
        "session_get" => handle_session_get(db, &args),
        "round_start" => handle_round_start(db, &args),
        "review_write" => handle_review_write(db, &args),
        "review_read" => handle_review_read(db, &args),
        "round_status" => handle_round_status(db, &args),
        "round_set_outcome" => handle_round_set_outcome(db, &args),
        "session_signal" => handle_session_signal(db, &args),
        "session_signals" => handle_session_signals(db, &args),
        "session_list" => handle_session_list(db, &args),
        _ => tool_result_error(&format!("unknown tool: {name}")),
    }
}

// --- Handlers ---

fn handle_session_create(db: &Db, args: &Value) -> Value {
    let target_path = match args.get("target_path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return tool_result_error("missing required parameter: target_path"),
    };
    let review_type = match args
        .get("review_type")
        .and_then(|v| v.as_str())
        .and_then(|s| ReviewType::from_str(s).ok())
    {
        Some(rt) => rt,
        None => return tool_result_error("missing or invalid parameter: review_type"),
    };

    let session = match db.create_session(target_path, review_type) {
        Ok(s) => s,
        Err(e) => return tool_result_error(&format!("failed to create session: {e}")),
    };

    // Auto-create round 1
    let round = match db.create_round(&session.id) {
        Ok(r) => r,
        Err(e) => return tool_result_error(&format!("failed to create round 1: {e}")),
    };

    let base_dir = storage::session_dir(&session.id);
    let round_dir = storage::round_dir(&session.id, round.round_number);

    let result = serde_json::json!({
        "session_id": session.id,
        "target_path": session.target_path,
        "review_type": session.review_type,
        "round_number": round.round_number,
        "base_dir": base_dir.display().to_string(),
        "round_dir": round_dir.display().to_string(),
        "paths": {
            "regular": round_dir.join(storage::review_file_name(ReviewerType::Regular, 1)).display().to_string(),
            "harsh": round_dir.join(storage::review_file_name(ReviewerType::Harsh, 1)).display().to_string(),
            "grounded": round_dir.join(storage::review_file_name(ReviewerType::Grounded, 1)).display().to_string(),
        }
    });
    tool_result_text(&serde_json::to_string_pretty(&result).unwrap_or_default())
}

fn handle_session_get(db: &Db, args: &Value) -> Value {
    let session_id = args.get("session_id").and_then(|v| v.as_str());
    let target_path = args.get("target_path").and_then(|v| v.as_str());

    let session = match (session_id, target_path) {
        (Some(id), _) => match db.get_session(id) {
            Ok(s) => s,
            Err(e) => return tool_result_error(&format!("{e}")),
        },
        (None, Some(path)) => match db.find_active_session_by_target(path) {
            Ok(Some(s)) => s,
            Ok(None) => return tool_result_error(&format!("no active session for target: {path}")),
            Err(e) => return tool_result_error(&format!("{e}")),
        },
        (None, None) => {
            return tool_result_error("at least one of session_id or target_path is required")
        }
    };

    // Get rounds info
    let rounds = db.get_rounds_for_session(&session.id).unwrap_or_default();
    let latest_round = rounds.last();

    let result = serde_json::json!({
        "session": session,
        "total_rounds": rounds.len(),
        "latest_round": latest_round,
    });
    tool_result_text(&serde_json::to_string_pretty(&result).unwrap_or_default())
}

fn handle_round_start(db: &Db, args: &Value) -> Value {
    let session_id = match args.get("session_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return tool_result_error("missing required parameter: session_id"),
    };

    let round = match db.create_round(session_id) {
        Ok(r) => r,
        Err(e) => return tool_result_error(&format!("failed to create round: {e}")),
    };

    let round_path = storage::round_dir(session_id, round.round_number);
    let n = round.round_number;

    let result = serde_json::json!({
        "session_id": session_id,
        "round_number": n,
        "round_dir": round_path.display().to_string(),
        "paths": {
            "regular": round_path.join(storage::review_file_name(ReviewerType::Regular, n)).display().to_string(),
            "harsh": round_path.join(storage::review_file_name(ReviewerType::Harsh, n)).display().to_string(),
            "grounded": round_path.join(storage::review_file_name(ReviewerType::Grounded, n)).display().to_string(),
        }
    });
    tool_result_text(&serde_json::to_string_pretty(&result).unwrap_or_default())
}

fn handle_review_write(db: &Db, args: &Value) -> Value {
    let session_id = match args.get("session_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return tool_result_error("missing required parameter: session_id"),
    };
    let round_number = match args.get("round").and_then(|v| v.as_i64()) {
        Some(n) => n as i32,
        None => return tool_result_error("missing required parameter: round"),
    };
    let reviewer = match args
        .get("reviewer")
        .and_then(|v| v.as_str())
        .and_then(|s| ReviewerType::from_str(s).ok())
    {
        Some(rt) => rt,
        None => return tool_result_error("missing or invalid parameter: reviewer"),
    };
    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return tool_result_error("missing required parameter: content"),
    };

    // Get the round from DB
    let round = match db.get_round(session_id, round_number) {
        Ok(r) => r,
        Err(e) => return tool_result_error(&format!("round not found: {e}")),
    };

    // Write file atomically
    let (file_path, bytes_written, content_hash) =
        match storage::write_review_atomic(session_id, round_number, reviewer, content) {
            Ok(result) => result,
            Err(e) => return tool_result_error(&format!("failed to write review file: {e}")),
        };

    // Record in database
    let file_path_str = file_path.display().to_string();
    match db.create_review(
        round.id,
        reviewer,
        &file_path_str,
        &content_hash,
        bytes_written as i64,
    ) {
        Ok(_) => {}
        Err(DbError::Conflict(_)) => {
            return tool_result_error(&format!(
                "review already exists for round {round_number} reviewer {reviewer}"
            ));
        }
        Err(e) => return tool_result_error(&format!("failed to record review: {e}")),
    }

    let result = serde_json::json!({
        "file_path": file_path_str,
        "bytes_written": bytes_written,
        "content_hash": content_hash,
    });
    tool_result_text(&serde_json::to_string_pretty(&result).unwrap_or_default())
}

fn handle_review_read(db: &Db, args: &Value) -> Value {
    let session_id = match args.get("session_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return tool_result_error("missing required parameter: session_id"),
    };
    let reviewer = match args
        .get("reviewer")
        .and_then(|v| v.as_str())
        .and_then(|s| ReviewerType::from_str(s).ok())
    {
        Some(rt) => rt,
        None => return tool_result_error("missing or invalid parameter: reviewer"),
    };

    // Determine round number
    let round = if let Some(n) = args.get("round").and_then(|v| v.as_i64()) {
        match db.get_round(session_id, n as i32) {
            Ok(r) => r,
            Err(e) => return tool_result_error(&format!("{e}")),
        }
    } else {
        match db.get_latest_round(session_id) {
            Ok(Some(r)) => r,
            Ok(None) => return tool_result_error("no rounds exist for this session"),
            Err(e) => return tool_result_error(&format!("{e}")),
        }
    };

    // Look up review record
    let review = match db.get_review(round.id, reviewer) {
        Ok(Some(r)) => r,
        Ok(None) => {
            return tool_result_error(&format!(
                "no {reviewer} review for round {}",
                round.round_number
            ))
        }
        Err(e) => return tool_result_error(&format!("{e}")),
    };

    // Read file content
    let content = match storage::read_review_file(&PathBuf::from(&review.file_path)) {
        Ok(c) => c,
        Err(e) => return tool_result_error(&format!("failed to read review file: {e}")),
    };

    let result = serde_json::json!({
        "session_id": session_id,
        "round": round.round_number,
        "reviewer": reviewer,
        "file_path": review.file_path,
        "content_hash": review.content_hash,
        "written_at": review.created_at,
        "content": content,
    });
    tool_result_text(&serde_json::to_string_pretty(&result).unwrap_or_default())
}

fn handle_round_status(db: &Db, args: &Value) -> Value {
    let session_id = match args.get("session_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return tool_result_error("missing required parameter: session_id"),
    };

    let round = if let Some(n) = args.get("round").and_then(|v| v.as_i64()) {
        match db.get_round(session_id, n as i32) {
            Ok(r) => r,
            Err(e) => return tool_result_error(&format!("{e}")),
        }
    } else {
        match db.get_latest_round(session_id) {
            Ok(Some(r)) => r,
            Ok(None) => return tool_result_error("no rounds exist for this session"),
            Err(e) => return tool_result_error(&format!("{e}")),
        }
    };

    let reviews = db.get_reviews_for_round(round.id).unwrap_or_default();
    let has_regular = reviews
        .iter()
        .any(|r| r.reviewer_type == ReviewerType::Regular);
    let has_harsh = reviews
        .iter()
        .any(|r| r.reviewer_type == ReviewerType::Harsh);
    let has_grounded = reviews
        .iter()
        .any(|r| r.reviewer_type == ReviewerType::Grounded);

    let result = serde_json::json!({
        "session_id": session_id,
        "round": round.round_number,
        "regular": has_regular,
        "harsh": has_harsh,
        "grounded": has_grounded,
        "all_reviews_present": has_regular && has_harsh && has_grounded,
        "outcome": round.outcome,
        "outcome_comment": round.outcome_comment,
    });
    tool_result_text(&serde_json::to_string_pretty(&result).unwrap_or_default())
}

fn handle_round_set_outcome(db: &Db, args: &Value) -> Value {
    let session_id = match args.get("session_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return tool_result_error("missing required parameter: session_id"),
    };
    let round_number = match args.get("round").and_then(|v| v.as_i64()) {
        Some(n) => n as i32,
        None => return tool_result_error("missing required parameter: round"),
    };
    let outcome = match args
        .get("outcome")
        .and_then(|v| v.as_str())
        .and_then(|s| RoundOutcome::from_str(s).ok())
    {
        Some(o) => o,
        None => return tool_result_error("missing or invalid parameter: outcome"),
    };
    let comment = args.get("comment").and_then(|v| v.as_str());

    let round = match db.set_round_outcome(session_id, round_number, outcome, comment) {
        Ok(r) => r,
        Err(e) => return tool_result_error(&format!("{e}")),
    };

    let result = serde_json::json!({
        "session_id": session_id,
        "round": round.round_number,
        "outcome": round.outcome,
        "outcome_comment": round.outcome_comment,
        "completed_at": round.completed_at,
    });
    tool_result_text(&serde_json::to_string_pretty(&result).unwrap_or_default())
}

fn handle_session_signal(db: &Db, args: &Value) -> Value {
    let session_id = match args.get("session_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return tool_result_error("missing required parameter: session_id"),
    };
    let signal_type = match args
        .get("signal_type")
        .and_then(|v| v.as_str())
        .and_then(|s| SignalType::from_str(s).ok())
    {
        Some(st) => st,
        None => return tool_result_error("missing or invalid parameter: signal_type"),
    };
    let source_label = match args.get("source_label").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => return tool_result_error("missing required parameter: source_label"),
    };
    let comment = args.get("comment").and_then(|v| v.as_str());

    let signal = match db.create_signal(session_id, signal_type, source_label, comment) {
        Ok(s) => s,
        Err(e) => return tool_result_error(&format!("{e}")),
    };

    let result = serde_json::json!({
        "signal_id": signal.id,
        "session_id": signal.session_id,
        "signal_type": signal.signal_type,
        "source_label": signal.source_label,
        "created_at": signal.created_at,
    });
    tool_result_text(&serde_json::to_string_pretty(&result).unwrap_or_default())
}

fn handle_session_signals(db: &Db, args: &Value) -> Value {
    let session_id = match args.get("session_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return tool_result_error("missing required parameter: session_id"),
    };

    let signals = match db.get_signals(session_id) {
        Ok(s) => s,
        Err(e) => return tool_result_error(&format!("{e}")),
    };

    let result = serde_json::json!({
        "session_id": session_id,
        "count": signals.len(),
        "signals": signals,
    });
    tool_result_text(&serde_json::to_string_pretty(&result).unwrap_or_default())
}

fn handle_session_list(db: &Db, args: &Value) -> Value {
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as u32;
    let offset = args.get("offset").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
    let review_type = args
        .get("review_type")
        .and_then(|v| v.as_str())
        .and_then(|s| ReviewType::from_str(s).ok());
    let status = args
        .get("status")
        .and_then(|v| v.as_str())
        .and_then(|s| SessionStatus::from_str(s).ok());

    let sessions = match db.list_sessions(limit, offset, review_type, status) {
        Ok(s) => s,
        Err(e) => return tool_result_error(&format!("{e}")),
    };

    // Enrich with round counts
    let enriched: Vec<Value> = sessions
        .iter()
        .map(|s| {
            let round_count = db.count_rounds(&s.id).unwrap_or(0);
            serde_json::json!({
                "session_id": s.id,
                "target_path": s.target_path,
                "review_type": s.review_type,
                "status": s.status,
                "rounds": round_count,
                "created_at": s.created_at,
                "updated_at": s.updated_at,
            })
        })
        .collect();

    let result = serde_json::json!({
        "count": enriched.len(),
        "sessions": enriched,
    });
    tool_result_text(&serde_json::to_string_pretty(&result).unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn test_db() -> Db {
        Db::open_memory().expect("failed to open test db")
    }

    #[test]
    fn test_list_tools_count() {
        let tools = list_tools();
        assert_eq!(tools.as_array().unwrap().len(), 10);
    }

    #[test]
    fn test_list_tools_has_required_names() {
        let tools = list_tools();
        let names: Vec<&str> = tools
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"session_create"));
        assert!(names.contains(&"session_get"));
        assert!(names.contains(&"round_start"));
        assert!(names.contains(&"review_write"));
        assert!(names.contains(&"review_read"));
        assert!(names.contains(&"round_status"));
        assert!(names.contains(&"round_set_outcome"));
        assert!(names.contains(&"session_signal"));
        assert!(names.contains(&"session_signals"));
        assert!(names.contains(&"session_list"));
    }

    #[test]
    fn test_call_unknown_tool() {
        let db = test_db();
        let result = call_tool(&db, "nonexistent", serde_json::json!({}));
        assert_eq!(result["isError"], true);
    }

    #[test]
    fn test_session_create_missing_params() {
        let db = test_db();
        let result = call_tool(&db, "session_create", serde_json::json!({}));
        assert_eq!(result["isError"], true);
    }

    #[test]
    fn test_session_create_success() {
        let db = test_db();
        let result = call_tool(
            &db,
            "session_create",
            serde_json::json!({"target_path": "/tmp/test.rs", "review_type": "code"}),
        );
        assert!(result.get("isError").is_none());
        let text = result["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["round_number"], 1);
        assert!(parsed["session_id"].as_str().unwrap().len() > 0);
    }

    #[test]
    fn test_session_get_by_id() {
        let db = test_db();
        let create_result = call_tool(
            &db,
            "session_create",
            serde_json::json!({"target_path": "/tmp/test.rs", "review_type": "plan"}),
        );
        let text = create_result["content"][0]["text"].as_str().unwrap();
        let created: Value = serde_json::from_str(text).unwrap();
        let session_id = created["session_id"].as_str().unwrap();

        let get_result = call_tool(
            &db,
            "session_get",
            serde_json::json!({"session_id": session_id}),
        );
        assert!(get_result.get("isError").is_none());
    }

    #[test]
    fn test_session_get_by_target() {
        let db = test_db();
        call_tool(
            &db,
            "session_create",
            serde_json::json!({"target_path": "/tmp/find_me.rs", "review_type": "code"}),
        );

        let result = call_tool(
            &db,
            "session_get",
            serde_json::json!({"target_path": "/tmp/find_me.rs"}),
        );
        assert!(result.get("isError").is_none());
    }

    #[test]
    fn test_session_get_no_params() {
        let db = test_db();
        let result = call_tool(&db, "session_get", serde_json::json!({}));
        assert_eq!(result["isError"], true);
    }

    #[test]
    fn test_round_start() {
        let db = test_db();
        let create_result = call_tool(
            &db,
            "session_create",
            serde_json::json!({"target_path": "/tmp/test.rs", "review_type": "code"}),
        );
        let text = create_result["content"][0]["text"].as_str().unwrap();
        let created: Value = serde_json::from_str(text).unwrap();
        let session_id = created["session_id"].as_str().unwrap();

        // session_create already makes round 1, so round_start should create round 2
        let result = call_tool(
            &db,
            "round_start",
            serde_json::json!({"session_id": session_id}),
        );
        let text = result["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["round_number"], 2);
    }

    #[test]
    fn test_round_status_empty() {
        let db = test_db();
        let create_result = call_tool(
            &db,
            "session_create",
            serde_json::json!({"target_path": "/tmp/test.rs", "review_type": "code"}),
        );
        let text = create_result["content"][0]["text"].as_str().unwrap();
        let created: Value = serde_json::from_str(text).unwrap();
        let session_id = created["session_id"].as_str().unwrap();

        let result = call_tool(
            &db,
            "round_status",
            serde_json::json!({"session_id": session_id}),
        );
        let text = result["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["regular"], false);
        assert_eq!(parsed["harsh"], false);
        assert_eq!(parsed["grounded"], false);
        assert_eq!(parsed["all_reviews_present"], false);
    }

    #[test]
    fn test_session_list() {
        let db = test_db();
        call_tool(
            &db,
            "session_create",
            serde_json::json!({"target_path": "/a.rs", "review_type": "code"}),
        );
        call_tool(
            &db,
            "session_create",
            serde_json::json!({"target_path": "/b.rs", "review_type": "plan"}),
        );

        let result = call_tool(&db, "session_list", serde_json::json!({}));
        let text = result["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["count"], 2);
    }

    #[test]
    fn test_session_signal_and_read() {
        let db = test_db();
        let create_result = call_tool(
            &db,
            "session_create",
            serde_json::json!({"target_path": "/tmp/test.rs", "review_type": "code"}),
        );
        let text = create_result["content"][0]["text"].as_str().unwrap();
        let created: Value = serde_json::from_str(text).unwrap();
        let session_id = created["session_id"].as_str().unwrap();

        // Send signal
        let sig_result = call_tool(
            &db,
            "session_signal",
            serde_json::json!({
                "session_id": session_id,
                "signal_type": "addressed",
                "source_label": "worker-1",
                "comment": "done with review"
            }),
        );
        assert!(sig_result.get("isError").is_none());

        // Read signals
        let read_result = call_tool(
            &db,
            "session_signals",
            serde_json::json!({"session_id": session_id}),
        );
        let text = read_result["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["count"], 1);
    }

    #[test]
    fn test_round_set_outcome() {
        let db = test_db();
        let create_result = call_tool(
            &db,
            "session_create",
            serde_json::json!({"target_path": "/tmp/test.rs", "review_type": "code"}),
        );
        let text = create_result["content"][0]["text"].as_str().unwrap();
        let created: Value = serde_json::from_str(text).unwrap();
        let session_id = created["session_id"].as_str().unwrap();

        let result = call_tool(
            &db,
            "round_set_outcome",
            serde_json::json!({
                "session_id": session_id,
                "round": 1,
                "outcome": "approved",
                "comment": "LGTM"
            }),
        );
        let text = result["content"][0]["text"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(text).unwrap();
        assert_eq!(parsed["outcome"], "approved");
        assert_eq!(parsed["outcome_comment"], "LGTM");
    }
}
