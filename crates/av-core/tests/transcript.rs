mod common;
use av_core::transcript::summarize_transcript;
use chrono::{DateTime, Utc};
use common::*;
use std::fs;

fn run(lines: &[String]) -> av_core::transcript::TranscriptSummary {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("t.jsonl");
    fs::write(&p, lines.join("\n") + "\n").unwrap();
    summarize_transcript(&p).unwrap()
}

fn ts(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}

#[test]
fn ai_title_wins_over_prompt() {
    let s = run(&[user_prompt("fix login", T0), ai_title("Login bug fix")]);
    assert_eq!(s.title.as_deref(), Some("Login bug fix"));
}

#[test]
fn falls_back_to_first_prompt() {
    let s = run(&[
        user_prompt("<system-reminder>x</system-reminder>", T0),
        user_prompt("add dark mode\nmore", T0),
    ]);
    assert_eq!(s.title.as_deref(), Some("add dark mode"));
}

#[test]
fn long_prompt_cut_to_80() {
    let s = run(&[user_prompt(&"a".repeat(200), T0)]);
    assert_eq!(s.title.unwrap().chars().count(), 80);
}

#[test]
fn meta_and_tool_result_are_not_titles() {
    let s = run(&[user_meta("meta text", T0), tool_result("t1", T0)]);
    assert_eq!(s.title, None);
}

#[test]
fn open_tool_is_pending() {
    let s = run(&[assistant_tool_use("m1", "t1", "Bash", T0)]);
    let p = s.pending_tool.unwrap();
    assert_eq!(p.name, "Bash");
    assert_eq!(p.since, ts(T0));
}

#[test]
fn answered_tool_is_not_pending() {
    let s = run(&[
        assistant_tool_use("m1", "t1", "Bash", T0),
        tool_result("t1", "2026-10-29T13:40:00.000Z"),
    ]);
    assert!(s.pending_tool.is_none());
}

#[test]
fn tool_result_does_not_clear_other_open_tools() {
    let s = run(&[
        assistant_tool_use("m1", "t1", "Bash", T0),
        assistant_tool_use("m2", "t2", "Read", "2026-10-29T13:40:00.000Z"),
        tool_result("t2", "2026-10-29T13:40:01.000Z"),
    ]);
    assert_eq!(s.pending_tool.unwrap().name, "Bash");
}

#[test]
fn real_prompt_clears_open_tools() {
    let s = run(&[
        assistant_tool_use("m1", "t1", "Bash", T0),
        user_prompt("never mind", "2026-10-29T13:40:00.000Z"),
    ]);
    assert!(s.pending_tool.is_none());
}

#[test]
fn tokens_deduped_by_message_id() {
    let s = run(&[
        assistant_text("m1", "tool_use", 10, 5, T0),
        assistant_text("m1", "end_turn", 10, 5, T0),
        assistant_text("m2", "end_turn", 1, 1, T0),
    ]);
    assert_eq!(s.tokens.input, 11);
    assert_eq!(s.tokens.output, 6);
    assert_eq!(s.tokens.cache_read, 14);
    assert_eq!(s.tokens.cache_write, 6);
    assert_eq!(s.tokens.total(), 11 + 6 + 14 + 6);
    assert_eq!(s.last_stop_reason.as_deref(), Some("end_turn"));
}

#[test]
fn truncated_last_line_is_skipped() {
    let s = run(&[ai_title("T"), assistant_text("m1", "end_turn", 1, 1, T0), r#"{"type":"assist"#.to_string()]);
    assert_eq!(s.skipped_lines, 1);
    assert_eq!(s.title.as_deref(), Some("T"));
}

#[test]
fn invalid_utf8_line_does_not_abort() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("t.jsonl");
    let mut bytes = ai_title("T").into_bytes();
    bytes.push(b'\n');
    bytes.extend_from_slice(b"\xff\xfe\n");
    bytes.extend_from_slice(assistant_text("m1", "end_turn", 1, 1, T0).as_bytes());
    fs::write(&p, bytes).unwrap();
    let s = summarize_transcript(&p).unwrap();
    assert_eq!(s.skipped_lines, 1);
    assert_eq!(s.title.as_deref(), Some("T"));
    assert_eq!(s.tokens.input, 1);
}

#[test]
fn unknown_types_are_ignored() {
    let s = run(&[r#"{"type":"artifact-autoreact-ledger","x":1}"#.to_string()]);
    assert_eq!(s.skipped_lines, 0);
    assert!(s.last_activity.is_none());
}

#[test]
fn error_as_last_record() {
    let s = run(&[assistant_text("m1", "end_turn", 1, 1, T0), system_error(T0)]);
    assert!(s.last_is_error);
}

#[test]
fn api_error_as_last_record() {
    let s = run(&[assistant_text("m1", "end_turn", 1, 1, T0), assistant_api_error(T0)]);
    assert!(s.last_is_error);
    assert_eq!(s.model.as_deref(), Some("claude-opus-5-5"));
}

#[test]
fn error_followed_by_normal_record_is_not_error() {
    let s = run(&[system_error(T0), assistant_text("m1", "end_turn", 1, 1, T0)]);
    assert!(!s.last_is_error);
    let s = run(&[assistant_api_error(T0), assistant_text("m1", "end_turn", 1, 1, T0)]);
    assert!(!s.last_is_error);
}

#[test]
fn api_error_then_turn_duration_is_error() {
    let s = run(&[assistant_api_error(T0), system_turn_duration(T0)]);
    assert!(s.last_is_error);
}

#[test]
fn title_skips_blank_and_indented_system_prompts() {
    let s = run(&[
        user_prompt("\n  \n", T0),
        user_prompt("  <system-reminder>x", T0),
        user_prompt("add dark mode", T0),
    ]);
    assert_eq!(s.title.as_deref(), Some("add dark mode"));
}

#[test]
fn handback_detected() {
    let s = run(&[assistant_tool_use("m1", "t1", "SubagentHandback", T0)]);
    assert!(s.handed_back);
}

#[test]
fn branch_model_activity_from_latest() {
    let s = run(&[
        assistant_text("m1", "end_turn", 1, 1, T0),
        assistant_text("m2", "end_turn", 1, 1, "2026-10-29T14:00:00.000Z"),
    ]);
    assert_eq!(s.git_branch.as_deref(), Some("main"));
    assert_eq!(s.model.as_deref(), Some("claude-opus-5-5"));
    assert_eq!(s.last_activity, Some(ts("2026-10-29T14:00:00.000Z")));
}
