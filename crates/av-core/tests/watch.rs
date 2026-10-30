use av_core::engine::Input;
use av_core::watch::{classify_path, rebase_event_path, spawn_watcher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn classify_cases() {
    let c = Path::new("/h/.claude");
    assert_eq!(
        classify_path(c, Path::new("/h/.claude/sessions/1.json")),
        Some(Input::Registry)
    );
    assert_eq!(
        classify_path(c, Path::new("/h/.claude/projects/-x/s.jsonl")),
        Some(Input::Transcript(PathBuf::from(
            "/h/.claude/projects/-x/s.jsonl"
        )))
    );
    assert_eq!(
        classify_path(
            c,
            Path::new("/h/.claude/projects/-x/s/subagents/agent-a.jsonl")
        ),
        Some(Input::Transcript(PathBuf::from(
            "/h/.claude/projects/-x/s/subagents/agent-a.jsonl"
        )))
    );
    assert_eq!(
        classify_path(c, Path::new("/h/.claude/agent-viewer/events.jsonl")),
        Some(Input::Events)
    );
    assert_eq!(
        classify_path(c, Path::new("/h/.claude/projects/-x/agent-a.meta.json")),
        None
    );
    assert_eq!(
        classify_path(c, Path::new("/h/.claude/settings.json")),
        None
    );
    assert_eq!(
        classify_path(c, Path::new("/elsewhere/sessions/1.json")),
        None
    );
}

#[test]
fn rebase_cases() {
    let given = Path::new("/var/x/.claude");
    let canon = Path::new("/private/var/x/.claude");
    assert_eq!(
        rebase_event_path(
            given,
            canon,
            Path::new("/private/var/x/.claude/projects/a.jsonl")
        ),
        PathBuf::from("/var/x/.claude/projects/a.jsonl")
    );
    assert_eq!(
        rebase_event_path(given, canon, Path::new("/var/x/.claude/projects/a.jsonl")),
        PathBuf::from("/var/x/.claude/projects/a.jsonl")
    );
    assert_eq!(
        rebase_event_path(given, canon, Path::new("/other/a")),
        PathBuf::from("/other/a")
    );
}

#[test]
fn watcher_missing_dir_errors() {
    let (tx, _rx) = mpsc::channel();
    assert!(spawn_watcher(Path::new("/definitely/not/here/.claude"), tx).is_err());
}

#[test]
fn watcher_sees_append() {
    let tmp = tempfile::tempdir().unwrap();
    let claude = tmp.path().join(".claude");
    std::fs::create_dir_all(claude.join("projects/-x")).unwrap();
    let (tx, rx) = mpsc::channel();
    let _h = spawn_watcher(&claude, tx).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let p = claude.join("projects/-x/s.jsonl");
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&p)
        .unwrap();
    writeln!(f, "{{}}").unwrap();
    f.sync_all().unwrap();
    let got = rx.recv_timeout(Duration::from_secs(2)).expect("event");
    assert_eq!(got, Input::Transcript(p));
}
