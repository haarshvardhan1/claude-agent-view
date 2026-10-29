mod common;

use av_core::paths::*;
use std::fs;
use std::path::Path;

fn touch(p: &Path) {
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, "{}\n").unwrap();
}

#[test]
fn encodes_spaces() {
    assert_eq!(
        encode_project_dir("/Users/g2c/Documents/Agent Viewer Application"),
        "-Users-g2c-Documents-Agent-Viewer-Application"
    );
}

#[test]
fn encodes_worktree() {
    assert_eq!(
        encode_project_dir("/Users/g2c/repo/.claude/worktrees/zero"),
        "-Users-g2c-repo--claude-worktrees-zero"
    );
}

#[test]
fn finds_by_encoded_cwd() {
    let d = tempfile::tempdir().unwrap();
    let t = d.path().join("projects/-Users-g2c-repo/abc.jsonl");
    touch(&t);
    assert_eq!(find_transcript(d.path(), "/Users/g2c/repo", "abc"), Some(t));
}

#[test]
fn falls_back_to_search() {
    let d = tempfile::tempdir().unwrap();
    let t = d.path().join("projects/some-other-folder/abc.jsonl");
    touch(&t);
    touch(&d.path().join("projects/unrelated/zzz.jsonl"));
    assert_eq!(find_transcript(d.path(), "/Users/g2c/repo", "abc"), Some(t));
}

#[test]
fn none_when_missing() {
    let d = tempfile::tempdir().unwrap();
    assert_eq!(find_transcript(d.path(), "/x", "abc"), None);
    fs::create_dir_all(d.path().join("projects/p")).unwrap();
    assert_eq!(find_transcript(d.path(), "/x", "abc"), None);
}

#[test]
fn subagents_dir_is_sibling_folder() {
    assert_eq!(
        subagents_dir(Path::new("/a/projects/p/abc.jsonl")),
        Path::new("/a/projects/p/abc/subagents")
    );
}
