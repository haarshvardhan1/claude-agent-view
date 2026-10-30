use av_core::model::{Status, SubagentState};
use av_core::registry::SessionEntry;
use av_core::subagents::{SubagentInfo, SubagentStatus};
use av_core::transcript::{PendingTool, TokenTotals, TranscriptSummary};
use av_core::view::{build_agent, build_subagent, context_limit, pick_title, project_name};
use chrono::{TimeZone, Utc};
use std::path::Path;

fn entry() -> SessionEntry {
    serde_json::from_str(
        r#"{"pid":42,"sessionId":"sess-1","cwd":"/a/proj","name":"","status":"busy",
        "kind":"interactive","entrypoint":"cli","version":"1","startedAt":1700000000000,"updatedAt":1700000001000}"#,
    )
    .unwrap()
}

#[test]
fn limit_rules() {
    assert_eq!(context_limit(None, 0), None);
    assert_eq!(
        context_limit(Some("claude-opus-5-5"), 150_000),
        Some(200_000)
    );
    assert_eq!(
        context_limit(Some("claude-opus-5-5[1m]"), 0),
        Some(1_000_000)
    );
    assert_eq!(
        context_limit(Some("claude-opus-5-5"), 250_000),
        Some(1_000_000)
    );
}

#[test]
fn title_priority() {
    let mut s = TranscriptSummary::default();
    assert_eq!(pick_title(&s, ""), "(untitled)");
    s.title = Some("T".into());
    assert_eq!(pick_title(&s, ""), "T");
    assert_eq!(pick_title(&s, "  \t"), "T");
    assert_eq!(pick_title(&s, "Reg"), "Reg");
    s.agent_name = Some("Named".into());
    assert_eq!(pick_title(&s, "Reg"), "Named");
    s.agent_name = Some("   ".into());
    assert_eq!(pick_title(&s, "Reg"), "Reg");
}

#[test]
fn project_name_cases() {
    assert_eq!(project_name("/a/b/"), "b");
    assert_eq!(project_name("/a/b"), "b");
    assert_eq!(project_name("/"), "/");
    assert_eq!(project_name(""), "");
}

#[test]
fn build_agent_maps_fields() {
    let mut s = TranscriptSummary::default();
    s.model = Some("claude-opus-5-5".into());
    s.context_used = Some(1234);
    s.max_context_used = 10;
    s.tokens = TokenTotals {
        input: 1,
        output: 2,
        cache_read: 3,
        cache_write: 4,
    };
    s.git_branch = Some("main".into());
    s.pending_tool = Some(PendingTool {
        name: "Bash".into(),
        since: Utc.timestamp_opt(5, 0).unwrap(),
    });
    let since = Utc.timestamp_opt(100, 0).unwrap();
    let a = build_agent(
        &entry(),
        &s,
        vec![],
        Status::Working,
        since,
        Some(Path::new("/t/x.jsonl")),
    );
    assert_eq!(a.id, "sess-1");
    assert_eq!(a.pid, 42);
    assert_eq!(a.project_name, "proj");
    assert_eq!(a.title, "(untitled)");
    assert_eq!(
        a.started_at,
        Utc.timestamp_millis_opt(1_700_000_000_000).unwrap()
    );
    assert_eq!(a.status_since, since);
    assert_eq!(
        (
            a.tokens.input,
            a.tokens.output,
            a.tokens.cache_read,
            a.tokens.cache_write
        ),
        (1, 2, 3, 4)
    );
    assert_eq!(a.context.used, Some(1234));
    assert_eq!(a.context.limit, Some(200_000));
    assert_eq!(a.pending_tool.unwrap().name, "Bash");
    assert_eq!(a.transcript_path.as_deref(), Some("/t/x.jsonl"));
}

#[test]
fn build_agent_out_of_range_start_falls_back_to_epoch() {
    let mut e = entry();
    e.started_at_ms = i64::MAX;
    let a = build_agent(
        &e,
        &TranscriptSummary::default(),
        vec![],
        Status::Idle,
        Utc::now(),
        None,
    );
    assert_eq!(a.started_at, Utc.timestamp_opt(0, 0).unwrap());
    assert_eq!(a.transcript_path, None);
}

#[test]
fn build_subagent_maps_fields() {
    let info = SubagentInfo {
        agent_id: "ag1".into(),
        agent_type: "Explore".into(),
        description: "d".into(),
        background: true,
        status: SubagentStatus::Done,
        summary: TranscriptSummary::default(),
    };
    let s = build_subagent(&info, Path::new("/t/sub.jsonl"));
    assert_eq!(s.id, "ag1");
    assert_eq!(s.status, SubagentState::Done);
    assert!(s.background);
    assert_eq!(s.transcript_path, "/t/sub.jsonl");
}
