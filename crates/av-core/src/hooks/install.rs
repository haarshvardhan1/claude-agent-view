//! Installs / removes the Agent Viewer hooks in a Claude `settings.json`.
//!
//! Safety rules: back up before any change, never write when the file is not
//! valid JSON, keep every unrelated setting (and key order), and write through
//! a symlinked settings file instead of replacing the link.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{json, Map, Value};
use std::fmt;
use std::path::{Path, PathBuf};

pub const MARKER: &str = "--agent-viewer";
const EVENTS: [&str; 2] = ["Notification", "Stop"];

pub fn hook_command(av_path: &Path) -> String {
    format!("\"{}\" hook {MARKER}", av_path.display())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HooksStatus {
    Installed,
    NotInstalled,
    Broken,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReport {
    pub backup: Option<PathBuf>,
    pub changed: bool,
}

#[derive(Debug)]
pub enum HookError {
    InvalidJson(String),
    Io(std::io::Error),
}

impl fmt::Display for HookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HookError::InvalidJson(m) => write!(f, "settings file is not valid JSON: {m}"),
            HookError::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for HookError {}

impl From<std::io::Error> for HookError {
    fn from(e: std::io::Error) -> Self {
        HookError::Io(e)
    }
}

/// Path that is actually written: the symlink target when `settings` is a symlink.
fn resolve_target(settings: &Path) -> Result<PathBuf, HookError> {
    match std::fs::symlink_metadata(settings) {
        Ok(m) if m.file_type().is_symlink() => Ok(std::fs::canonicalize(settings)?),
        _ => Ok(settings.to_path_buf()),
    }
}

/// Returns the parsed document (`{}` when the file is missing) and whether the file existed.
fn load(target: &Path) -> Result<(Map<String, Value>, bool), HookError> {
    let text = match std::fs::read_to_string(target) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok((Map::new(), false)),
        Err(e) => return Err(e.into()),
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(m)) => Ok((m, true)),
        Ok(_) => Err(HookError::InvalidJson("top level is not an object".into())),
        Err(e) => Err(HookError::InvalidJson(e.to_string())),
    }
}

fn backup_path(settings: &Path, now: DateTime<Utc>) -> PathBuf {
    let name = settings
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "settings.json".into());
    let stamp = now.format("%Y%m%d-%H%M%S");
    let dir = settings.parent().unwrap_or_else(|| Path::new("."));
    let base = format!("{name}.agent-viewer-backup-{stamp}");
    let mut candidate = dir.join(&base);
    let mut n = 1;
    while candidate.symlink_metadata().is_ok() {
        candidate = dir.join(format!("{base}-{n}"));
        n += 1;
    }
    candidate
}

fn write_atomic(target: &Path, doc: &Map<String, Value>) -> Result<(), HookError> {
    let mut text =
        serde_json::to_string_pretty(doc).map_err(|e| HookError::InvalidJson(e.to_string()))?;
    text.push('\n');
    if let Some(dir) = target.parent() {
        if !dir.as_os_str().is_empty() {
            std::fs::create_dir_all(dir)?;
        }
    }
    let mut tmp = target.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, text)?;
    if let Err(e) = std::fs::rename(&tmp, target) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

fn is_marker(hook: &Value) -> bool {
    hook.get("command")
        .and_then(Value::as_str)
        .is_some_and(|c| c.contains(MARKER))
}

/// Removes marker hooks from one event's matcher groups; drops groups that this
/// emptied. Returns whether anything was removed.
fn strip_markers(groups: &mut Vec<Value>) -> bool {
    let mut removed = false;
    groups.retain_mut(|g| {
        let Some(hooks) = g.get_mut("hooks").and_then(Value::as_array_mut) else {
            return true;
        };
        let before = hooks.len();
        hooks.retain(|h| !is_marker(h));
        if hooks.len() == before {
            return true;
        }
        removed = true;
        !hooks.is_empty()
    });
    removed
}

/// All marker commands registered under `event`.
fn marker_commands(doc: &Map<String, Value>, event: &str) -> Vec<String> {
    doc.get("hooks")
        .and_then(|h| h.get(event))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|g| g.get("hooks").and_then(Value::as_array))
        .flatten()
        .filter(|h| is_marker(h))
        .filter_map(|h| h.get("command").and_then(Value::as_str).map(str::to_owned))
        .collect()
}

fn quoted_path(command: &str) -> Option<&str> {
    let rest = command.strip_prefix('"')?;
    rest.split('"').next()
}

fn path_exists(command: &str) -> bool {
    quoted_path(command).is_some_and(|p| Path::new(p).exists())
}

pub fn hooks_status(settings: &Path) -> HooksStatus {
    let Ok(target) = resolve_target(settings) else {
        return HooksStatus::NotInstalled;
    };
    let Ok((doc, _)) = load(&target) else {
        return HooksStatus::NotInstalled;
    };
    let mut broken = false;
    let mut all_present = true;
    for ev in EVENTS {
        let cmds = marker_commands(&doc, ev);
        if cmds.is_empty() {
            all_present = false;
        }
        if cmds.iter().any(|c| !path_exists(c)) {
            broken = true;
        }
    }
    if broken {
        HooksStatus::Broken
    } else if all_present {
        HooksStatus::Installed
    } else {
        HooksStatus::NotInstalled
    }
}

fn finish(
    settings: &Path,
    target: &Path,
    existed: bool,
    doc: &Map<String, Value>,
    now: DateTime<Utc>,
) -> Result<InstallReport, HookError> {
    let backup = if existed {
        let b = backup_path(settings, now);
        std::fs::copy(target, &b)?;
        Some(b)
    } else {
        None
    };
    write_atomic(target, doc)?;
    Ok(InstallReport {
        backup,
        changed: true,
    })
}

pub fn install(
    settings: &Path,
    av_path: &Path,
    now: DateTime<Utc>,
) -> Result<InstallReport, HookError> {
    let target = resolve_target(settings)?;
    let (mut doc, existed) = load(&target)?;
    let command = hook_command(av_path);

    let already = EVENTS
        .iter()
        .all(|ev| marker_commands(&doc, ev) == [command.clone()]);
    if already {
        return Ok(InstallReport {
            backup: None,
            changed: false,
        });
    }

    let hooks = doc
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()));
    let Some(hooks) = hooks.as_object_mut() else {
        return Err(HookError::InvalidJson("`hooks` is not an object".into()));
    };
    for ev in EVENTS {
        let slot = hooks.entry(ev).or_insert_with(|| Value::Array(Vec::new()));
        let Some(groups) = slot.as_array_mut() else {
            return Err(HookError::InvalidJson(format!(
                "`hooks.{ev}` is not an array"
            )));
        };
        strip_markers(groups);
        groups.push(json!({
            "matcher": "",
            "hooks": [{"type": "command", "command": command, "timeout": 5}]
        }));
    }
    finish(settings, &target, existed, &doc, now)
}

pub fn uninstall(settings: &Path, now: DateTime<Utc>) -> Result<InstallReport, HookError> {
    let target = resolve_target(settings)?;
    let (mut doc, existed) = load(&target)?;
    let mut changed = false;
    let mut hooks_empty = false;
    if let Some(hooks) = doc.get_mut("hooks").and_then(Value::as_object_mut) {
        for ev in EVENTS {
            let Some(groups) = hooks.get_mut(ev).and_then(Value::as_array_mut) else {
                continue;
            };
            if strip_markers(groups) {
                changed = true;
                if groups.is_empty() {
                    hooks.remove(ev);
                }
            }
        }
        hooks_empty = changed && hooks.is_empty();
    }
    if !changed {
        return Ok(InstallReport {
            backup: None,
            changed: false,
        });
    }
    if hooks_empty {
        doc.remove("hooks");
    }
    finish(settings, &target, existed, &doc, now)
}
