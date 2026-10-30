use av_core::hooks::events::*;
use av_core::status::HookSignal;
use chrono::{DateTime, Utc};
use std::fs::{self, OpenOptions};
use std::io::Write;

fn now() -> DateTime<Utc> {
    "2026-10-30T10:00:00.123Z".parse().unwrap()
}

fn notif(msg: &str) -> String {
    serde_json::json!({"session_id":"s1","hook_event_name":"Notification","message":msg,
        "notification_type":"permission_prompt"})
    .to_string()
}

#[test]
fn classify_by_type() {
    assert_eq!(
        classify_notification(Some("permission_prompt"), "x"),
        Some(HookSignal::Permission { tool: None })
    );
    assert_eq!(
        classify_notification(Some("idle_prompt"), "x"),
        Some(HookSignal::WaitingInput)
    );
    assert_eq!(
        classify_notification(Some("auth_success"), "needs your permission"),
        None
    );
}

#[test]
fn classify_by_message() {
    assert_eq!(
        classify_notification(None, "Claude needs your permission to use Bash"),
        Some(HookSignal::Permission {
            tool: Some("Bash".into())
        })
    );
    assert_eq!(
        classify_notification(None, "Claude is waiting for your input"),
        Some(HookSignal::WaitingInput)
    );
    assert_eq!(classify_notification(None, "hello"), None);
}

#[test]
fn write_then_parse_roundtrip() {
    let d = tempfile::tempdir().unwrap();
    let p = events_path(d.path());
    write_event(
        &p,
        &notif("Claude needs your permission to use Bash"),
        now(),
    )
    .unwrap();
    let mut t = EventsTail::new(p);
    let ev = t.read_new().unwrap();
    assert_eq!(ev.len(), 1);
    assert_eq!(ev[0].session_id, "s1");
    assert_eq!(ev[0].received_at, now());
    assert_eq!(
        ev[0].kind,
        HookKind::Notification(HookSignal::Permission { tool: None })
    );
}

#[test]
fn stop_parses() {
    let d = tempfile::tempdir().unwrap();
    let p = events_path(d.path());
    write_event(
        &p,
        r#"{"session_id":"s2","hook_event_name":"Stop","stop_hook_active":false}"#,
        now(),
    )
    .unwrap();
    let ev = EventsTail::new(p).read_new().unwrap();
    assert_eq!(ev[0].kind, HookKind::Stop);
}

#[test]
fn oversized_message_is_truncated() {
    let d = tempfile::tempdir().unwrap();
    let p = events_path(d.path());
    let msg = "é".repeat(10_000);
    write_event(&p, &notif(&msg), now()).unwrap();
    let raw = fs::read(&p).unwrap();
    assert!(raw.len() <= 4096, "{}", raw.len());
    assert_eq!(raw.last(), Some(&b'\n'));
    let line = String::from_utf8(raw).unwrap();
    assert!(parse_event_line(line.trim()).is_some());
}

#[test]
fn junk_stdin_still_writes() {
    let d = tempfile::tempdir().unwrap();
    let p = events_path(d.path());
    write_event(&p, "not json", now()).unwrap();
    let s = fs::read_to_string(&p).unwrap();
    assert_eq!(s.lines().count(), 1);
    assert!(s.contains("\"raw\""));
    assert!(parse_event_line(s.trim()).is_none());
}

#[test]
fn partial_line_is_carried() {
    let d = tempfile::tempdir().unwrap();
    let p = events_path(d.path());
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    let full = format!(
        "{}\n",
        serde_json::json!({"session_id":"s3","hook_event_name":"Stop","receivedAt":"2026-10-30T10:00:00Z"})
    );
    let (a, b) = full.as_bytes().split_at(20);
    let mut f = OpenOptions::new()
        .append(true)
        .create(true)
        .open(&p)
        .unwrap();
    f.write_all(a).unwrap();
    let mut t = EventsTail::new(p.clone());
    assert!(t.read_new().unwrap().is_empty());
    f.write_all(b).unwrap();
    assert_eq!(t.read_new().unwrap().len(), 1);
    assert!(t.read_new().unwrap().is_empty());
}

#[test]
fn trim_keeps_whole_lines() {
    let d = tempfile::tempdir().unwrap();
    let p = events_path(d.path());
    for i in 0..3000 {
        write_event(
            &p,
            &format!(r#"{{"session_id":"s{i}","hook_event_name":"Stop"}}"#),
            now(),
        )
        .unwrap();
    }
    trim_events_file(&p, 1024).unwrap();
    let s = fs::read_to_string(&p).unwrap();
    assert!(s.len() <= 1024 && !s.is_empty());
    for l in s.lines() {
        assert!(parse_event_line(l).is_some(), "{l}");
    }
}
