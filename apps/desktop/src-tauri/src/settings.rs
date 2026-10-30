//! App settings, stored as `settings.json` in the Tauri app data dir.

use av_core::notifier::NotifySettings;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const FILE_NAME: &str = "settings.json";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub notify: NotifySettings,
    pub launch_at_login: bool,
    /// Overrides `$HOME/.claude`. Takes effect on restart.
    pub claude_dir: Option<String>,
    pub hooks_banner_dismissed: bool,
}

/// Reads `<dir>/settings.json`. A missing or invalid file gives the defaults.
pub fn load(dir: &Path) -> AppSettings {
    fs::read_to_string(dir.join(FILE_NAME))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Writes `<dir>/settings.json` atomically (temp file + rename).
pub fn save(dir: &Path, s: &AppSettings) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let body = serde_json::to_string_pretty(s).map_err(io::Error::other)?;
    let tmp = dir.join(format!("{FILE_NAME}.tmp"));
    fs::write(&tmp, body)?;
    fs::rename(&tmp, dir.join(FILE_NAME))
}

/// `settings.claude_dir` if set and non-empty, else `$HOME/.claude`.
pub fn resolve_claude_dir(s: &AppSettings, home: Option<PathBuf>) -> PathBuf {
    match s.claude_dir.as_deref().map(str::trim) {
        Some(d) if !d.is_empty() => PathBuf::from(d),
        _ => home.unwrap_or_default().join(".claude"),
    }
}

/// Env var that overrides the claude folder for this run (perf fixtures,
/// testing against a scratch folder). Checked before `settings.claudeDir`.
pub const CLAUDE_DIR_ENV: &str = "AGENT_VIEWER_CLAUDE_DIR";

/// `override_dir` (the [`CLAUDE_DIR_ENV`] value) when set and non-empty, else
/// [`resolve_claude_dir`].
pub fn resolve_claude_dir_with_override(
    s: &AppSettings,
    override_dir: Option<std::ffi::OsString>,
    home: Option<PathBuf>,
) -> PathBuf {
    match override_dir {
        Some(d) if !d.to_string_lossy().trim().is_empty() => PathBuf::from(d),
        _ => resolve_claude_dir(s, home),
    }
}

/// The `--claude-dir` to put in the hook command: `None` when `claude_dir` is
/// the default `$HOME/.claude` (where `av hook` writes anyway), else the dir.
pub fn hook_claude_dir(claude_dir: &Path, home: Option<&Path>) -> Option<PathBuf> {
    match home {
        Some(h) if h.join(".claude") == claude_dir => None,
        _ => Some(claude_dir.to_path_buf()),
    }
}
