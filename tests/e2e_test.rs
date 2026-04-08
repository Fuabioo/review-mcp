use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// Send JSON-RPC messages to a server instance with a specific data dir.
fn send_jsonrpc_with_dir(
    data_dir: &Path,
    messages: &[serde_json::Value],
) -> Vec<serde_json::Value> {
    let binary = env!("CARGO_BIN_EXE_review-mcp");

    let mut child = Command::new(binary)
        .arg("serve")
        .env("XDG_DATA_HOME", data_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start review-mcp");

    let stdin = child.stdin.as_mut().unwrap();
    for msg in messages {
        let line = serde_json::to_string(msg).unwrap();
        writeln!(stdin, "{line}").unwrap();
    }
    drop(child.stdin.take());

    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("invalid JSON response"))
        .collect()
}

/// Send JSON-RPC messages using an ephemeral temp dir.
fn send_jsonrpc(messages: &[serde_json::Value]) -> Vec<serde_json::Value> {
    let tmp = tempfile::tempdir().unwrap();
    send_jsonrpc_with_dir(tmp.path(), messages)
}

fn send_raw(messages: &[&str]) -> Vec<serde_json::Value> {
    let binary = env!("CARGO_BIN_EXE_review-mcp");
    let tmp = tempfile::tempdir().unwrap();

    let mut child = Command::new(binary)
        .arg("serve")
        .env("XDG_DATA_HOME", tmp.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start review-mcp");

    let stdin = child.stdin.as_mut().unwrap();
    for msg in messages {
        writeln!(stdin, "{msg}").unwrap();
    }
    drop(child.stdin.take());

    let output = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    stdout
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("invalid JSON response"))
        .collect()
}

fn init_msg() -> serde_json::Value {
    serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}})
}

fn tool_call(id: i32, name: &str, args: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": { "name": name, "arguments": args }
    })
}

fn extract_tool_text(response: &serde_json::Value) -> serde_json::Value {
    let text = response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("{}");
    serde_json::from_str(text).unwrap_or_default()
}

fn is_tool_error(response: &serde_json::Value) -> bool {
    response["result"]["isError"] == true
}

#[test]
fn test_e2e_initialize() {
    let responses = send_jsonrpc(&[init_msg()]);
    assert_eq!(responses.len(), 1);
    assert_eq!(responses[0]["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(responses[0]["result"]["serverInfo"]["name"], "review-mcp");
}

#[test]
fn test_e2e_tools_list() {
    let responses = send_jsonrpc(&[
        init_msg(),
        serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}),
    ]);
    assert_eq!(responses.len(), 2);
    let tools = responses[1]["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 10);
}

#[test]
fn test_e2e_full_workflow() {
    let tmp = tempfile::tempdir().unwrap();

    // All in one server instance — single shared DB
    let responses = send_jsonrpc_with_dir(
        tmp.path(),
        &[
            init_msg(),
            // Create session
            tool_call(2, "session_create", serde_json::json!({
                "target_path": "/tmp/e2e-workflow.rs",
                "review_type": "plan"
            })),
        ],
    );
    let session_data = extract_tool_text(&responses[1]);
    let sid = session_data["session_id"].as_str().unwrap().to_string();
    assert_eq!(session_data["round_number"], 1);

    // Write reviews + workflow in same DB
    let responses = send_jsonrpc_with_dir(
        tmp.path(),
        &[
            init_msg(),
            // Write regular review
            tool_call(3, "review_write", serde_json::json!({
                "session_id": sid,
                "round": 1,
                "reviewer": "regular",
                "content": "# Regular Review\n\nLGTM"
            })),
            // Write harsh review
            tool_call(4, "review_write", serde_json::json!({
                "session_id": sid,
                "round": 1,
                "reviewer": "harsh",
                "content": "# Harsh Review\n\nNeeds work"
            })),
            // Write grounded review
            tool_call(5, "review_write", serde_json::json!({
                "session_id": sid,
                "round": 1,
                "reviewer": "grounded",
                "content": "# Grounded\n\nVerified"
            })),
            // Round status
            tool_call(6, "round_status", serde_json::json!({
                "session_id": sid
            })),
            // Set outcome
            tool_call(7, "round_set_outcome", serde_json::json!({
                "session_id": sid,
                "round": 1,
                "outcome": "rejected",
                "comment": "Fix HIGH findings"
            })),
            // Start round 2
            tool_call(8, "round_start", serde_json::json!({
                "session_id": sid
            })),
            // Send signal
            tool_call(9, "session_signal", serde_json::json!({
                "session_id": sid,
                "signal_type": "addressed",
                "source_label": "worker",
                "comment": "Fixed"
            })),
            // Read signals
            tool_call(10, "session_signals", serde_json::json!({
                "session_id": sid
            })),
            // Read grounded review back
            tool_call(11, "review_read", serde_json::json!({
                "session_id": sid,
                "round": 1,
                "reviewer": "grounded"
            })),
            // Get session
            tool_call(12, "session_get", serde_json::json!({
                "session_id": sid
            })),
            // List sessions
            tool_call(13, "session_list", serde_json::json!({})),
        ],
    );

    assert_eq!(responses.len(), 12);

    // Initialize
    assert_eq!(responses[0]["result"]["protocolVersion"], "2024-11-05");

    // Review writes (1-3) should succeed
    for i in 1..=3 {
        let data = extract_tool_text(&responses[i]);
        assert!(!is_tool_error(&responses[i]), "response {i} is error: {:?}", responses[i]);
        assert!(data["bytes_written"].as_i64().unwrap() > 0);
    }

    // Round status — all present
    let status = extract_tool_text(&responses[4]);
    assert_eq!(status["regular"], true);
    assert_eq!(status["harsh"], true);
    assert_eq!(status["grounded"], true);
    assert_eq!(status["all_reviews_present"], true);

    // Set outcome
    let outcome = extract_tool_text(&responses[5]);
    assert_eq!(outcome["outcome"], "rejected");

    // Round 2
    let round2 = extract_tool_text(&responses[6]);
    assert_eq!(round2["round_number"], 2);

    // Signal
    let signal = extract_tool_text(&responses[7]);
    assert_eq!(signal["signal_type"], "addressed");

    // Signals list
    let signals = extract_tool_text(&responses[8]);
    assert_eq!(signals["count"], 1);

    // Read review back
    let read = extract_tool_text(&responses[9]);
    assert_eq!(read["content"], "# Grounded\n\nVerified");

    // Session get — 2 rounds
    let session = extract_tool_text(&responses[10]);
    assert_eq!(session["total_rounds"], 2);

    // Session list
    let list = extract_tool_text(&responses[11]);
    assert!(list["count"].as_i64().unwrap() >= 1);
}

#[test]
fn test_e2e_cross_session_access() {
    // Simulates orchestrator and worker being separate server instances sharing same DB
    let tmp = tempfile::tempdir().unwrap();

    // Orchestrator creates session
    let r1 = send_jsonrpc_with_dir(
        tmp.path(),
        &[
            init_msg(),
            tool_call(2, "session_create", serde_json::json!({
                "target_path": "/home/user/code.rs",
                "review_type": "code"
            })),
        ],
    );
    let sid = extract_tool_text(&r1[1])["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    // Worker (separate server instance) writes review
    let r2 = send_jsonrpc_with_dir(
        tmp.path(),
        &[
            init_msg(),
            tool_call(2, "review_write", serde_json::json!({
                "session_id": sid,
                "round": 1,
                "reviewer": "regular",
                "content": "# Review from worker\n\nAll good."
            })),
            tool_call(3, "session_signal", serde_json::json!({
                "session_id": sid,
                "signal_type": "addressed",
                "source_label": "worker-regular"
            })),
        ],
    );
    assert!(!is_tool_error(&r2[1]));
    assert!(!is_tool_error(&r2[2]));

    // Orchestrator (another server instance) checks status and reads
    let r3 = send_jsonrpc_with_dir(
        tmp.path(),
        &[
            init_msg(),
            tool_call(2, "round_status", serde_json::json!({
                "session_id": sid
            })),
            tool_call(3, "session_signals", serde_json::json!({
                "session_id": sid
            })),
            tool_call(4, "review_read", serde_json::json!({
                "session_id": sid,
                "round": 1,
                "reviewer": "regular"
            })),
        ],
    );

    let status = extract_tool_text(&r3[1]);
    assert_eq!(status["regular"], true);
    assert_eq!(status["harsh"], false);

    let signals = extract_tool_text(&r3[2]);
    assert_eq!(signals["count"], 1);

    let review = extract_tool_text(&r3[3]);
    assert!(review["content"].as_str().unwrap().contains("All good"));
}

#[test]
fn test_e2e_duplicate_review_error() {
    let tmp = tempfile::tempdir().unwrap();

    let r1 = send_jsonrpc_with_dir(
        tmp.path(),
        &[
            init_msg(),
            tool_call(2, "session_create", serde_json::json!({
                "target_path": "/tmp/dup-test.rs",
                "review_type": "code"
            })),
        ],
    );
    let sid = extract_tool_text(&r1[1])["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let r2 = send_jsonrpc_with_dir(
        tmp.path(),
        &[
            init_msg(),
            tool_call(3, "review_write", serde_json::json!({
                "session_id": sid,
                "round": 1,
                "reviewer": "regular",
                "content": "first write"
            })),
            tool_call(4, "review_write", serde_json::json!({
                "session_id": sid,
                "round": 1,
                "reviewer": "regular",
                "content": "duplicate write"
            })),
        ],
    );

    // First write succeeds
    assert!(!is_tool_error(&r2[1]));

    // Second write fails with conflict
    assert!(is_tool_error(&r2[2]));
    let text = r2[2]["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("already exists"), "got: {text}");
}

#[test]
fn test_e2e_error_cases() {
    let responses = send_jsonrpc(&[
        init_msg(),
        // Missing params
        tool_call(2, "session_create", serde_json::json!({})),
        // Unknown tool
        tool_call(3, "nonexistent", serde_json::json!({})),
        // Get nonexistent session
        tool_call(5, "session_get", serde_json::json!({"session_id": "does-not-exist"})),
    ]);

    assert_eq!(responses.len(), 4);
    assert!(is_tool_error(&responses[1]));
    assert!(is_tool_error(&responses[2]));
    assert!(is_tool_error(&responses[3]));
}

#[test]
fn test_e2e_unknown_method() {
    let responses = send_jsonrpc(&[
        init_msg(),
        serde_json::json!({"jsonrpc":"2.0","id":4,"method":"unknown_method","params":{}}),
    ]);
    assert_eq!(responses.len(), 2);
    assert!(responses[1]["error"].is_object());
}

#[test]
fn test_e2e_parse_error() {
    let responses = send_raw(&["not valid json"]);
    assert_eq!(responses.len(), 1);
    assert!(responses[0]["error"].is_object());
    assert_eq!(responses[0]["error"]["code"], -32700);
}

#[test]
fn test_e2e_version_command() {
    let binary = env!("CARGO_BIN_EXE_review-mcp");
    let output = Command::new(binary)
        .arg("--version")
        .output()
        .expect("failed to run review-mcp");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("review-mcp v"));
    assert!(stdout.contains("commit:"));
    assert!(stdout.contains("built:"));
}

#[test]
fn test_e2e_audit_empty() {
    let binary = env!("CARGO_BIN_EXE_review-mcp");
    let tmp = tempfile::tempdir().unwrap();
    let output = Command::new(binary)
        .args(["audit"])
        .env("XDG_DATA_HOME", tmp.path())
        .output()
        .expect("failed to run audit");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No review sessions found"));
}

#[test]
fn test_e2e_session_get_by_target() {
    let tmp = tempfile::tempdir().unwrap();

    let r1 = send_jsonrpc_with_dir(
        tmp.path(),
        &[
            init_msg(),
            tool_call(2, "session_create", serde_json::json!({
                "target_path": "/tmp/find-by-target.rs",
                "review_type": "manuscript"
            })),
        ],
    );
    let sid = extract_tool_text(&r1[1])["session_id"]
        .as_str()
        .unwrap()
        .to_string();

    let r2 = send_jsonrpc_with_dir(
        tmp.path(),
        &[
            init_msg(),
            tool_call(2, "session_get", serde_json::json!({
                "target_path": "/tmp/find-by-target.rs"
            })),
        ],
    );
    let session = extract_tool_text(&r2[1]);
    assert_eq!(session["session"]["id"], sid);
}
