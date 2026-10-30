use std::fs;
use std::path::Path;

pub const CWD: &str = "/tmp/myproj";
pub const SESSION: &str = "sess-1";
pub const PID: u32 = 999999;

/// Fixture home with one dead-pid session, titled transcript, and one subagent.
pub fn fixture_home(home: &Path) {
    let sessions = home.join("sessions");
    fs::create_dir_all(&sessions).unwrap();
    let body = serde_json::json!({
        "pid": PID, "sessionId": SESSION, "cwd": CWD, "startedAt": 1793281174333i64,
        "procStart": "Thu Oct 29 12:36:53 2026", "version": "2.1.312", "peerProtocol": 1,
        "kind": "interactive", "entrypoint": "cli", "name": "my-session",
        "status": "idle", "updatedAt": 1793281194333i64,
    });
    fs::write(sessions.join(format!("{PID}.json")), body.to_string()).unwrap();

    let proj = home.join("projects").join("-tmp-myproj");
    fs::create_dir_all(&proj).unwrap();
    let t = proj.join(format!("{SESSION}.jsonl"));
    let title =
        serde_json::json!({"type": "ai-title", "aiTitle": "Fix the widget", "sessionId": SESSION});
    fs::write(&t, format!("{title}\n")).unwrap();

    let sub = proj.join(SESSION).join("subagents");
    fs::create_dir_all(&sub).unwrap();
    let line = serde_json::json!({
        "type": "user", "timestamp": "2026-10-29T13:39:54.792Z",
        "message": {"role": "user", "content": "go"},
    });
    fs::write(sub.join("agent-a1.jsonl"), format!("{line}\n")).unwrap();
    fs::write(
        sub.join("agent-a1.meta.json"),
        r#"{"agentType":"Explore","description":"look around"}"#,
    )
    .unwrap();
}
