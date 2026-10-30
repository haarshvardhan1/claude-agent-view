//! Reader for Claude Code's session registry (`<claude_dir>/sessions/*.json`). Read-only.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEntry {
    pub pid: u32,
    pub session_id: String,
    #[serde(default)]
    pub cwd: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub entrypoint: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub proc_start: Option<String>,
    #[serde(default, rename = "startedAt")]
    pub started_at_ms: i64,
    #[serde(default, rename = "updatedAt")]
    pub updated_at_ms: i64,
}

/// Reads all `*.json` registry files, newest-started first.
/// Returns entries plus one warning per unreadable file.
pub fn read_registry(claude_dir: &Path) -> (Vec<SessionEntry>, Vec<String>) {
    let mut entries = Vec::new();
    let mut warnings = Vec::new();
    let Ok(dir) = fs::read_dir(claude_dir.join("sessions")) else {
        return (entries, warnings);
    };
    for item in dir.flatten() {
        let path = item.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        match fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str::<SessionEntry>(&s).map_err(|e| e.to_string()))
        {
            Ok(entry) => entries.push(entry),
            Err(e) => warnings.push(format!("{name}: {e}")),
        }
    }
    entries.sort_by_key(|e| std::cmp::Reverse(e.started_at_ms));
    (entries, warnings)
}
