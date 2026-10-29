mod common;

use assert_cmd::Command;

fn av(args: &[&str]) -> String {
    let dir = tempfile::tempdir().unwrap();
    common::fixture_home(dir.path());
    let out = Command::cargo_bin("av")
        .unwrap()
        .args(args)
        .arg("--claude-dir")
        .arg(dir.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn list_hides_exited_by_default() {
    let out = av(&["list"]);
    assert!(out.contains("STATUS"));
    assert!(!out.contains("my-session"));
}

#[test]
fn list_all_shows_exited() {
    let out = av(&["list", "--all"]);
    assert!(out.contains("my-session"));
    assert!(out.contains("exited"));
    assert!(out.contains("myproj"));
    assert!(out.contains("Fix the widget"));
}

#[test]
fn json_is_valid() {
    let out = av(&["list", "--json", "--all"]);
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["agents"].as_array().unwrap().len(), 1);
}

#[test]
fn subagent_line_is_indented() {
    let out = av(&["list", "--all"]);
    let line = out.lines().find(|l| l.starts_with("  └ ")).expect("subagent line");
    assert!(line.contains("look around"));
    assert!(line.contains("Explore"));
}
