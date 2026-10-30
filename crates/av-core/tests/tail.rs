mod common;
use av_core::transcript::{summarize_transcript, TranscriptTail};
use chrono::{DateTime, Utc};
use common::*;
use std::fs::{self, OpenOptions};
use std::io::Write;

const T1: &str = "2026-10-29T13:40:10.000Z";
const T2: &str = "2026-10-29T13:40:20.000Z";

fn ts(s: &str) -> DateTime<Utc> {
    s.parse().unwrap()
}

fn append(p: &std::path::Path, bytes: &[u8]) {
    let mut f = OpenOptions::new().append(true).create(true).open(p).unwrap();
    f.write_all(bytes).unwrap();
}

fn write_lines(p: &std::path::Path, lines: &[String]) {
    fs::write(p, lines.join("\n") + "\n").unwrap();
}

#[test]
fn chunked_appends_equal_full_read() {
    let lines = vec![
        user_prompt("fix login", T0),
        agent_name("api contract"),
        assistant_tool_use("m1", "t1", "Bash", T0),
        tool_result("t1", T1),
        assistant_tool_use("m2", "t2", "Read", T1),
        assistant_text("m3", "end_turn", 5, 6, T1),
        system_turn_duration(T2),
        assistant_api_error_kind("authentication_failed", T2),
        user_prompt("again", T2),
        assistant_text_cached("m4", 10, 2, 20, 170, "claude-opus-5-5", T2),
        assistant_api_error(T2),
        ai_title("Login é bug"),
    ];
    let bytes = (lines.join("\n") + "\n").into_bytes();
    let dir = tempfile::tempdir().unwrap();
    let full = dir.path().join("full.jsonl");
    fs::write(&full, &bytes).unwrap();
    let expected = summarize_transcript(&full).unwrap();

    let mut k = 0;
    while k <= bytes.len() {
        let p = dir.path().join(format!("c{k}.jsonl"));
        fs::write(&p, &bytes[..k]).unwrap();
        let mut tail = TranscriptTail::new(p.clone());
        tail.read_new().unwrap();
        append(&p, &bytes[k..]);
        tail.read_new().unwrap();
        assert_eq!(tail.summary(), expected, "split at {k}");
        assert_eq!(tail.offset(), bytes.len() as u64);
        k += 7;
    }
}

#[test]
fn half_line_is_carried() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("t.jsonl");
    fs::write(&p, ai_title("Half")).unwrap();
    let mut tail = TranscriptTail::new(p.clone());
    assert!(!tail.read_new().unwrap());
    assert_eq!(tail.summary().title, None);
    append(&p, b"\n");
    assert!(tail.read_new().unwrap());
    assert_eq!(tail.summary().title.as_deref(), Some("Half"));
}

#[test]
fn truncation_resets() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("t.jsonl");
    write_lines(&p, &[ai_title("A long first title here"), user_prompt("x", T0)]);
    let mut tail = TranscriptTail::new(p.clone());
    tail.read_new().unwrap();
    write_lines(&p, &[ai_title("B")]);
    assert!(tail.read_new().unwrap());
    assert_eq!(tail.summary(), summarize_transcript(&p).unwrap());
    assert_eq!(tail.summary().title.as_deref(), Some("B"));
}

#[test]
fn replacement_resets() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("t.jsonl");
    write_lines(&p, &[ai_title("AAAA")]);
    let mut tail = TranscriptTail::new(p.clone());
    tail.read_new().unwrap();
    // Keep the old inode alive so the new file cannot reuse it.
    let held = dir.path().join("old.jsonl");
    fs::rename(&p, &held).unwrap();
    write_lines(&p, &[ai_title("BBBB")]);
    assert!(tail.read_new().unwrap());
    assert_eq!(tail.summary().title.as_deref(), Some("BBBB"));
    assert_eq!(tail.summary(), summarize_transcript(&p).unwrap());
}

#[test]
fn context_used_is_last_call() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("t.jsonl");
    write_lines(
        &p,
        &[
            assistant_text_cached("a", 10, 1, 20, 170, "claude-opus-5-5", T0),
            assistant_text_cached("b", 5, 1, 0, 300, "claude-opus-5-5", T1),
        ],
    );
    let s = summarize_transcript(&p).unwrap();
    assert_eq!(s.context_used, Some(305));
    assert_eq!(s.max_context_used, 305);
}

#[test]
fn agent_name_and_turn_end() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("t.jsonl");
    write_lines(&p, &[agent_name("api contract"), system_turn_duration(T2)]);
    let s = summarize_transcript(&p).unwrap();
    assert_eq!(s.agent_name.as_deref(), Some("api contract"));
    assert_eq!(s.turn_ended_at, Some(ts(T2)));
}

#[test]
fn error_kind() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("t.jsonl");
    write_lines(&p, &[assistant_api_error_kind("authentication_failed", T0)]);
    let s = summarize_transcript(&p).unwrap();
    assert!(s.last_is_error);
    assert_eq!(s.last_error_kind.as_deref(), Some("authentication_failed"));
    append(&p, (user_prompt("retry", T1) + "\n").as_bytes());
    let s = summarize_transcript(&p).unwrap();
    assert!(!s.last_is_error);
    assert_eq!(s.last_error_kind, None);
}
