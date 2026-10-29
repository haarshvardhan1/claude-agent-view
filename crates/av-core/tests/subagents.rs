mod common;
use av_core::subagents::{read_subagents, SubagentStatus};
use common::*;
use std::path::PathBuf;

const META: &str = r#"{"agentType":"Explore","description":"Check frontend","toolUseId":"toolu_1","spawnDepth":1,"requestShape":"background","requestNonInteractive":true}"#;

fn transcript(dir: &tempfile::TempDir) -> PathBuf {
    dir.path().join("s.jsonl")
}

#[test]
fn reads_meta_fields() {
    let d = tempfile::tempdir().unwrap();
    let t = transcript(&d);
    write_subagent(&t, "a1", &[user_prompt("go", T0)], Some(META));
    let v = read_subagents(&t, true);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].agent_id, "a1");
    assert_eq!(v[0].agent_type, "Explore");
    assert_eq!(v[0].description, "Check frontend");
    assert!(v[0].background);
}

#[test]
fn handback_means_done() {
    let d = tempfile::tempdir().unwrap();
    let t = transcript(&d);
    write_subagent(
        &t,
        "a1",
        &[
            user_prompt("go", T0),
            assistant_tool_use("m1", "tu1", "SubagentHandback", T0),
            tool_result("tu1", T0),
        ],
        Some(META),
    );
    assert_eq!(read_subagents(&t, false)[0].status, SubagentStatus::Done);
}

#[test]
fn open_work_with_live_parent_is_running() {
    let d = tempfile::tempdir().unwrap();
    let t = transcript(&d);
    write_subagent(&t, "a1", &[user_prompt("go", T0), assistant_tool_use("m1", "tu1", "Read", T0)], None);
    assert_eq!(read_subagents(&t, true)[0].status, SubagentStatus::Running);
}

#[test]
fn open_work_with_dead_parent_is_stopped() {
    let d = tempfile::tempdir().unwrap();
    let t = transcript(&d);
    write_subagent(&t, "a1", &[user_prompt("go", T0), assistant_tool_use("m1", "tu1", "Read", T0)], None);
    assert_eq!(read_subagents(&t, false)[0].status, SubagentStatus::Stopped);
}

#[test]
fn missing_meta_uses_agent_id() {
    let d = tempfile::tempdir().unwrap();
    let t = transcript(&d);
    write_subagent(&t, "a1", &[user_prompt("go", T0)], None);
    let v = read_subagents(&t, true);
    assert_eq!(v[0].agent_type, "unknown");
    assert_eq!(v[0].description, "a1");
    assert!(!v[0].background);
}

#[test]
fn no_subagents_dir_is_empty() {
    let d = tempfile::tempdir().unwrap();
    assert!(read_subagents(&transcript(&d), true).is_empty());
}

#[test]
fn corrupt_meta_treated_as_missing_and_meta_files_ignored() {
    let d = tempfile::tempdir().unwrap();
    let t = transcript(&d);
    write_subagent(&t, "a1", &[user_prompt("go", T0)], Some("{not json"));
    let v = read_subagents(&t, true);
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].agent_type, "unknown");
}

#[test]
fn sorted_newest_first_none_last() {
    let d = tempfile::tempdir().unwrap();
    let t = transcript(&d);
    write_subagent(&t, "old", &[user_prompt("x", "2026-10-29T10:00:00.000Z")], None);
    write_subagent(&t, "new", &[user_prompt("x", "2026-10-29T12:00:00.000Z")], None);
    write_subagent(&t, "none", &[], None);
    let ids: Vec<_> = read_subagents(&t, true).into_iter().map(|s| s.agent_id).collect();
    assert_eq!(ids, ["new", "old", "none"]);
}
