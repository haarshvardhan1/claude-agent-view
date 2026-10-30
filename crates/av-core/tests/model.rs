use av_core::model::{
    Agent, Change, Context, PendingToolView, Status, Subagent, SubagentState, Tokens,
    TranscriptEntry,
};
use chrono::{DateTime, Utc};
use serde_json::{json, to_value};

fn ts(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}

fn sample_agent() -> Agent {
    Agent {
        id: "a1".into(),
        pid: 4242,
        cwd: "/Users/g2c/proj".into(),
        project_name: "proj".into(),
        git_branch: Some("main".into()),
        title: "Fix bug".into(),
        entrypoint: "cli".into(),
        model: Some("claude-opus-5".into()),
        status: Status::Working,
        status_since: ts("2026-10-29T14:00:00Z"),
        started_at: ts("2026-10-29T13:00:00Z"),
        last_activity: Some(ts("2026-10-29T14:00:01Z")),
        context: Context { used: Some(1000), limit: Some(200_000) },
        tokens: Tokens { input: 1, output: 2, cache_write: 3, cache_read: 4 },
        pending_tool: Some(PendingToolView {
            name: "Bash".into(),
            since: ts("2026-10-29T14:00:00Z"),
        }),
        subagents: vec![Subagent {
            id: "s1".into(),
            agent_type: "Explore".into(),
            description: "look around".into(),
            background: false,
            status: SubagentState::Running,
            last_activity: None,
            tokens: Tokens { input: 0, output: 0, cache_write: 0, cache_read: 0 },
            pending_tool: None,
            transcript_path: "/tmp/sub.jsonl".into(),
        }],
        transcript_path: Some("/tmp/main.jsonl".into()),
        skipped_lines: 0,
    }
}

#[test]
fn status_is_kind_tagged() {
    assert_eq!(
        to_value(Status::NeedsPermission { tool: Some("Bash".into()) }).unwrap(),
        json!({"kind": "needsPermission", "tool": "Bash"})
    );
    assert_eq!(to_value(Status::MaybeWaiting).unwrap(), json!({"kind": "maybeWaiting"}));
    assert_eq!(
        to_value(Status::Unknown { reason: "x".into() }).unwrap(),
        json!({"kind": "unknown", "reason": "x"})
    );
}

#[test]
fn error_kind_field_is_renamed_to_avoid_tag_clash() {
    assert_eq!(
        to_value(Status::Error { kind: Some("api".into()) }).unwrap(),
        json!({"kind": "error", "errorKind": "api"})
    );
}

#[test]
fn change_is_type_tagged() {
    assert_eq!(
        to_value(Change::AgentRemoved { id: "a".into() }).unwrap(),
        json!({"type": "agentRemoved", "id": "a"})
    );
}

#[test]
fn agent_fields_are_camel_case() {
    let v = to_value(sample_agent()).unwrap();
    let obj = v.as_object().unwrap();
    for key in [
        "projectName",
        "statusSince",
        "pendingTool",
        "transcriptPath",
        "skippedLines",
        "startedAt",
        "lastActivity",
        "gitBranch",
    ] {
        assert!(obj.contains_key(key), "missing key {key}");
    }
    assert_eq!(v["tokens"]["cacheWrite"], json!(3));
    assert_eq!(v["tokens"]["cacheRead"], json!(4));
    assert!(v["subagents"][0].get("agentType").is_some());
    assert!(v["subagents"][0].get("transcriptPath").is_some());
    assert_eq!(v["subagents"][0]["status"], json!("running"));
    assert_eq!(v["pendingTool"]["name"], json!("Bash"));
    fn no_underscore_keys(v: &serde_json::Value) {
        match v {
            serde_json::Value::Object(m) => {
                for (k, val) in m {
                    assert!(!k.contains('_'), "snake_case key {k}");
                    no_underscore_keys(val);
                }
            }
            serde_json::Value::Array(a) => a.iter().for_each(no_underscore_keys),
            _ => {}
        }
    }
    no_underscore_keys(&v);
}

#[test]
fn needs_you_set() {
    assert!(Status::NeedsPermission { tool: None }.needs_you());
    assert!(Status::WaitingInput.needs_you());
    assert!(Status::Error { kind: None }.needs_you());
    assert!(!Status::MaybeWaiting.needs_you());
    assert!(!Status::Working.needs_you());
    assert!(!Status::Idle.needs_you());
    assert!(!Status::Exited.needs_you());
    assert!(!Status::Unknown { reason: "x".into() }.needs_you());
}

#[test]
fn busy_set() {
    assert!(Status::Working.is_busy());
    assert!(Status::MaybeWaiting.is_busy());
    assert!(Status::NeedsPermission { tool: None }.is_busy());
    assert!(!Status::WaitingInput.is_busy());
    assert!(!Status::Idle.is_busy());
    assert!(!Status::Error { kind: None }.is_busy());
    assert!(!Status::Exited.is_busy());
}

#[test]
fn tool_call_entry_shape() {
    let e = TranscriptEntry::ToolCall {
        id: "t1".into(),
        name: "Bash".into(),
        summary: "ls".into(),
        input: "{}".into(),
        result: None,
        is_error: true,
        timestamp: None,
    };
    let v = to_value(e).unwrap();
    assert_eq!(v["kind"], json!("toolCall"));
    assert_eq!(v["isError"], json!(true));
    assert!(v.get("is_error").is_none());
}
