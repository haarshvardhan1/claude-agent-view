//! Background runtime: watcher -> engine -> `agents://changed` events.
//!
//! One loop thread drives the engine. The engine sits behind a mutex so
//! commands can read it; locks are held only to compute, never while emitting
//! or reading transcript files.

use crate::commands::{CmdError, PageReq};
use crate::settings::{self, AppSettings};
use av_core::SystemProbe;
use av_core::engine::{Engine, Input};
use av_core::hooks::events::{events_path, trim_events_file};
use av_core::hooks::install::{HooksStatus, hooks_status};
use av_core::liveness::ProcessProbe;
use av_core::model::{Agent, Change, TranscriptPage};
use av_core::notifier::Notifier;
use av_core::transcript_view::read_page;
use av_core::watch::{WatchHandle, spawn_watcher};
use chrono::{DateTime, Utc};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

pub const CHANGED_EVENT: &str = "agents://changed";
pub const EVENTS_MAX_BYTES: u64 = 1_048_576;
pub const PAGE_LIMIT: usize = 200;
const TICK: Duration = Duration::from_secs(1);
const RESCAN: Duration = Duration::from_secs(2);

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

pub struct Runtime {
    claude_dir: PathBuf,
    settings_dir: PathBuf,
    engine: Mutex<Engine>,
    notifier: Mutex<Notifier>,
    settings: Mutex<AppSettings>,
    selected_agent: Mutex<Option<String>>,
    polling: AtomicBool,
}

impl Runtime {
    /// Builds the runtime; reads hooks status from `<claude_dir>/settings.json`.
    /// Writes nothing.
    pub fn new(
        claude_dir: PathBuf,
        settings_dir: PathBuf,
        settings: AppSettings,
        probe: Box<dyn ProcessProbe + Send>,
    ) -> Self {
        let hooks = hooks_status(&claude_dir.join("settings.json")) == HooksStatus::Installed;
        Self {
            engine: Mutex::new(Engine::new(claude_dir.clone(), probe, hooks)),
            claude_dir,
            settings_dir,
            notifier: Mutex::new(Notifier::new()),
            settings: Mutex::new(settings),
            selected_agent: Mutex::new(None),
            polling: AtomicBool::new(false),
        }
    }

    pub fn claude_dir(&self) -> &PathBuf {
        &self.claude_dir
    }

    /// Startup: trims the events file to 1 MB (never creates it), loads the
    /// engine and primes the notifier (initial load never notifies).
    pub fn init(&self, now: DateTime<Utc>) -> Vec<Change> {
        // A missing file is an error here; that's fine, nothing to trim.
        let _ = trim_events_file(&events_path(&self.claude_dir), EVENTS_MAX_BYTES);
        let changes = lock(&self.engine).load(now);
        let notify = lock(&self.settings).notify.clone();
        let _ = lock(&self.notifier).on_changes(&changes, &notify, None, now);
        changes
    }

    /// Feeds one input to the engine and the resulting changes to the notifier.
    pub fn apply(&self, input: Input, now: DateTime<Utc>) -> Vec<Change> {
        let changes = lock(&self.engine).handle(input, now);
        if !changes.is_empty() {
            let notify = lock(&self.settings).notify.clone();
            // Task 13 sends these (with focus suppression).
            let _notifications = lock(&self.notifier).on_changes(&changes, &notify, None, now);
        }
        changes
    }

    pub fn list_agents(&self) -> Vec<Agent> {
        lock(&self.engine).agents()
    }

    pub fn get_transcript(
        &self,
        agent_id: &str,
        subagent_id: Option<&str>,
        req: PageReq,
    ) -> Result<TranscriptPage, CmdError> {
        let path = lock(&self.engine)
            .transcript_path(agent_id, subagent_id)
            .ok_or_else(|| {
                let what = match subagent_id {
                    Some(s) => format!("agent {agent_id} / subagent {s}"),
                    None => format!("agent {agent_id}"),
                };
                CmdError::new("notFound", format!("no transcript for {what}"))
            })?;
        read_page(&path, req.into(), PAGE_LIMIT).map_err(CmdError::io)
    }

    pub fn settings(&self) -> AppSettings {
        lock(&self.settings).clone()
    }

    /// Saves to disk, then updates memory. `claudeDir` applies on restart.
    pub fn set_settings(&self, s: AppSettings) -> Result<(), CmdError> {
        settings::save(&self.settings_dir, &s).map_err(CmdError::io)?;
        *lock(&self.settings) = s;
        Ok(())
    }

    pub fn set_selected_agent(&self, id: Option<String>) {
        *lock(&self.selected_agent) = id;
    }

    pub fn selected_agent(&self) -> Option<String> {
        lock(&self.selected_agent).clone()
    }

    pub fn is_polling(&self) -> bool {
        self.polling.load(Ordering::SeqCst)
    }

    pub fn set_polling(&self, on: bool) {
        self.polling.store(on, Ordering::SeqCst);
    }

    pub fn watcher_mode(&self) -> &'static str {
        if self.is_polling() {
            "polling"
        } else {
            "watching"
        }
    }
}

/// Builds the runtime, manages it as Tauri state and starts the loop thread.
pub fn start(app: &AppHandle) -> tauri::Result<()> {
    let settings_dir = app.path().app_data_dir()?;
    let settings = settings::load(&settings_dir);
    let claude_dir =
        settings::resolve_claude_dir(&settings, std::env::var_os("HOME").map(PathBuf::from));
    let rt = Arc::new(Runtime::new(
        claude_dir,
        settings_dir,
        settings,
        Box::new(SystemProbe),
    ));
    app.manage(rt.clone());

    let (tx, rx) = mpsc::channel();
    // Watch before loading so nothing that changes during the load is missed.
    let watch = try_watch(&rt, &tx);
    rt.init(Utc::now());
    eprintln!(
        "agent-viewer: {} agents from {} ({})",
        rt.list_agents().len(),
        rt.claude_dir().display(),
        rt.watcher_mode()
    );

    let app = app.clone();
    std::thread::Builder::new()
        .name("agent-viewer-runtime".into())
        .spawn(move || run_loop(&app, &rt, &tx, &rx, watch))?;
    Ok(())
}

/// Spawns the watcher; on failure (or a missing claude dir, which is never
/// created) switches to polling. Clears polling on success.
pub fn try_watch(rt: &Runtime, tx: &Sender<Input>) -> Option<WatchHandle> {
    let handle = if rt.claude_dir().is_dir() {
        spawn_watcher(rt.claude_dir(), tx.clone()).ok()
    } else {
        None
    };
    rt.set_polling(handle.is_none());
    handle
}

fn emit(app: &AppHandle, changes: &[Change]) {
    if !changes.is_empty() {
        let _ = app.emit(CHANGED_EVENT, changes);
    }
}

fn run_loop(
    app: &AppHandle,
    rt: &Runtime,
    tx: &Sender<Input>,
    rx: &Receiver<Input>,
    mut watch: Option<WatchHandle>,
) {
    let mut next_tick = Instant::now() + TICK;
    let mut next_rescan = Instant::now() + RESCAN;
    loop {
        let timeout = next_tick.saturating_duration_since(Instant::now());
        match rx.recv_timeout(timeout) {
            Ok(input) => emit(app, &rt.apply(input, Utc::now())),
            Err(RecvTimeoutError::Timeout) => {}
            // Unreachable while we hold `tx`; stop cleanly anyway.
            Err(RecvTimeoutError::Disconnected) => return,
        }
        // Tick on timeout, and also when a steady input stream keeps the
        // channel busy past the tick deadline.
        if Instant::now() < next_tick {
            continue;
        }
        next_tick = Instant::now() + TICK;
        emit(app, &rt.apply(Input::Tick, Utc::now()));

        if watch.as_ref().is_some_and(WatchHandle::failed) {
            watch = None;
            rt.set_polling(true);
            next_rescan = Instant::now();
        }
        if rt.is_polling() && Instant::now() >= next_rescan {
            next_rescan = Instant::now() + RESCAN;
            watch = try_watch(rt, tx);
            emit(app, &rt.apply(Input::Rescan, Utc::now()));
        }
    }
}
