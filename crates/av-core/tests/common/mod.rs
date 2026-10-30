#![allow(dead_code)]

use std::fs;
use std::path::Path;

/// Write a realistic session registry file to `<home>/sessions/<pid>.json`.
pub fn write_session(home: &Path, pid: u32, session_id: &str, cwd: &str, status: &str) {
    write_session_at(home, pid, session_id, cwd, status, 1793281174333);
}

pub fn write_session_at(
    home: &Path,
    pid: u32,
    session_id: &str,
    cwd: &str,
    status: &str,
    started_at: i64,
) {
    let dir = home.join("sessions");
    fs::create_dir_all(&dir).unwrap();
    let body = serde_json::json!({
        "pid": pid,
        "sessionId": session_id,
        "cwd": cwd,
        "startedAt": started_at,
        "procStart": "Thu Oct 29 12:36:53 2026",
        "version": "2.1.312",
        "peerProtocol": 1,
        "kind": "interactive",
        "entrypoint": "cli",
        "name": format!("agent-{pid}"),
        "status": status,
        "updatedAt": started_at + 20000,
    });
    fs::write(dir.join(format!("{pid}.json")), body.to_string()).unwrap();
}

use av_core::liveness::ProcessProbe;
use av_core::registry::SessionEntry;
use std::collections::HashMap;

pub struct FakeProbe(pub HashMap<u32, String>);

impl ProcessProbe for FakeProbe {
    fn proc_start(&self, pid: u32) -> Option<String> {
        self.0.get(&pid).cloned()
    }
}

pub fn entry(pid: u32, proc_start: Option<&str>) -> SessionEntry {
    serde_json::from_value(serde_json::json!({
        "pid": pid,
        "sessionId": "s",
        "procStart": proc_start,
    }))
    .unwrap()
}

// ---- transcript line builders ----

pub const T0: &str = "2026-10-29T13:39:54.792Z";

pub fn user_prompt(text: &str, ts: &str) -> String {
    serde_json::json!({
        "type": "user", "isSidechain": false, "timestamp": ts,
        "message": {"role": "user", "content": text},
        "gitBranch": "main", "sessionId": "s", "version": "2.1.312",
    })
    .to_string()
}

pub fn user_meta(text: &str, ts: &str) -> String {
    serde_json::json!({
        "type": "user", "isMeta": true, "timestamp": ts,
        "message": {"role": "user", "content": [{"type": "text", "text": text}]},
    })
    .to_string()
}

pub fn ai_title(text: &str) -> String {
    serde_json::json!({"type": "ai-title", "aiTitle": text, "sessionId": "s"}).to_string()
}

fn usage(i: u64, o: u64) -> serde_json::Value {
    serde_json::json!({
        "input_tokens": i, "output_tokens": o,
        "cache_creation_input_tokens": 3, "cache_read_input_tokens": 7,
    })
}

pub fn assistant_tool_use(msg_id: &str, tool_id: &str, name: &str, ts: &str) -> String {
    serde_json::json!({
        "type": "assistant", "timestamp": ts, "gitBranch": "main",
        "message": {
            "id": msg_id, "model": "claude-opus-5-5", "role": "assistant",
            "content": [{"type": "tool_use", "id": tool_id, "name": name, "input": {}}],
            "stop_reason": "tool_use", "usage": usage(1, 1),
        },
    })
    .to_string()
}

pub fn assistant_text(msg_id: &str, stop_reason: &str, i: u64, o: u64, ts: &str) -> String {
    serde_json::json!({
        "type": "assistant", "timestamp": ts, "gitBranch": "main",
        "message": {
            "id": msg_id, "model": "claude-opus-5-5", "role": "assistant",
            "content": [{"type": "text", "text": "hello"}],
            "stop_reason": stop_reason, "usage": usage(i, o),
        },
    })
    .to_string()
}

pub fn tool_result(tool_id: &str, ts: &str) -> String {
    serde_json::json!({
        "type": "user", "timestamp": ts,
        "message": {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": tool_id, "content": "ok", "is_error": false}
        ]},
    })
    .to_string()
}

pub fn system_error(ts: &str) -> String {
    serde_json::json!({
        "type": "system", "subtype": "api_error", "level": "error",
        "content": "boom", "timestamp": ts,
    })
    .to_string()
}

pub fn system_turn_duration(ts: &str) -> String {
    serde_json::json!({
        "type": "system", "subtype": "turn_duration", "durationMs": 1000,
        "messageCount": 2, "timestamp": ts,
    })
    .to_string()
}

pub fn assistant_api_error(ts: &str) -> String {
    serde_json::json!({
        "type": "assistant", "isApiErrorMessage": true, "error": "authentication_failed",
        "timestamp": ts,
        "message": {
            "id": "synthetic", "model": "<synthetic>", "role": "assistant",
            "stop_reason": "stop_sequence",
            "content": [{"type": "text", "text": "Login expired · Please run /login"}],
            "usage": {"input_tokens": 0, "output_tokens": 0},
        },
    })
    .to_string()
}

// ---- subagent fixtures ----

/// Write `<transcript sans .jsonl>/subagents/agent-<id>.jsonl` (+ optional meta).
pub fn write_subagent(transcript: &Path, id: &str, lines: &[String], meta: Option<&str>) {
    let dir = av_core::paths::subagents_dir(transcript);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!("agent-{id}.jsonl")), lines.join("\n") + "\n").unwrap();
    if let Some(m) = meta {
        fs::write(dir.join(format!("agent-{id}.meta.json")), m).unwrap();
    }
}

// ---- tail builders ----

pub fn agent_name(name: &str) -> String {
    serde_json::json!({"type": "agent-name", "agentName": name, "sessionId": "s"}).to_string()
}

pub fn assistant_text_cached(
    msg_id: &str,
    i: u64,
    o: u64,
    cache_write: u64,
    cache_read: u64,
    model: &str,
    ts: &str,
) -> String {
    serde_json::json!({
        "type": "assistant", "timestamp": ts, "gitBranch": "main",
        "message": {
            "id": msg_id, "model": model, "role": "assistant",
            "content": [{"type": "text", "text": "hello"}],
            "stop_reason": "end_turn",
            "usage": {
                "input_tokens": i, "output_tokens": o,
                "cache_creation_input_tokens": cache_write,
                "cache_read_input_tokens": cache_read,
            },
        },
    })
    .to_string()
}

pub fn assistant_api_error_kind(kind: &str, ts: &str) -> String {
    serde_json::json!({
        "type": "assistant", "isApiErrorMessage": true, "error": kind,
        "timestamp": ts,
        "message": {
            "id": "synthetic", "model": "<synthetic>", "role": "assistant",
            "stop_reason": "stop_sequence",
            "content": [{"type": "text", "text": "error"}],
            "usage": {"input_tokens": 0, "output_tokens": 0},
        },
    })
    .to_string()
}
