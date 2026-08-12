use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Normalized session events written to Journal before UI push.
/// Keeping a stable shape means UI replay works even when OpenCode `_meta` drifts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum NormalizedEvent {
    Message {
        role: String,
        text: String,
    },
    Thought {
        text: String,
    },
    ToolCall {
        tool_call_id: String,
        title: String,
        /// ACP tool kind (edit/execute/...). Named tool_kind so it does not
        /// collide with the serde externally-tagged `kind` discriminator.
        tool_kind: Option<String>,
        status: Option<String>,
    },
    ToolCallUpdate {
        tool_call_id: String,
        status: Option<String>,
        diff: Option<String>,
        terminal: Option<String>,
    },
    Diff {
        path: Option<String>,
        diff: String,
    },
    PermissionRequest {
        request_id: Option<String>,
        tool_call_id: Option<String>,
        op_type: Option<String>,
        raw: Value,
    },
    ConfigOptionUpdate {
        options: Value,
    },
    /// Unknown extension / `_meta` only — ignored by UI but logged for debugging.
    Ignored {
        reason: String,
        raw: Value,
    },
    Other {
        session_update: String,
        raw: Value,
    },
}

pub fn normalize_session_update(params: &Value) -> Vec<NormalizedEvent> {
    let update = match params.get("update") {
        Some(u) => u,
        None => {
            return vec![NormalizedEvent::Ignored {
                reason: "missing update field".into(),
                raw: params.clone(),
            }]
        }
    };
    let kind = update
        .get("sessionUpdate")
        .or_else(|| update.get("session_update"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    match kind {
        "agent_message_chunk" | "user_message_chunk" | "agent_thought_chunk" => {
            let text = extract_text(update);
            if kind == "agent_thought_chunk" {
                vec![NormalizedEvent::Thought { text }]
            } else {
                let role = if kind.starts_with("user") {
                    "user"
                } else {
                    "agent"
                };
                vec![NormalizedEvent::Message {
                    role: role.into(),
                    text,
                }]
            }
        }
        "tool_call" => {
            vec![NormalizedEvent::ToolCall {
                tool_call_id: update
                    .get("toolCallId")
                    .or_else(|| update.get("tool_call_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .into(),
                title: update
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("tool")
                    .into(),
                tool_kind: update
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                status: update
                    .get("status")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
            }]
        }
        "tool_call_update" => {
            let diff = update
                .pointer("/content")
                .and_then(|c| c.as_array())
                .and_then(|arr| {
                    arr.iter().find_map(|b| {
                        if b.get("type").and_then(|t| t.as_str()) == Some("diff") {
                            b.get("diff")
                                .or_else(|| b.get("text"))
                                .and_then(|d| d.as_str())
                                .map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                });
            let terminal = update
                .pointer("/content")
                .and_then(|c| c.as_array())
                .and_then(|arr| {
                    arr.iter().find_map(|b| {
                        let t = b.get("type").and_then(|t| t.as_str());
                        if t == Some("terminal") || t == Some("output") {
                            b.get("output")
                                .or_else(|| b.get("text"))
                                .and_then(|d| d.as_str())
                                .map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                });
            vec![NormalizedEvent::ToolCallUpdate {
                tool_call_id: update
                    .get("toolCallId")
                    .or_else(|| update.get("tool_call_id"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .into(),
                status: update
                    .get("status")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string()),
                diff,
                terminal,
            }]
        }
        "config_option_update" => {
            vec![NormalizedEvent::ConfigOptionUpdate {
                options: update
                    .get("configOptions")
                    .cloned()
                    .unwrap_or(Value::Null),
            }]
        }
        "" => {
            // Pure _meta / extension — ignore per protocol guidance.
            if update.get("_meta").is_some() || kind.starts_with('_') {
                vec![NormalizedEvent::Ignored {
                    reason: "extension/_meta".into(),
                    raw: update.clone(),
                }]
            } else {
                vec![NormalizedEvent::Ignored {
                    reason: "empty sessionUpdate".into(),
                    raw: update.clone(),
                }]
            }
        }
        other if other.starts_with('_') => {
            vec![NormalizedEvent::Ignored {
                reason: "extension method".into(),
                raw: update.clone(),
            }]
        }
        other => vec![NormalizedEvent::Other {
            session_update: other.into(),
            raw: update.clone(),
        }],
    }
}

pub fn normalize_permission_request(params: &Value) -> NormalizedEvent {
    NormalizedEvent::PermissionRequest {
        request_id: params
            .get("requestId")
            .or_else(|| params.get("id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        tool_call_id: params
            .pointer("/toolCall/toolCallId")
            .or_else(|| params.get("toolCallId"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        op_type: params
            .get("opType")
            .or_else(|| params.get("op_type"))
            .or_else(|| params.pointer("/toolCall/kind"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        raw: params.clone(),
    }
}

fn extract_text(update: &Value) -> String {
    if let Some(c) = update.get("content") {
        if let Some(s) = c.get("text").and_then(|t| t.as_str()) {
            return s.to_string();
        }
        if let Some(s) = c.as_str() {
            return s.to_string();
        }
        if let Some(arr) = c.as_array() {
            return arr
                .iter()
                .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                .collect::<Vec<_>>()
                .join("");
        }
    }
    update
        .get("text")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string()
}

/// Helper for tests / fixture replay.
pub fn fixture_transcript() -> Value {
    json!([
        {
            "sessionId": "sess_fixture",
            "update": {
                "sessionUpdate": "agent_thought_chunk",
                "content": { "type": "text", "text": "I should inspect src/lib.rs first." }
            }
        },
        {
            "sessionId": "sess_fixture",
            "update": {
                "sessionUpdate": "agent_message_chunk",
                "content": { "type": "text", "text": "I'll fix the add function." }
            }
        },
        {
            "sessionId": "sess_fixture",
            "update": {
                "sessionUpdate": "tool_call",
                "toolCallId": "call_1",
                "title": "Edit src/lib.rs",
                "kind": "edit",
                "status": "pending"
            }
        },
        {
            "sessionId": "sess_fixture",
            "update": {
                "sessionUpdate": "tool_call_update",
                "toolCallId": "call_1",
                "status": "completed",
                "content": [
                    {
                        "type": "diff",
                        "path": "src/lib.rs",
                        "diff": "@@\n-pub fn add(a:i32,b:i32)->i32{a-b}\n+pub fn add(a:i32,b:i32)->i32{a+b}\n"
                    },
                    {
                        "type": "terminal",
                        "output": "$ cargo test\nok\n"
                    }
                ]
            }
        },
        {
            "sessionId": "sess_fixture",
            "update": {
                "sessionUpdate": "_custom_extension",
                "data": 1,
                "_meta": { "x": true }
            }
        }
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_message_thought_tool_diff() {
        let fixtures = fixture_transcript();
        let arr = fixtures.as_array().unwrap();
        let n0 = normalize_session_update(&arr[0]);
        assert!(matches!(&n0[0], NormalizedEvent::Thought { text } if text.contains("inspect")));
        let n1 = normalize_session_update(&arr[1]);
        assert!(matches!(&n1[0], NormalizedEvent::Message { role, .. } if role == "agent"));
        let n2 = normalize_session_update(&arr[2]);
        assert!(matches!(&n2[0], NormalizedEvent::ToolCall { tool_call_id, .. } if tool_call_id == "call_1"));
        let n3 = normalize_session_update(&arr[3]);
        match &n3[0] {
            NormalizedEvent::ToolCallUpdate { diff, terminal, .. } => {
                assert!(diff.as_ref().unwrap().contains("a+b"));
                assert!(terminal.as_ref().unwrap().contains("cargo test"));
            }
            other => panic!("{other:?}"),
        }
        let n4 = normalize_session_update(&arr[4]);
        assert!(matches!(&n4[0], NormalizedEvent::Ignored { .. }));
    }
}
