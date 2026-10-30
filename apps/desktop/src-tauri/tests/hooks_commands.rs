//! Hook commands against a temp claude dir (never the real ~/.claude).

#[path = "../../../../crates/av-core/tests/common/mod.rs"]
mod common;

use av_core::hooks::install::HooksStatus;
use av_core::model::Status;
use av_core::paths::encode_project_dir;
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use common::*;
use desktop_lib::runtime::Runtime;
use desktop_lib::settings::AppSettings;
use std::fs;
use std::path::{Path, PathBuf};

const CWD: &str = "/work/proj";

fn t0() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(T0)
        .unwrap()
        .with_timezone(&Utc)
}
fn at(s: i64) -> DateTime<Utc> {
    t0() + Duration::seconds(s)
}
fn ts(s: i64) -> String {
    at(s).to_rfc3339_opts(SecondsFormat::Millis, true)
}

struct Fx {
    _d: tempfile::TempDir,
    home: PathBuf,
    av: PathBuf,
    probe: SharedProbe,
    rt: Runtime,
}

fn fx(av_content: Option<&[u8]>) -> Fx {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join(".claude");
    fs::create_dir_all(&home).unwrap();
    let av = d.path().join("av");
    if let Some(c) = av_content {
        fs::write(&av, c).unwrap();
    }
    let probe = SharedProbe::default();
    let rt = Runtime::new(
        home.clone(),
        d.path().join("appdata"),
        AppSettings::default(),
        Box::new(probe.clone()),
    )
    .with_av_path(av.clone());
    // A busy session with a Bash tool open.
    write_session(&home, 100, "a", CWD, "busy");
    probe.set_running(100, true);
    let t: &Path = &home
        .join("projects")
        .join(encode_project_dir(CWD))
        .join("a.jsonl");
    fs::create_dir_all(t.parent().unwrap()).unwrap();
    fs::write(
        t,
        format!(
            "{}\n{}\n",
            user_prompt("hi", &ts(0)),
            assistant_tool_use("m1", "tu1", "Bash", &ts(1))
        ),
    )
    .unwrap();
    Fx {
        _d: d,
        home,
        av,
        probe,
        rt,
    }
}

fn status(rt: &Runtime) -> Status {
    rt.list_agents()[0].status.clone()
}

#[test]
fn install_writes_entries_and_backup() {
    let f = fx(Some(b"bin"));
    let settings = f.home.join("settings.json");
    fs::write(&settings, r#"{"theme":"dark"}"#).unwrap();
    f.rt.init(at(2));
    f.rt.apply(desktop_lib_tick(), at(8));
    assert_eq!(status(&f.rt), Status::MaybeWaiting);

    let report = f.rt.install_hooks(at(9)).unwrap();
    assert!(report.changed);
    let backup = PathBuf::from(report.backup.expect("backup"));
    assert!(backup.exists());
    let text = fs::read_to_string(&settings).unwrap();
    assert_eq!(text.matches("--agent-viewer").count(), 2);
    assert!(text.contains("\"theme\""));
    assert_eq!(f.rt.hooks_status(), HooksStatus::Installed);
    // R25: the session was already running, so it never loaded the hooks;
    // the heuristic stays on for it.
    assert_eq!(status(&f.rt), Status::MaybeWaiting);
}

/// Without a loop attached, install still updates the engine's hooks state:
/// a session started after the install counts as hooked.
#[test]
fn install_updates_engine_hooks_state() {
    let f = fx(Some(b"bin"));
    f.rt.init(at(2));
    f.rt.install_hooks(at(3)).unwrap();
    new_session(&f, 200, "b", at(5));
    f.rt.apply(av_core::engine::Input::Registry, at(6));
    f.rt.apply(desktop_lib_tick(), at(14));
    let b =
        f.rt.list_agents()
            .into_iter()
            .find(|a| a.id == "b")
            .unwrap();
    assert_eq!(b.status, Status::Working);
}

/// With the loop attached, install/uninstall don't touch the engine on the
/// calling thread: they send the new status to the loop, in order.
#[test]
fn install_routes_through_loop_channel() {
    let f = fx(Some(b"bin"));
    let settings = f.home.join("settings.json");
    fs::write(&settings, r#"{"theme":"dark"}"#).unwrap();
    f.rt.init(at(2));
    let (tx, rx) = std::sync::mpsc::channel();
    f.rt.attach_loop(tx);
    f.rt.install_hooks(at(3)).unwrap();
    f.rt.uninstall_hooks(at(4)).unwrap();
    f.rt.install_hooks(at(5)).unwrap();
    let got: Vec<_> = rx.try_iter().collect();
    use av_core::engine::Input::HooksInstalled;
    assert_eq!(
        got,
        vec![
            HooksInstalled(true),
            HooksInstalled(false),
            HooksInstalled(true)
        ]
    );
    // The loop applies it: a session started after the install is hooked.
    f.rt.apply(got[2].clone(), at(5));
    new_session(&f, 200, "b", at(6));
    f.rt.apply(av_core::engine::Input::Registry, at(7));
    f.rt.apply(desktop_lib_tick(), at(15));
    let b =
        f.rt.list_agents()
            .into_iter()
            .find(|a| a.id == "b")
            .unwrap();
    assert_eq!(b.status, Status::Working);
}

/// A busy session `sid` started at `started` with a Bash tool open.
fn new_session(f: &Fx, pid: u32, sid: &str, started: DateTime<Utc>) {
    let home = &f.home;
    f.probe.set_running(pid, true);
    write_session_at(home, pid, sid, CWD, "busy", started.timestamp_millis());
    let t = home
        .join("projects")
        .join(encode_project_dir(CWD))
        .join(format!("{sid}.jsonl"));
    fs::write(
        &t,
        format!(
            "{}\n{}\n",
            user_prompt("hi", &ts(5)),
            assistant_tool_use("m1", "tu1", "Bash", &ts(6))
        ),
    )
    .unwrap();
}

fn desktop_lib_tick() -> av_core::engine::Input {
    av_core::engine::Input::Tick
}

#[test]
fn uninstall_removes_entries() {
    let f = fx(Some(b"bin"));
    let settings = f.home.join("settings.json");
    fs::write(&settings, r#"{"theme":"dark"}"#).unwrap();
    f.rt.init(at(2));
    f.rt.install_hooks(at(3)).unwrap();
    assert_eq!(status(&f.rt), Status::Working);

    let report = f.rt.uninstall_hooks(at(10)).unwrap();
    assert!(report.changed);
    assert!(
        !fs::read_to_string(&settings)
            .unwrap()
            .contains("--agent-viewer")
    );
    assert_eq!(f.rt.hooks_status(), HooksStatus::NotInstalled);
    assert_eq!(status(&f.rt), Status::MaybeWaiting);
}

#[test]
fn invalid_settings_code() {
    let f = fx(Some(b"bin"));
    let settings = f.home.join("settings.json");
    fs::write(&settings, "{ not json").unwrap();
    let e = f.rt.install_hooks(at(3)).unwrap_err();
    assert_eq!(e.code, "invalidSettings");
    assert_eq!(fs::read(&settings).unwrap(), b"{ not json");
    let e = f.rt.uninstall_hooks(at(3)).unwrap_err();
    assert_eq!(e.code, "invalidSettings");
    assert_eq!(fs::read(&settings).unwrap(), b"{ not json");
}

#[test]
fn sidecar_missing_refused() {
    // Missing av.
    let f = fx(None);
    let e = f.rt.install_hooks(at(3)).unwrap_err();
    assert_eq!(e.code, "sidecarMissing");
    assert!(!f.home.join("settings.json").exists());

    // Zero-byte av, existing settings untouched.
    let f = fx(Some(b""));
    let settings = f.home.join("settings.json");
    fs::write(&settings, r#"{"a":1}"#).unwrap();
    let e = f.rt.install_hooks(at(3)).unwrap_err();
    assert_eq!(e.code, "sidecarMissing");
    assert_eq!(fs::read(&settings).unwrap(), br#"{"a":1}"#);
    assert!(f.av.exists());
}

#[test]
fn concurrent_installs_yield_one_marker_per_event() {
    let f = fx(Some(b"bin"));
    let settings = f.home.join("settings.json");
    fs::write(&settings, r#"{"theme":"dark"}"#).unwrap();
    f.rt.init(at(2));
    std::thread::scope(|s| {
        for _ in 0..2 {
            s.spawn(|| {
                f.rt.install_hooks(at(9)).unwrap();
            });
        }
    });
    let text = fs::read_to_string(&settings).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).expect("valid json");
    assert_eq!(text.matches("--agent-viewer").count(), 2);
    for (_, arr) in v["hooks"].as_object().unwrap() {
        assert_eq!(arr.as_array().unwrap().len(), 1);
    }
}

/// A non-default Claude folder: the hook command carries `--claude-dir` so
/// `av hook` writes events where the app reads them.
#[test]
fn install_with_override_writes_claude_dir_flag() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    let claude = d.path().join("custom claude");
    fs::create_dir_all(&claude).unwrap();
    let av = d.path().join("av");
    fs::write(&av, b"bin").unwrap();
    let rt = Runtime::new_in_home(
        claude.clone(),
        d.path().join("appdata"),
        AppSettings::default(),
        Box::new(SharedProbe::default()),
        Some(&home),
    )
    .with_av_path(av.clone());
    rt.install_hooks(at(1)).unwrap();
    let text = fs::read_to_string(claude.join("settings.json")).unwrap();
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    for ev in ["Notification", "Stop"] {
        let cmd = v["hooks"][ev][0]["hooks"][0]["command"].as_str().unwrap();
        assert_eq!(
            cmd,
            format!(
                "\"{}\" hook --agent-viewer --claude-dir \"{}\"",
                av.display(),
                claude.display()
            )
        );
    }
    assert_eq!(rt.hooks_status(), HooksStatus::Installed);
}

/// The default `$HOME/.claude`: no `--claude-dir` flag.
#[test]
fn install_default_dir_has_no_claude_dir_flag() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    let claude = home.join(".claude");
    fs::create_dir_all(&claude).unwrap();
    let av = d.path().join("av");
    fs::write(&av, b"bin").unwrap();
    let rt = Runtime::new_in_home(
        claude.clone(),
        d.path().join("appdata"),
        AppSettings::default(),
        Box::new(SharedProbe::default()),
        Some(&home),
    )
    .with_av_path(av.clone());
    rt.install_hooks(at(1)).unwrap();
    let text = fs::read_to_string(claude.join("settings.json")).unwrap();
    assert!(!text.contains("--claude-dir"));
    assert_eq!(text.matches("--agent-viewer").count(), 2);
    assert_eq!(rt.hooks_status(), HooksStatus::Installed);
}
