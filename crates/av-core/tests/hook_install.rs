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
    let r = install(&s, &av, None, t(0)).unwrap();
    assert!(r.changed);
    assert!(r.backup.is_none());
    assert!(s.exists());
    assert_eq!(hooks_status(&s, None), HooksStatus::Installed);
    assert!(backups(d.path()).is_empty());
}

#[test]
fn install_keeps_existing() {
    let d = tempfile::tempdir().unwrap();
    let s = d.path().join("settings.json");
    let orig = r#"{"model":"x","hooks":{"Stop":[{"matcher":"","hooks":[{"type":"command","command":"say hi"}]}]},"zeta":1}"#;
    fs::write(&s, orig).unwrap();
    let av = fake_av(d.path(), "av");
    let r = install(&s, &av, None, t(1)).unwrap();
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
    assert!(install(&s, &av, None, t(2)).unwrap().changed);
    let bytes = fs::read(&s).unwrap();
    let r = install(&s, &av, None, t(3)).unwrap();
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
    install(&s, &av, None, t(4)).unwrap();
    let r = uninstall(&s, t(5)).unwrap();
    assert!(r.changed);
    assert!(r.backup.is_some());
    let v: Value = serde_json::from_str(&fs::read_to_string(&s).unwrap()).unwrap();
    assert_eq!(v, orig);
    assert_eq!(hooks_status(&s, None), HooksStatus::NotInstalled);
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
    install(&s, &av, None, t(7)).unwrap();
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
    install(&s, &av, None, t(9)).unwrap();
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
        install(&s, &av, None, t(10)),
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
    install(&s, &av1, None, t(11)).unwrap();
    fs::remove_file(&av1).unwrap();
    assert_eq!(hooks_status(&s, None), HooksStatus::Broken);
    let av2 = fake_av(d.path(), "av2");
    install(&s, &av2, None, t(12)).unwrap();
    assert_eq!(hooks_status(&s, None), HooksStatus::Installed);
    let v: Value = serde_json::from_str(&fs::read_to_string(&s).unwrap()).unwrap();
    assert_eq!(marker_count(&v, "Stop"), 1);
    assert_eq!(marker_count(&v, "Notification"), 1);
}

#[test]
fn not_installed_when_absent() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(
        hooks_status(&d.path().join("nope.json"), None),
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
    let r = install(&link, &av, None, t(13)).unwrap();
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
    let cmd = hook_command(&av, None);
    assert!(cmd.starts_with('"') && cmd.ends_with("hook --agent-viewer"));
    let st = std::process::Command::new("sh")
        .arg("-c")
        .arg(&cmd)
        .status()
        .unwrap();
    assert!(st.success());
    assert!(dir.join("ran").exists());
}

#[test]
fn uninstall_preserves_key_order() {
    let d = tempfile::tempdir().unwrap();
    let s = d.path().join("settings.json");
    let orig = r#"{"model":"x","hooks":{"Notification":[{"matcher":"","hooks":[{"type":"command","command":"mine"}]}],"PreToolUse":[]},"zeta":1,"permissions":{}}"#;
    fs::write(&s, orig).unwrap();
    let av = fake_av(d.path(), "av");
    install(&s, &av, None, t(14)).unwrap();
    uninstall(&s, t(15)).unwrap();
    let v: Value = serde_json::from_str(&fs::read_to_string(&s).unwrap()).unwrap();
    let top: Vec<&String> = v.as_object().unwrap().keys().collect();
    assert_eq!(top, ["model", "hooks", "zeta", "permissions"]);
    let ev: Vec<&String> = v["hooks"].as_object().unwrap().keys().collect();
    assert_eq!(ev, ["Notification", "PreToolUse"]);

    // fully emptied `hooks` removal must not reorder siblings either
    fs::write(&s, r#"{"model":"x","hooks":{},"zeta":1,"permissions":{}}"#).unwrap();
    install(&s, &av, None, t(16)).unwrap();
    uninstall(&s, t(17)).unwrap();
    let v: Value = serde_json::from_str(&fs::read_to_string(&s).unwrap()).unwrap();
    let top: Vec<&String> = v.as_object().unwrap().keys().collect();
    assert_eq!(top, ["model", "zeta", "permissions"]);
}

#[test]
fn file_mode_preserved() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let s = d.path().join("settings.json");
    fs::write(&s, r#"{"env":{"K":"secret"}}"#).unwrap();
    fs::set_permissions(&s, fs::Permissions::from_mode(0o600)).unwrap();
    let av = fake_av(d.path(), "av");
    let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
    install(&s, &av, None, t(18)).unwrap();
    assert_eq!(mode(&s), 0o600);
    uninstall(&s, t(19)).unwrap();
    assert_eq!(mode(&s), 0o600);
}

#[test]
fn hook_command_quotes_claude_dir_with_spaces() {
    let av = Path::new("/Applications/Agent Viewer.app/Contents/MacOS/av-hook");
    let dir = Path::new("/Users/me/My Claude");
    assert_eq!(
        hook_command(av, Some(dir)),
        r#""/Applications/Agent Viewer.app/Contents/MacOS/av-hook" hook --agent-viewer --claude-dir "/Users/me/My Claude""#
    );
    assert_eq!(
        hook_command(av, None),
        r#""/Applications/Agent Viewer.app/Contents/MacOS/av-hook" hook --agent-viewer"#
    );
}

/// The flag survives the shell: `sh -c` passes the dir as one argument.
#[test]
fn claude_dir_flag_reaches_av_as_one_argument() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("Agent Viewer.app/Contents/MacOS");
    fs::create_dir_all(&dir).unwrap();
    let av = dir.join("av");
    fs::write(
        &av,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$(dirname \"$0\")/args\"\n",
    )
    .unwrap();
    fs::set_permissions(&av, fs::Permissions::from_mode(0o755)).unwrap();
    let claude = d.path().join("my claude");
    let st = std::process::Command::new("sh")
        .arg("-c")
        .arg(hook_command(&av, Some(&claude)))
        .status()
        .unwrap();
    assert!(st.success());
    let args = fs::read_to_string(dir.join("args")).unwrap();
    let want = format!("hook\n--agent-viewer\n--claude-dir\n{}\n", claude.display());
    assert_eq!(args, want);
}

#[test]
fn install_with_claude_dir_writes_flag_and_is_installed() {
    let d = tempfile::tempdir().unwrap();
    let s = d.path().join("settings.json");
    let av = fake_av(d.path(), "av");
    let claude = d.path().join("other claude");
    install(&s, &av, Some(&claude), t(20)).unwrap();
    let text = fs::read_to_string(&s).unwrap();
    let v: Value = serde_json::from_str(&text).unwrap();
    let cmd = v["hooks"]["Stop"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert_eq!(cmd, hook_command(&av, Some(&claude)));
    assert_eq!(hooks_status(&s, Some(&claude)), HooksStatus::Installed);
    // Re-install with the same dir is a no-op.
    assert!(!install(&s, &av, Some(&claude), t(21)).unwrap().changed);
}

#[test]
fn status_broken_when_claude_dir_mismatches() {
    let d = tempfile::tempdir().unwrap();
    let s = d.path().join("settings.json");
    let av = fake_av(d.path(), "av");
    let a = d.path().join("a");
    let b = d.path().join("b");
    // Installed for `a`, app reads `b`.
    install(&s, &av, Some(&a), t(22)).unwrap();
    assert_eq!(hooks_status(&s, Some(&b)), HooksStatus::Broken);
    // Installed with a dir, app reads the default folder.
    assert_eq!(hooks_status(&s, None), HooksStatus::Broken);
    // Installed for the default folder, app reads an override.
    install(&s, &av, None, t(23)).unwrap();
    assert_eq!(hooks_status(&s, Some(&a)), HooksStatus::Broken);
    // Repair rewrites the command for the right dir, still one per event.
    assert!(install(&s, &av, Some(&b), t(24)).unwrap().changed);
    assert_eq!(hooks_status(&s, Some(&b)), HooksStatus::Installed);
    let v: Value = serde_json::from_str(&fs::read_to_string(&s).unwrap()).unwrap();
    assert_eq!(marker_count(&v, "Stop"), 1);
    assert_eq!(marker_count(&v, "Notification"), 1);
}
