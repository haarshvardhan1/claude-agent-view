use desktop_lib::settings::{AppSettings, load, save};
use std::fs;

#[test]
fn settings_roundtrip_and_defaults() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path();

    let s = load(dir);
    assert_eq!(s, AppSettings::default());
    assert_eq!(s.notify.finished_after_secs, 30);

    let mut changed = AppSettings::default();
    changed.notify.finished = false;
    changed.notify.finished_after_secs = 90;
    changed.launch_at_login = true;
    changed.claude_dir = Some("/tmp/somewhere/.claude".into());
    changed.hooks_banner_dismissed = true;
    save(dir, &changed).unwrap();
    assert_eq!(load(dir), changed);

    fs::write(dir.join("settings.json"), "{ not json").unwrap();
    assert_eq!(load(dir), AppSettings::default());
}
