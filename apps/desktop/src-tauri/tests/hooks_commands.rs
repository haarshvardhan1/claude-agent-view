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

    let (report, (changes, _)) = f.rt.install_hooks(at(9)).unwrap();
    assert!(report.changed);
    let backup = PathBuf::from(report.backup.expect("backup"));
    assert!(backup.exists());
    let text = fs::read_to_string(&settings).unwrap();
    assert_eq!(text.matches("--agent-viewer").count(), 2);
    assert!(text.contains("\"theme\""));
    assert_eq!(f.rt.hooks_status(), HooksStatus::Installed);
    assert!(!changes.is_empty());
    assert_eq!(status(&f.rt), Status::Working);
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

    let (report, _) = f.rt.uninstall_hooks(at(10)).unwrap();
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
