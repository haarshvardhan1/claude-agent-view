//! Maps a session's cwd + sessionId to its transcript file (read-only).

use std::fs;
use std::path::{Path, PathBuf};

/// Encode a cwd the way Claude Code names project folders: every char that is
/// not ASCII alphanumeric becomes `-`.
pub fn encode_project_dir(cwd: &str) -> String {
    cwd.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Locate `<claude_dir>/projects/<encode(cwd)>/<session_id>.jsonl`, falling
/// back to a single-level scan of `projects/*/<session_id>.jsonl`.
pub fn find_transcript(claude_dir: &Path, cwd: &str, session_id: &str) -> Option<PathBuf> {
    let projects = claude_dir.join("projects");
    let file = format!("{session_id}.jsonl");
    let direct = projects.join(encode_project_dir(cwd)).join(&file);
    if direct.is_file() {
        return Some(direct);
    }
    fs::read_dir(&projects)
        .ok()?
        .filter_map(Result::ok)
        .map(|e| e.path().join(&file))
        .find(|p| p.is_file())
}

/// `<transcript without .jsonl>/subagents`.
pub fn subagents_dir(transcript: &Path) -> PathBuf {
    transcript.with_extension("").join("subagents")
}
