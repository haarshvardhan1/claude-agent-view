mod common;
use av_core::status::AgentStatus;
use av_core::{scan, SystemProbe};
use common::*;
use std::collections::HashMap;
use std::fs;

const PS: &str = "Thu Oct 29 12:36:53 2026";

fn now() -> chrono::DateTime<chrono::Utc> {
    "2026-10-29T14:00:00Z".parse().unwrap()
}

fn alive(pid: u32) -> FakeProbe {
    FakeProbe(HashMap::from([(pid, PS.to_string())]))
}

#[test]
fn live_session_full_picture() {
    let d = tempfile::tempdir().unwrap();
    write_session(d.path(), 100, "abc", "/Users/g2c/repo", "busy");
    let t = d.path().join("projects/-Users-g2c-repo/abc.jsonl");
    fs::create_dir_all(t.parent().unwrap()).unwrap();
    fs::write(&t, [user_prompt("hi", T0), ai_title("Fix the bug")].join("\n") + "\n").unwrap();
    write_subagent(&t, "a1", &[user_prompt("go", T0)], None);
    let r = scan(d.path(), &alive(100), now());
    assert_eq!(r.agents.len(), 1);
    let a = &r.agents[0];
    assert!(a.alive);
    assert_eq!(a.title, "Fix the bug");
    assert_eq!(a.status, AgentStatus::Working);
    assert_eq!(a.subagents.len(), 1);
    assert!(r.warnings.is_empty());
}

#[test]
fn session_without_transcript_is_listed() {
    let d = tempfile::tempdir().unwrap();
    write_session(d.path(), 100, "abc", "/Users/g2c/repo", "idle");
    let r = scan(d.path(), &alive(100), now());
    assert_eq!(r.agents.len(), 1);
    assert_eq!(r.agents[0].title, "(new session)");
    assert_eq!(r.agents[0].status, AgentStatus::Idle);
    assert!(r.agents[0].subagents.is_empty());
    assert!(r.warnings.is_empty());
}

#[test]
fn dead_session_is_exited() {
    let d = tempfile::tempdir().unwrap();
    write_session(d.path(), 100, "abc", "/Users/g2c/repo", "busy");
    let r = scan(d.path(), &FakeProbe(HashMap::new()), now());
    assert_eq!(r.agents[0].status, AgentStatus::Exited);
    assert!(!r.agents[0].alive);
}

#[test]
fn registry_order_preserved_and_serializes() {
    let d = tempfile::tempdir().unwrap();
    write_session_at(d.path(), 1, "old", "/x", "idle", 1000);
    write_session_at(d.path(), 2, "new", "/x", "idle", 2000);
    let r = scan(d.path(), &alive(2), now());
    assert_eq!(r.agents[0].entry.session_id, "new");
    serde_json::to_string(&r).unwrap();
    let _ = SystemProbe;
}
