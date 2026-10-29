mod common;

use av_core::registry::read_registry;
use common::write_session_at;
use std::fs;

#[test]
fn reads_entries_newest_first() {
    let t = tempfile::tempdir().unwrap();
    write_session_at(t.path(), 1, "s-old", "/a", "idle", 1000);
    write_session_at(t.path(), 2, "s-new", "/b", "busy", 2000);
    let (entries, warnings) = read_registry(t.path());
    assert!(warnings.is_empty());
    let starts: Vec<i64> = entries.iter().map(|e| e.started_at_ms).collect();
    assert_eq!(starts, vec![2000, 1000]);
    assert_eq!(entries[0].pid, 2);
    assert_eq!(entries[0].session_id, "s-new");
    assert_eq!(entries[0].cwd, "/b");
    assert_eq!(entries[0].status, "busy");
    assert_eq!(entries[0].kind, "interactive");
}

#[test]
fn bad_file_becomes_warning() {
    let t = tempfile::tempdir().unwrap();
    fs::create_dir_all(t.path().join("sessions")).unwrap();
    fs::write(t.path().join("sessions/999.json"), "{not json").unwrap();
    let (entries, warnings) = read_registry(t.path());
    assert!(entries.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("999.json"));
}

#[test]
fn missing_sessions_dir_is_empty() {
    let t = tempfile::tempdir().unwrap();
    let (entries, warnings) = read_registry(t.path());
    assert!(entries.is_empty());
    assert!(warnings.is_empty());
}

#[test]
fn unknown_fields_are_ignored() {
    let t = tempfile::tempdir().unwrap();
    fs::create_dir_all(t.path().join("sessions")).unwrap();
    fs::write(
        t.path().join("sessions/5.json"),
        r#"{"pid":5,"sessionId":"s","cwd":"/x","startedAt":1,"peerFeatures":["x"],"jobId":"j"}"#,
    )
    .unwrap();
    fs::write(t.path().join("sessions/5.abc.key"), "secret").unwrap();
    let (entries, warnings) = read_registry(t.path());
    assert!(warnings.is_empty());
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "");
}
