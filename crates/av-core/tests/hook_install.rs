use av_core::hooks::install::*;
use chrono::{DateTime, TimeZone, Utc};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

fn t(s: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 30, 12, 0, s).unwrap()
}

fn fake_av(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    fs::write(&p, "#!/bin/sh\n").unwrap();
    p
}

fn backups(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| {
            p.file_name()
                .unwrap()
                .to_string_lossy()
                .contains(".agent-viewer-backup-")
        })
        .collect();
    v.sort();
    v
}

fn marker_count(v: &Value, event: &str) -> usize {
    v["hooks"][event]
        .as_array()
        .map(|a| {
            a.iter()
                .flat_map(|g| g["hooks"].as_array().cloned().unwrap_or_default())
                .filter(|h| h["command"].as_str().is_some_and(|c| c.contains(MARKER)))
                .count()
        })
        .unwrap_or(0)
}

#[test]
fn install_into_missing_file() {
    let d = tempfile::tempdir().unwrap();
    let s = d.path().join("settings.json");
    let av = fake_av(d.path(), "av");
    let r = install(&s, &av, t(0)).unwrap();
    assert!(r.changed);
    assert!(r.backup.is_none());
    assert!(s.exists());
    assert_eq!(hooks_status(&s), HooksStatus::Installed);
    assert!(backups(d.path()).is_empty());
}

#[test]
fn install_keeps_existing() {
    let d = tempfile::tempdir().unwrap();
    let s = d.path().join("settings.json");
    let orig = r#"{"model":"x","hooks":{"Stop":[{"matcher":"","hooks":[{"type":"command","command":"say hi"}]}]},"zeta":1}"#;
    fs::write(&s, orig).unwrap();
    let av = fake_av(d.path(), "av");
    let r = install(&s, &av, t(1)).unwrap();
    let text = fs::read_to_string(&s).unwrap();
    assert!(text.contains("say hi"));
    let v: Value = serde_json::from_str(&text).unwrap();
    let keys: Vec<&String> = v.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["model", "hooks", "zeta"]);
    let b = r.backup.unwrap();
    assert_eq!(fs::read(&b).unwrap(), orig.as_bytes());
    assert_eq!(
        b.file_name().unwrap().to_str().unwrap(),
        "settings.json.agent-viewer-backup-20261030-120001"
    );
    assert_eq!(marker_count(&v, "Stop"), 1);
    assert_eq!(marker_count(&v, "Notification"), 1);
}

#[test]
fn install_twice_is_noop() {
    let d = tempfile::tempdir().unwrap();
    let s = d.path().join("settings.json");
    fs::write(&s, r#"{"a":1}"#).unwrap();
    let av = fake_av(d.path(), "av");
    assert!(install(&s, &av, t(2)).unwrap().changed);
    let bytes = fs::read(&s).unwrap();
    let r = install(&s, &av, t(3)).unwrap();
    assert!(!r.changed);
    assert!(r.backup.is_none());
    assert_eq!(fs::read(&s).unwrap(), bytes);
    assert_eq!(backups(d.path()).len(), 1);
}

#[test]
fn uninstall_restores_meaning() {
    let d = tempfile::tempdir().unwrap();
    let s = d.path().join("settings.json");
    let orig = json!({"model":"x","hooks":{"Stop":[{"matcher":"","hooks":[{"type":"command","command":"say hi"}]}]},"zeta":1});
    fs::write(&s, serde_json::to_string(&orig).unwrap()).unwrap();
    let av = fake_av(d.path(), "av");
    install(&s, &av, t(4)).unwrap();
    let r = uninstall(&s, t(5)).unwrap();
    assert!(r.changed);
    assert!(r.backup.is_some());
    let v: Value = serde_json::from_str(&fs::read_to_string(&s).unwrap()).unwrap();
    assert_eq!(v, orig);
    assert_eq!(hooks_status(&s), HooksStatus::NotInstalled);
}

#[test]
fn uninstall_removes_empty_hooks_and_noop_when_absent() {
    let d = tempfile::tempdir().unwrap();
    let s = d.path().join("settings.json");
    fs::write(&s, r#"{"a":1}"#).unwrap();
    let av = fake_av(d.path(), "av");
    // nothing to remove: no backup, no write
    let r = uninstall(&s, t(6)).unwrap();
    assert!(!r.changed && r.backup.is_none());
    assert!(backups(d.path()).is_empty());
    install(&s, &av, t(7)).unwrap();
    uninstall(&s, t(8)).unwrap();
    let v: Value = serde_json::from_str(&fs::read_to_string(&s).unwrap()).unwrap();
    assert_eq!(v, json!({"a":1}));
    assert_eq!(backups(d.path()).len(), 2);
}

#[test]
fn same_now_does_not_overwrite_backup() {
    let d = tempfile::tempdir().unwrap();
    let s = d.path().join("settings.json");
    fs::write(&s, r#"{"a":1}"#).unwrap();
    let av = fake_av(d.path(), "av");
    install(&s, &av, t(9)).unwrap();
    uninstall(&s, t(9)).unwrap();
    let b = backups(d.path());
    assert_eq!(b.len(), 2);
    assert!(b[1]
        .to_string_lossy()
        .ends_with("agent-viewer-backup-20261030-120009-1"));
}

#[test]
fn invalid_json_refused() {
    let d = tempfile::tempdir().unwrap();
    let s = d.path().join("settings.json");
    fs::write(&s, "{ not json").unwrap();
    let av = fake_av(d.path(), "av");
    assert!(matches!(
        install(&s, &av, t(10)),
        Err(HookError::InvalidJson(_))
    ));
    assert!(matches!(
        uninstall(&s, t(10)),
        Err(HookError::InvalidJson(_))
    ));
    assert_eq!(fs::read(&s).unwrap(), b"{ not json");
    assert!(backups(d.path()).is_empty());
}

#[test]
fn broken_when_path_missing() {
    let d = tempfile::tempdir().unwrap();
    let s = d.path().join("settings.json");
    let av1 = fake_av(d.path(), "av1");
    install(&s, &av1, t(11)).unwrap();
    fs::remove_file(&av1).unwrap();
    assert_eq!(hooks_status(&s), HooksStatus::Broken);
    let av2 = fake_av(d.path(), "av2");
    install(&s, &av2, t(12)).unwrap();
    assert_eq!(hooks_status(&s), HooksStatus::Installed);
    let v: Value = serde_json::from_str(&fs::read_to_string(&s).unwrap()).unwrap();
    assert_eq!(marker_count(&v, "Stop"), 1);
    assert_eq!(marker_count(&v, "Notification"), 1);
}

#[test]
fn not_installed_when_absent() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(
        hooks_status(&d.path().join("nope.json")),
        HooksStatus::NotInstalled
    );
}

#[test]
fn symlinked_settings_survive() {
    let d = tempfile::tempdir().unwrap();
    fs::create_dir(d.path().join("dotfiles")).unwrap();
    let target = d.path().join("dotfiles/settings.json");
    fs::write(&target, r#"{"a":1}"#).unwrap();
    let link = d.path().join("settings.json");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let av = fake_av(d.path(), "av");
    let r = install(&link, &av, t(13)).unwrap();
    assert!(fs::symlink_metadata(&link)
        .unwrap()
        .file_type()
        .is_symlink());
    assert!(fs::read_to_string(&target).unwrap().contains(MARKER));
    // backup sits next to the link path, not the target
    assert_eq!(r.backup.unwrap().parent().unwrap(), d.path());
    assert!(backups(&d.path().join("dotfiles")).is_empty());
}

#[test]
fn path_with_spaces_runs() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("Agent Viewer.app/Contents/MacOS");
    fs::create_dir_all(&dir).unwrap();
    let av = dir.join("av");
    fs::write(&av, "#!/bin/sh\necho ok > \"$(dirname \"$0\")/ran\"\n").unwrap();
    fs::set_permissions(&av, fs::Permissions::from_mode(0o755)).unwrap();
    let cmd = hook_command(&av);
    assert!(cmd.starts_with('"') && cmd.ends_with("hook --agent-viewer"));
    let st = std::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .status()
        .unwrap();
    assert!(st.success());
    assert!(dir.join("ran").exists());
}
