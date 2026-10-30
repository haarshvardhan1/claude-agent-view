//! Incremental state store behind the desktop app.
//!
//! The engine owns no threads or watchers: the caller feeds it [`Input`]s with
//! the current time and emits the returned [`Change`]s. Read-only: it never
//! writes to `~/.claude` or signals any process.

use crate::hooks::events::{events_path, EventsTail, HookKind};
use crate::liveness::{alive_from, check_alive, ProcessProbe};
use crate::model::{Agent, Change, Status, Subagent};
use crate::paths::{encode_project_dir, find_transcript, subagents_dir};
use crate::registry::{read_registry, SessionEntry};
use crate::status::{derive_status_v2, ActiveHook, StatusInputs};
use crate::subagents::{subagent_files, subagent_info};
use crate::transcript::TranscriptTail;
use crate::view::{build_agent, build_subagent};
use chrono::{DateTime, Duration, TimeZone, Utc};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::io::ErrorKind;
use std::mem::discriminant;
use std::path::{Path, PathBuf};

/// Liveness (`ps`) is re-checked at most this often per session on `Tick`.
pub const LIVENESS_RECHECK_SECS: i64 = 5;
/// Hook events older than this are ignored (startup replay window).
pub const EVENT_MAX_AGE_SECS: i64 = 10 * 60;
/// Exited agents stay listed this long, then are removed.
pub const EXITED_KEEP_HOURS: i64 = 24;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    /// Something under `<claude_dir>/sessions` changed.
    Registry,
    /// A transcript (main or subagent) changed.
    Transcript(PathBuf),
    /// The hook events file grew.
    Events,
    /// Periodic timer (liveness re-checks, time-based statuses, expiry).
    Tick,
    /// Full re-read: registry, every transcript, events.
    Rescan,
    /// Hooks were installed (`true`) or removed (`false`); same as
    /// [`Engine::set_hooks_installed`]. Lets a caller route install/uninstall
    /// through the thread that drives the engine, in order with other inputs.
    HooksInstalled(bool),
}

struct SubTail {
    tail: TranscriptTail,
    readable: bool,
}

struct Session {
    entry: SessionEntry,
    in_registry: bool,
    alive: bool,
    last_probe: DateTime<Utc>,
    tail: TranscriptTail,
    transcript_exists: bool,
    read_error: Option<String>,
    /// Set while this session's registry file fails to read or parse.
    registry_error: Option<String>,
    subs: HashMap<PathBuf, SubTail>,
    subs_view: Vec<Subagent>,
    subs_dirty: bool,
    hook: Option<ActiveHook>,
    stopped_at: Option<DateTime<Utc>>,
    /// Newest applied hook event; anything not newer is a replay and ignored.
    last_event_at: Option<DateTime<Utc>>,
    /// This session has delivered at least one accepted hook event, so its
    /// Claude Code process runs our hooks (R25).
    hook_seen: bool,
    status_since: DateTime<Utc>,
    exited_at: Option<DateTime<Utc>>,
    last: Option<Agent>,
}

pub struct Engine {
    claude_dir: PathBuf,
    probe: Box<dyn ProcessProbe + Send>,
    hooks_installed: bool,
    /// When hooks were detected installed: sessions started after this run
    /// our hooks (Claude Code snapshots hooks at session start). `None`
    /// while hooks are not installed (R25).
    hooks_since: Option<DateTime<Utc>>,
    sessions: HashMap<String, Session>,
    /// Sessions removed (or skipped) as long-exited, with the registry entry
    /// they had, so a lingering registry file does not bring them back.
    tombstones: HashMap<String, serde_json::Value>,
    events: EventsTail,
}

fn entry_value(e: &SessionEntry) -> serde_json::Value {
    serde_json::to_value(e).unwrap_or_default()
}

fn ms_to_utc(ms: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(ms).single().unwrap_or_default()
}

/// `(pid, error)` for each registry file that failed to read or parse, from
/// `read_registry` warnings of the form `<pid>.json: <error>`.
fn failed_registry_pids(warnings: &[String]) -> Vec<(u32, String)> {
    warnings
        .iter()
        .filter_map(|w| {
            let (pid, err) = w.split_once(".json: ")?;
            Some((pid.parse().ok()?, err.to_string()))
        })
        .collect()
}

fn is_subagent_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with("agent-") && n.ends_with(".jsonl"))
}

/// Reads the main transcript. A missing file is not an error (empty summary).
fn read_main(s: &mut Session) {
    match s.tail.read_new() {
        Ok(_) => {
            s.transcript_exists = true;
            s.read_error = None;
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {
            s.transcript_exists = false;
            s.read_error = None;
        }
        Err(e) => {
            s.transcript_exists = true;
            s.read_error = Some(e.to_string());
        }
    }
}

fn read_sub(sub: &mut SubTail) -> bool {
    match sub.tail.read_new() {
        Ok(changed) => {
            let was = sub.readable;
            sub.readable = true;
            changed || !was
        }
        Err(_) => std::mem::replace(&mut sub.readable, false),
    }
}

/// Reads every known subagent tail; with `discover`, first adds tails for
/// subagent files not seen yet.
fn read_subs(s: &mut Session, discover: bool) {
    if discover {
        for (jsonl, _) in subagent_files(s.tail.path()) {
            s.subs.entry(jsonl.clone()).or_insert_with(|| {
                s.subs_dirty = true;
                SubTail {
                    tail: TranscriptTail::new(jsonl),
                    readable: false,
                }
            });
        }
    }
    for sub in s.subs.values_mut() {
        if read_sub(sub) {
            s.subs_dirty = true;
        }
    }
}

/// Points the session at a (newly found) transcript and reads it from the start.
fn attach_transcript(s: &mut Session, path: PathBuf) {
    if s.tail.path() != path {
        s.tail = TranscriptTail::new(path);
        s.subs.clear();
        s.subs_dirty = true;
    }
    read_main(s);
    read_subs(s, true);
}

/// Re-probes liveness; a probe error keeps the previous `alive`.
fn apply_probe(s: &mut Session, probe: &dyn ProcessProbe, now: DateTime<Utc>) {
    s.last_probe = now;
    if let Some(alive) = check_alive(&s.entry, probe) {
        set_alive(s, alive, now);
    }
}

fn set_alive(s: &mut Session, alive: bool, now: DateTime<Utc>) {
    if s.alive != alive {
        s.alive = alive;
        s.exited_at = (!alive).then_some(now);
        if !alive {
            s.stopped_at = None;
        }
        s.subs_dirty = true;
    }
}

fn build_subs(s: &Session) -> Vec<Subagent> {
    let mut infos: Vec<_> = s
        .subs
        .iter()
        .filter(|(_, sub)| sub.readable)
        .map(|(path, sub)| {
            let meta = path.with_extension("meta.json");
            let info = subagent_info(path, Some(&meta), sub.tail.summary(), s.alive);
            (info, path)
        })
        .collect();
    infos.sort_by(|(a, _), (b, _)| {
        b.summary
            .last_activity
            .cmp(&a.summary.last_activity)
            .then_with(|| a.agent_id.cmp(&b.agent_id))
    });
    infos
        .iter()
        .map(|(info, path)| build_subagent(info, path))
        .collect()
}

impl Engine {
    pub fn new(
        claude_dir: PathBuf,
        probe: Box<dyn ProcessProbe + Send>,
        hooks_installed: bool,
    ) -> Self {
        let events = EventsTail::new(events_path(&claude_dir));
        // Installed before we started: the best install time we have is the
        // settings file's mtime (any later edit only makes this conservative).
        let hooks_since = hooks_installed
            .then(|| std::fs::metadata(claude_dir.join("settings.json")).ok())
            .flatten()
            .and_then(|m| m.modified().ok())
            .map(DateTime::<Utc>::from);
        Self {
            claude_dir,
            probe,
            hooks_installed,
            hooks_since,
            sessions: HashMap::new(),
            tombstones: HashMap::new(),
            events,
        }
    }

    /// First full read: registry, transcripts, subagents, and a replay of
    /// recent hook events. Returns `AgentAdded` for each agent.
    pub fn load(&mut self, now: DateTime<Utc>) -> Vec<Change> {
        self.sync_registry(now);
        self.read_events(now);
        self.rebuild_all(now)
    }

    pub fn handle(&mut self, input: Input, now: DateTime<Utc>) -> Vec<Change> {
        match input {
            Input::Registry => {
                let touched = self.sync_registry(now);
                self.rebuild(touched, now)
            }
            Input::Transcript(path) => {
                let touched = self.route_transcript(&path);
                self.rebuild(touched.into_iter().collect(), now)
            }
            Input::Events => {
                let touched = self.read_events(now);
                self.rebuild(touched, now)
            }
            Input::Tick => {
                self.probe_due(now);
                let mut changes = self.expire(now);
                changes.extend(self.rebuild_all(now));
                changes
            }
            Input::HooksInstalled(on) => self.set_hooks_installed(on, now),
            Input::Rescan => {
                self.sync_registry(now);
                for s in self.sessions.values_mut() {
                    read_main(s);
                    read_subs(s, true);
                }
                self.read_events(now);
                self.rebuild_all(now)
            }
        }
    }

    /// Re-probes every live session whose last probe is at least
    /// `LIVENESS_RECHECK_SECS` old, with one batched probe call.
    fn probe_due(&mut self, now: DateTime<Utc>) {
        let max_age = Duration::seconds(LIVENESS_RECHECK_SECS);
        let mut due: Vec<&mut Session> = self
            .sessions
            .values_mut()
            .filter(|s| s.in_registry && s.alive && now - s.last_probe >= max_age)
            .collect();
        if due.is_empty() {
            return;
        }
        let pids: Vec<u32> = due.iter().map(|s| s.entry.pid).collect();
        let results = self.probe.check_many(&pids);
        for (s, probed) in due.iter_mut().zip(results) {
            s.last_probe = now;
            if let Some(alive) = alive_from(&s.entry, probed) {
                set_alive(s, alive, now);
            }
        }
    }

    /// Records whether hooks are installed. The first `true` after `false`
    /// stamps `now` as the install time; only sessions started after it (or
    /// that have delivered a hook event) count as hooked (R25).
    pub fn set_hooks_installed(&mut self, on: bool, now: DateTime<Utc>) -> Vec<Change> {
        if !on {
            self.hooks_since = None;
        } else if !self.hooks_installed || self.hooks_since.is_none() {
            self.hooks_since = Some(now);
        }
        self.hooks_installed = on;
        self.rebuild_all(now)
    }

    /// The last emitted agents, sorted by id.
    pub fn agents(&self) -> Vec<Agent> {
        let mut out: Vec<Agent> = self
            .sessions
            .values()
            .filter_map(|s| s.last.clone())
            .collect();
        out.sort_by(|a, b| a.id.cmp(&b.id));
        out
    }

    pub fn transcript_path(&self, agent_id: &str, subagent_id: Option<&str>) -> Option<PathBuf> {
        let s = self.sessions.get(agent_id)?;
        match subagent_id {
            None => s.transcript_exists.then(|| s.tail.path().to_path_buf()),
            Some(sub) => {
                let name = format!("agent-{sub}.jsonl");
                s.subs
                    .keys()
                    .find(|p| p.file_name().and_then(|n| n.to_str()) == Some(name.as_str()))
                    .cloned()
            }
        }
    }

    fn expected_transcript(&self, e: &SessionEntry) -> PathBuf {
        self.claude_dir
            .join("projects")
            .join(encode_project_dir(&e.cwd))
            .join(format!("{}.jsonl", e.session_id))
    }

    /// A session first seen in the registry, or `None` when it is already
    /// dead for longer than the retention window.
    fn new_session(&self, entry: SessionEntry, now: DateTime<Utc>) -> Option<Session> {
        let alive = check_alive(&entry, &*self.probe).unwrap_or(true);
        let exited_at = (!alive).then(|| ms_to_utc(entry.updated_at_ms).min(now));
        if exited_at.is_some_and(|x| now - x > Duration::hours(EXITED_KEEP_HOURS)) {
            return None;
        }
        let path = find_transcript(&self.claude_dir, &entry.cwd, &entry.session_id)
            .unwrap_or_else(|| self.expected_transcript(&entry));
        let mut s = Session {
            entry,
            in_registry: true,
            alive,
            last_probe: now,
            tail: TranscriptTail::new(path),
            transcript_exists: false,
            read_error: None,
            registry_error: None,
            subs: HashMap::new(),
            subs_view: Vec::new(),
            subs_dirty: true,
            hook: None,
            stopped_at: None,
            last_event_at: None,
            hook_seen: false,
            status_since: exited_at.unwrap_or(now),
            exited_at,
            last: None,
        };
        read_main(&mut s);
        read_subs(&mut s, true);
        Some(s)
    }

    /// Re-reads the registry. New ids are added; known ids whose entry changed
    /// are re-probed; ids no longer listed (file gone, or its pid now names
    /// another session) become exited. A known session whose file failed to
    /// read keeps its state and shows `Unknown` until the next good read.
    /// Returns the touched session ids.
    fn sync_registry(&mut self, now: DateTime<Utc>) -> Vec<String> {
        let (entries, warnings) = read_registry(&self.claude_dir);
        let mut seen = HashSet::new();
        let mut touched = Vec::new();
        for (pid, err) in failed_registry_pids(&warnings) {
            let pid_owner = self
                .sessions
                .iter_mut()
                .find(|(_, s)| s.in_registry && s.entry.pid == pid);
            if let Some((id, s)) = pid_owner {
                seen.insert(id.clone());
                if s.registry_error.as_ref() != Some(&err) {
                    s.registry_error = Some(err);
                    touched.push(id.clone());
                }
            }
        }
        for entry in entries {
            let id = entry.session_id.clone();
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Some(s) = self.sessions.get_mut(&id) {
                if s.registry_error.take().is_some() {
                    touched.push(id.clone());
                }
            }
            if let Some(dead) = self.tombstones.get(&id) {
                if *dead == entry_value(&entry) {
                    continue;
                }
                self.tombstones.remove(&id);
            }
            let Some(s) = self.sessions.get_mut(&id) else {
                let value = entry_value(&entry);
                match self.new_session(entry, now) {
                    Some(s) => {
                        self.sessions.insert(id.clone(), s);
                        touched.push(id);
                    }
                    None => {
                        self.tombstones.insert(id, value);
                    }
                }
                continue;
            };
            if !s.in_registry || entry_value(&s.entry) != entry_value(&entry) {
                s.in_registry = true;
                s.entry = entry;
                apply_probe(s, &*self.probe, now);
                touched.push(id.clone());
            }
            if !s.transcript_exists {
                if let Some(p) = find_transcript(&self.claude_dir, &s.entry.cwd, &id) {
                    attach_transcript(s, p);
                    touched.push(id);
                }
            }
        }
        for (id, s) in &mut self.sessions {
            if s.in_registry && !seen.contains(id) {
                s.in_registry = false;
                set_alive(s, false, now);
                touched.push(id.clone());
            }
        }
        self.tombstones.retain(|id, _| seen.contains(id));
        touched
    }

    /// Routes a changed path to the session it belongs to, if any.
    fn route_transcript(&mut self, path: &Path) -> Option<String> {
        if let Some((id, s)) = self
            .sessions
            .iter_mut()
            .find(|(_, s)| s.tail.path() == path)
        {
            read_main(s);
            return Some(id.clone());
        }
        let parent = path.parent();
        if let Some((id, s)) = self
            .sessions
            .iter_mut()
            .find(|(_, s)| parent == Some(subagents_dir(s.tail.path()).as_path()))
        {
            if is_subagent_file(path) {
                let sub = s.subs.entry(path.to_path_buf()).or_insert_with(|| SubTail {
                    tail: TranscriptTail::new(path.to_path_buf()),
                    readable: false,
                });
                read_sub(sub);
            }
            // Any change in the dir (including meta files) refreshes the list.
            s.subs_dirty = true;
            return Some(id.clone());
        }
        // A transcript that did not exist yet, created somewhere other than
        // the expected project folder.
        let name = path.file_name()?.to_str()?;
        let (id, s) = self.sessions.iter_mut().find(|(id, s)| {
            !s.transcript_exists && name.strip_suffix(".jsonl") == Some(id.as_str())
        })?;
        if !path.is_file() {
            return None;
        }
        attach_transcript(s, path.to_path_buf());
        Some(id.clone())
    }

    /// Applies new hook events. Ignored: events older than the replay window,
    /// for unknown or dead sessions, or not newer than the session's last
    /// applied event (a replay after the file was trimmed or replaced).
    fn read_events(&mut self, now: DateTime<Utc>) -> Vec<String> {
        let Ok(events) = self.events.read_new() else {
            return Vec::new();
        };
        let oldest = now - Duration::seconds(EVENT_MAX_AGE_SECS);
        let mut touched = Vec::new();
        for ev in events {
            if ev.received_at < oldest {
                continue;
            }
            let Some(s) = self.sessions.get_mut(&ev.session_id) else {
                continue;
            };
            if !s.alive || s.last_event_at.is_some_and(|t| ev.received_at <= t) {
                continue;
            }
            s.last_event_at = Some(ev.received_at);
            s.hook_seen = true;
            match ev.kind {
                HookKind::Notification(signal) => {
                    s.hook = Some(ActiveHook {
                        signal,
                        at: ev.received_at,
                    });
                }
                HookKind::Stop => {
                    s.hook = None;
                    s.stopped_at = Some(ev.received_at);
                }
            }
            touched.push(ev.session_id);
        }
        touched
    }

    /// Removes agents exited longer than the retention window.
    fn expire(&mut self, now: DateTime<Utc>) -> Vec<Change> {
        let keep = Duration::hours(EXITED_KEEP_HOURS);
        let mut ids: Vec<String> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.exited_at.is_some_and(|x| now - x > keep))
            .map(|(id, _)| id.clone())
            .collect();
        ids.sort();
        let mut changes = Vec::new();
        for id in ids {
            let s = self.sessions.remove(&id).expect("listed above");
            if s.in_registry {
                self.tombstones.insert(id.clone(), entry_value(&s.entry));
            }
            if s.last.is_some() {
                changes.push(Change::AgentRemoved { id });
            }
        }
        changes
    }

    fn rebuild_all(&mut self, now: DateTime<Utc>) -> Vec<Change> {
        let ids = self.sessions.keys().cloned().collect();
        self.rebuild(ids, now)
    }

    /// Rebuilds the given sessions (sorted, deduplicated) and returns a change
    /// for each agent that is new or differs from the last emitted one.
    fn rebuild(&mut self, ids: Vec<String>, now: DateTime<Utc>) -> Vec<Change> {
        let ids: BTreeSet<String> = ids.into_iter().collect();
        ids.iter()
            .filter_map(|id| self.rebuild_one(id, now))
            .collect()
    }

    fn rebuild_one(&mut self, id: &str, now: DateTime<Utc>) -> Option<Change> {
        let hooks_on = self.hooks_installed;
        let hooks_since = self.hooks_since;
        let s = self.sessions.get_mut(id)?;
        // Hooks only count for a session that runs them (R25): it delivered
        // an event, or it started after hooks were installed.
        let hooks_installed = hooks_on
            && (s.hook_seen || hooks_since.is_some_and(|t| ms_to_utc(s.entry.started_at_ms) > t));
        let summary = s.tail.summary();
        let newer = |t: DateTime<Utc>| summary.last_activity.is_some_and(|la| la > t);

        // A hook lasts until newer transcript activity or the process exits.
        if !s.alive || s.hook.as_ref().is_some_and(|h| newer(h.at)) {
            s.hook = None;
        }
        // A new prompt after a Stop starts a new turn.
        if s.entry.status == "busy" && s.stopped_at.is_some_and(newer) {
            s.stopped_at = None;
        }

        let status = if !s.alive {
            Status::Exited
        } else if let Some(reason) = s.registry_error.as_ref().or(s.read_error.as_ref()) {
            Status::Unknown {
                reason: reason.clone(),
            }
        } else {
            derive_status_v2(&StatusInputs {
                alive: s.alive,
                registry_status: &s.entry.status,
                summary: &summary,
                hook: s.hook.as_ref(),
                stopped_at: s.stopped_at,
                hooks_installed,
                now,
            })
        };
        if let Some(prev) = &s.last {
            if discriminant(&prev.status) != discriminant(&status) {
                s.status_since = match status {
                    Status::Exited => s.exited_at.unwrap_or(now),
                    _ => now,
                };
            }
        }
        if s.subs_dirty {
            s.subs_view = build_subs(s);
            s.subs_dirty = false;
        }

        let transcript = s.transcript_exists.then(|| s.tail.path());
        let agent = build_agent(
            &s.entry,
            &summary,
            s.subs_view.clone(),
            status,
            s.status_since,
            transcript,
        );
        let change = match &s.last {
            None => Some(Change::AgentAdded {
                agent: agent.clone(),
            }),
            Some(prev) if *prev != agent => Some(Change::AgentUpdated {
                agent: agent.clone(),
            }),
            Some(_) => None,
        };
        s.last = Some(agent);
        change
    }
}
