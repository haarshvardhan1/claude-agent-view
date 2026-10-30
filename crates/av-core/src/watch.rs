//! Debounced file watcher over `~/.claude`, feeding engine `Input`s.

use crate::engine::Input;
use crate::hooks::events::events_path;
use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, Debouncer};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::Duration;

const DEBOUNCE: Duration = Duration::from_millis(100);

/// Pure path classification. `path` must already be under `claude_dir`'s prefix.
pub fn classify_path(claude_dir: &Path, path: &Path) -> Option<Input> {
    if path == events_path(claude_dir) {
        return Some(Input::Events);
    }
    let ext = path.extension().and_then(|e| e.to_str());
    if path.starts_with(claude_dir.join("sessions")) && ext == Some("json") {
        return Some(Input::Registry);
    }
    if path.starts_with(claude_dir.join("projects")) && ext == Some("jsonl") {
        return Some(Input::Transcript(path.to_path_buf()));
    }
    None
}

/// Rewrite a path under the canonical prefix onto the caller's prefix.
pub fn rebase_event_path(claude_dir: &Path, canonical: &Path, path: &Path) -> PathBuf {
    match path.strip_prefix(canonical) {
        Ok(rest) => claude_dir.join(rest),
        Err(_) => path.to_path_buf(),
    }
}

/// Dropping this stops watching.
pub struct WatchHandle {
    _debouncer: Debouncer<notify::RecommendedWatcher>,
}

pub fn spawn_watcher(claude_dir: &Path, tx: Sender<Input>) -> notify::Result<WatchHandle> {
    let canonical = std::fs::canonicalize(claude_dir)?;
    let given = claude_dir.to_path_buf();
    let canon = canonical.clone();
    let mut debouncer = new_debouncer(
        DEBOUNCE,
        move |res: notify_debouncer_mini::DebounceEventResult| {
            let Ok(events) = res else { return };
            let mut seen = HashSet::new();
            for ev in events {
                let p = rebase_event_path(&given, &canon, &ev.path);
                if let Some(input) = classify_path(&given, &p) {
                    if seen.insert(p) && tx.send(input).is_err() {
                        return;
                    }
                }
            }
        },
    )?;
    debouncer
        .watcher()
        .watch(&canonical, RecursiveMode::Recursive)?;
    Ok(WatchHandle {
        _debouncer: debouncer,
    })
}
