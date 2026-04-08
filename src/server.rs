use crate::db::Db;
use crate::mcp::{JsonRpcRequest, JsonRpcResponse, INVALID_PARAMS, METHOD_NOT_FOUND, PARSE_ERROR};
use std::io::{self, BufRead, Write};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let db = Db::open()?;

    let stdin = io::stdin();
    let stdout = io::stdout();
    let reader = stdin.lock();
    let mut writer = stdout.lock();

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse::error(None, PARSE_ERROR, format!("parse error: {e}"));
                write_response(&mut writer, &resp)?;
                continue;
            }
        };

        let response = handle_request(&db, request);
        write_response(&mut writer, &response)?;
    }

    Ok(())
}

fn write_response(
    writer: &mut impl Write,
    response: &JsonRpcResponse,
) -> Result<(), Box<dyn std::error::Error>> {
    let json = serde_json::to_string(response)?;
    writer.write_all(json.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

fn handle_request(db: &Db, request: JsonRpcRequest) -> JsonRpcResponse {
    let id = request.id.clone();

    match request.method.as_str() {
        "initialize" => handle_initialize(id),
        "initialized" => JsonRpcResponse::empty(id),
        "notifications/initialized" => JsonRpcResponse::empty(id),
        "tools/list" => {
            let tools = crate::tools::list_tools();
            JsonRpcResponse::success(id, serde_json::json!({"tools": tools}))
        }
        "tools/call" => handle_tools_call(db, id.clone(), &request),
        _ => JsonRpcResponse::error(
            id,
            METHOD_NOT_FOUND,
            format!("method not found: {}", request.method),
        ),
    }
}

fn handle_initialize(id: Option<serde_json::Value>) -> JsonRpcResponse {
    JsonRpcResponse::success(
        id,
        serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "tools": {}
            },
            "serverInfo": {
                "name": "review-mcp",
                "version": env!("BUILD_VERSION")
            }
        }),
    )
}

fn handle_tools_call(
    db: &Db,
    id: Option<serde_json::Value>,
    request: &JsonRpcRequest,
) -> JsonRpcResponse {
    let params = match &request.params {
        Some(p) => p,
        None => {
            return JsonRpcResponse::error(id, INVALID_PARAMS, "missing params".to_string());
        }
    };

    let tool_name = match params.get("name").and_then(|v| v.as_str()) {
        Some(n) => n,
        None => {
            return JsonRpcResponse::error(id, INVALID_PARAMS, "missing tool name".to_string());
        }
    };

    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or(serde_json::Value::Object(Default::default()));

    let result = crate::tools::call_tool(db, tool_name, arguments);
    JsonRpcResponse::success(id, result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_initialize() {
        let resp = handle_initialize(Some(serde_json::json!(1)));
        let result = resp.result.unwrap();
        assert_eq!(result["protocolVersion"], "2024-11-05");
        assert_eq!(result["serverInfo"]["name"], "review-mcp");
    }

    #[test]
    fn test_handle_unknown_method() {
        let db = Db::open_memory().unwrap();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "unknown".to_string(),
            params: None,
        };
        let resp = handle_request(&db, request);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, METHOD_NOT_FOUND);
    }

    #[test]
    fn test_handle_tools_list() {
        let db = Db::open_memory().unwrap();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "tools/list".to_string(),
            params: Some(serde_json::json!({})),
        };
        let resp = handle_request(&db, request);
        let result = resp.result.unwrap();
        assert!(result["tools"].is_array());
        assert_eq!(result["tools"].as_array().unwrap().len(), 10);
    }

    #[test]
    fn test_handle_tools_call_missing_params() {
        let db = Db::open_memory().unwrap();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "tools/call".to_string(),
            params: None,
        };
        let resp = handle_request(&db, request);
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, INVALID_PARAMS);
    }

    #[test]
    fn test_handle_tools_call_session_create() {
        let db = Db::open_memory().unwrap();
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(serde_json::json!(1)),
            method: "tools/call".to_string(),
            params: Some(serde_json::json!({
                "name": "session_create",
                "arguments": {
                    "target_path": "/tmp/test.rs",
                    "review_type": "code"
                }
            })),
        };
        let resp = handle_request(&db, request);
        assert!(resp.error.is_none());
        let result = resp.result.unwrap();
        // Tool results are wrapped in content array
        assert!(result["content"][0]["text"].is_string());
    }
}
