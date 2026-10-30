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

#[test]
fn hook_claude_dir_only_for_non_default_folder() {
    use desktop_lib::settings::hook_claude_dir;
    use std::path::{Path, PathBuf};
    let home = Path::new("/Users/me");
    assert_eq!(hook_claude_dir(&home.join(".claude"), Some(home)), None);
    assert_eq!(
        hook_claude_dir(Path::new("/x/claude"), Some(home)),
        Some(PathBuf::from("/x/claude"))
    );
    assert_eq!(
        hook_claude_dir(Path::new("/x/claude"), None),
        Some(PathBuf::from("/x/claude"))
    );
}
