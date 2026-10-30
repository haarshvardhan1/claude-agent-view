//! Command logic against a temp claude dir (no Tauri window needed).

#[path = "../../../../crates/av-core/tests/common/mod.rs"]
mod common;

use av_core::engine::Input;
use av_core::hooks::events::events_path;
use av_core::model::{Change, TranscriptEntry};
use av_core::paths::encode_project_dir;
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use common::*;
use desktop_lib::commands::{CmdError, PageReq};
use desktop_lib::runtime::Runtime;
use desktop_lib::settings::{self, AppSettings};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const CWD: &str = "/work/proj";

fn t0() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(T0)
        .unwrap()
        .with_timezone(&Utc)
}

fn ts(secs: i64) -> String {
    (t0() + Duration::seconds(secs)).to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn at(secs: i64) -> DateTime<Utc> {
    t0() + Duration::seconds(secs)
}

fn transcript(home: &Path, sid: &str) -> PathBuf {
    home.join("projects")
        .join(encode_project_dir(CWD))
        .join(format!("{sid}.jsonl"))
}

/// A live session `sid` at `pid` with a one-prompt transcript.
fn live_session(home: &Path, probe: &SharedProbe, pid: u32, sid: &str) -> PathBuf {
    write_session(home, pid, sid, CWD, "busy");
    probe.set_running(pid, true);
    let t = transcript(home, sid);
    fs::create_dir_all(t.parent().unwrap()).unwrap();
    fs::write(&t, user_prompt("hello", &ts(0)) + "\n").unwrap();
    t
}

fn runtime(home: &Path, settings_dir: &Path, probe: &SharedProbe) -> Runtime {
    Runtime::new(
        home.to_path_buf(),
        settings_dir.to_path_buf(),
        AppSettings::default(),
        Box::new(probe.clone()),
    )
}

#[test]
fn list_agents_and_get_transcript() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join(".claude");
    let sdir = d.path().join("appdata");
    let probe = SharedProbe::default();
    let t = live_session(&home, &probe, 100, "a");
    let rt = runtime(&home, &sdir, &probe);

    let changes = rt.init(at(1));
    assert!(matches!(changes.as_slice(), [Change::AgentAdded { agent }] if agent.id == "a"));
    let ids: Vec<_> = rt.list_agents().into_iter().map(|a| a.id).collect();
    assert_eq!(ids, ["a"]);

    let page = rt.get_transcript("a", None, PageReq::Tail).unwrap();
    assert_eq!(page.entries.len(), 1);
    assert!(matches!(
        &page.entries[0],
        TranscriptEntry::UserPrompt { .. }
    ));
    let len = fs::metadata(&t).unwrap().len();
    assert_eq!(page.end_offset, len);

    // Growth: engine reports an update, `After` returns only the new record.
    let mut f = fs::OpenOptions::new().append(true).open(&t).unwrap();
    writeln!(f, "{}", assistant_text("m1", "end_turn", 10, 5, &ts(2))).unwrap();
    let changes = rt.apply(Input::Transcript(t.clone()), at(3));
    assert!(matches!(changes.as_slice(), [Change::AgentUpdated { .. }]));
    let next = rt
        .get_transcript(
            "a",
            None,
            PageReq::After {
                offset: page.end_offset,
            },
        )
        .unwrap();
    assert_eq!(next.start_offset, page.end_offset);
    assert_eq!(next.entries.len(), 1);
}

#[test]
fn unknown_agent_is_not_found() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join(".claude");
    let probe = SharedProbe::default();
    live_session(&home, &probe, 100, "a");
    let rt = runtime(&home, &d.path().join("appdata"), &probe);
    rt.init(at(1));

    let e = rt.get_transcript("nope", None, PageReq::Tail).unwrap_err();
    assert_eq!(e.code, "notFound");
    let e = rt
        .get_transcript("a", Some("zzz"), PageReq::Tail)
        .unwrap_err();
    assert_eq!(e.code, "notFound");
}

#[test]
fn page_req_and_error_wire_format() {
    let r: PageReq = serde_json::from_str(r#"{"kind":"tail"}"#).unwrap();
    assert_eq!(r, PageReq::Tail);
    let r: PageReq = serde_json::from_str(r#"{"kind":"before","offset":7}"#).unwrap();
    assert_eq!(r, PageReq::Before { offset: 7 });
    let r: PageReq = serde_json::from_str(r#"{"kind":"after","offset":9}"#).unwrap();
    assert_eq!(r, PageReq::After { offset: 9 });

    let e = serde_json::to_value(CmdError::new("notFound", "x")).unwrap();
    assert_eq!(e, serde_json::json!({"code": "notFound", "message": "x"}));
}

#[test]
fn settings_selected_agent_and_watcher_mode() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join(".claude");
    let sdir = d.path().join("appdata");
    let probe = SharedProbe::default();
    fs::create_dir_all(&home).unwrap();
    let rt = runtime(&home, &sdir, &probe);

    assert_eq!(rt.watcher_mode(), "watching");
    rt.set_polling(true);
    assert_eq!(rt.watcher_mode(), "polling");

    let mut s = AppSettings::default();
    s.notify.error = false;
    rt.set_settings(s.clone()).unwrap();
    assert_eq!(rt.settings(), s);
    assert_eq!(settings::load(&sdir), s);

    rt.set_selected_agent(Some("a".into()));
    assert_eq!(rt.selected_agent().as_deref(), Some("a"));
    rt.set_selected_agent(None);
    assert_eq!(rt.selected_agent(), None);
}

#[test]
fn init_never_creates_files_in_claude_dir() {
    let d = tempfile::tempdir().unwrap();
    let probe = SharedProbe::default();

    // Existing claude dir without agent-viewer/: trimming must not create it.
    let home = d.path().join(".claude");
    live_session(&home, &probe, 100, "a");
    runtime(&home, &d.path().join("appdata"), &probe).init(at(1));
    assert!(!home.join("agent-viewer").exists());

    // Missing claude dir: stays missing.
    let missing = d.path().join("missing").join(".claude");
    let rt = runtime(&missing, &d.path().join("appdata"), &probe);
    assert!(rt.init(at(1)).is_empty());
    assert!(rt.list_agents().is_empty());
    assert!(!missing.exists());
}

#[test]
fn init_trims_events_file_to_1mb() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join(".claude");
    let probe = SharedProbe::default();
    let ev = events_path(&home);
    fs::create_dir_all(ev.parent().unwrap()).unwrap();
    let line = format!("{{\"pad\":\"{}\"}}\n", "x".repeat(1000));
    fs::write(&ev, line.repeat(1500)).unwrap();
    assert!(fs::metadata(&ev).unwrap().len() > 1_048_576);

    runtime(&home, &d.path().join("appdata"), &probe).init(at(1));
    assert!(fs::metadata(&ev).unwrap().len() <= 1_048_576);
}

#[test]
fn claude_dir_from_settings_or_home() {
    let mut s = AppSettings::default();
    let home = Some(PathBuf::from("/Users/x"));
    assert_eq!(
        settings::resolve_claude_dir(&s, home.clone()),
        PathBuf::from("/Users/x/.claude")
    );
    s.claude_dir = Some("/data/claude".into());
    assert_eq!(
        settings::resolve_claude_dir(&s, home),
        PathBuf::from("/data/claude")
    );
}

#[test]
fn try_watch_polls_until_claude_dir_exists() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join(".claude");
    let probe = SharedProbe::default();
    let rt = runtime(&home, &d.path().join("appdata"), &probe);
    let (tx, _rx) = std::sync::mpsc::channel();

    assert!(desktop_lib::runtime::try_watch(&rt, &tx).is_none());
    assert_eq!(rt.watcher_mode(), "polling");
    assert!(!home.exists(), "must not create the claude dir");

    fs::create_dir_all(&home).unwrap();
    let handle = desktop_lib::runtime::try_watch(&rt, &tx);
    assert!(handle.is_some());
    assert_eq!(rt.watcher_mode(), "watching");
}

/// Error status after init: one notification, unless the main window is
/// focused and visible on that agent.
fn error_notifications(focused: bool, visible: bool) -> usize {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join(".claude");
    let probe = SharedProbe::default();
    let t = live_session(&home, &probe, 100, "a");
    let rt = runtime(&home, &d.path().join("appdata"), &probe);
    rt.init(at(1));
    rt.set_selected_agent(Some("a".into()));
    rt.set_main_focused(focused);
    rt.set_main_visible(visible);

    let mut f = fs::OpenOptions::new().append(true).open(&t).unwrap();
    writeln!(f, "{}", assistant_api_error(&ts(2))).unwrap();
    let (changes, notes) = rt.step(Input::Transcript(t.clone()), at(3));
    assert!(!changes.is_empty());
    assert!(notes.iter().all(|n| n.agent_id == "a"));
    notes.len()
}

#[test]
fn step_notifies_unless_focused_on_agent() {
    assert_eq!(error_notifications(false, true), 1);
    assert_eq!(error_notifications(true, false), 1);
    assert_eq!(error_notifications(true, true), 0);
}

#[test]
fn env_override_wins_over_settings_and_home() {
    let mut s = AppSettings {
        claude_dir: Some("/data/claude".into()),
        ..AppSettings::default()
    };
    let home = Some(PathBuf::from("/Users/x"));
    assert_eq!(
        settings::resolve_claude_dir_with_override(&s, Some("/tmp/fixture".into()), home.clone()),
        PathBuf::from("/tmp/fixture")
    );
    // Unset or blank falls through to settings, then $HOME/.claude.
    assert_eq!(
        settings::resolve_claude_dir_with_override(&s, Some("  ".into()), home.clone()),
        PathBuf::from("/data/claude")
    );
    s.claude_dir = None;
    assert_eq!(
        settings::resolve_claude_dir_with_override(&s, None, home),
        PathBuf::from("/Users/x/.claude")
    );
}

#[test]
fn change_log_line_has_epoch_ms_and_change_kinds() {
    use chrono::TimeZone;
    let now = chrono::Utc.timestamp_millis_opt(1_793_281_174_333).unwrap();
    let changes = vec![av_core::model::Change::AgentRemoved { id: "s1".into() }];
    assert_eq!(
        desktop_lib::runtime::change_log_line(now, &changes),
        "agent-viewer: changes t=1793281174333 removed:s1"
    );
}
