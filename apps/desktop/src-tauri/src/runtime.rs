//! Background runtime: watcher -> engine -> `agents://changed` events, tray
//! refresh and notifications.
//!
//! One loop thread drives the engine. The engine sits behind a mutex so
//! commands can read it; locks are held only to compute, never while emitting
//! or reading transcript files.

use crate::commands::{CmdError, PageReq};
use crate::lifecycle::{self, LastNotified};
use crate::settings::{self, AppSettings};
use av_core::SystemProbe;
use av_core::engine::{Engine, Input};
use av_core::hooks::events::{events_path, trim_events_file};
use av_core::hooks::install::{HooksStatus, hooks_status};
use av_core::liveness::ProcessProbe;
use av_core::model::{Agent, Change, TranscriptPage};
use av_core::notifier::{Notification, Notifier};
use av_core::transcript_view::read_page;
use av_core::watch::{WatchHandle, spawn_watcher};
use chrono::{DateTime, Utc};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

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
    main_focused: AtomicBool,
    main_visible: AtomicBool,
    last_notified: Mutex<Option<LastNotified>>,
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
            main_focused: AtomicBool::new(false),
            // The main window starts shown.
            main_visible: AtomicBool::new(true),
            last_notified: Mutex::new(None),
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
        self.step(input, now).0
    }

    /// Like [`Runtime::apply`], also returning the notifications to send.
    /// Suppresses notifications for the agent the user is looking at.
    pub fn step(&self, input: Input, now: DateTime<Utc>) -> (Vec<Change>, Vec<Notification>) {
        let changes = lock(&self.engine).handle(input, now);
        if changes.is_empty() {
            return (changes, Vec::new());
        }
        let notify = lock(&self.settings).notify.clone();
        let focused = self.focused_agent();
        let notifications =
            lock(&self.notifier).on_changes(&changes, &notify, focused.as_deref(), now);
        (changes, notifications)
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

    /// Saves to disk, then updates memory, under one lock so concurrent calls
    /// can't leave disk and memory disagreeing. `claudeDir` applies on restart.
    pub fn set_settings(&self, s: AppSettings) -> Result<(), CmdError> {
        let mut current = lock(&self.settings);
        settings::save(&self.settings_dir, &s).map_err(CmdError::io)?;
        *current = s;
        Ok(())
    }

    pub fn set_selected_agent(&self, id: Option<String>) {
        *lock(&self.selected_agent) = id;
    }

    pub fn selected_agent(&self) -> Option<String> {
        lock(&self.selected_agent).clone()
    }

    pub fn set_main_focused(&self, on: bool) {
        self.main_focused.store(on, Ordering::SeqCst);
    }

    pub fn set_main_visible(&self, on: bool) {
        self.main_visible.store(on, Ordering::SeqCst);
    }

    /// The selected agent while the main window is visible and focused.
    pub fn focused_agent(&self) -> Option<String> {
        lifecycle::focused_agent(
            self.selected_agent().as_deref(),
            self.main_focused.load(Ordering::SeqCst),
            self.main_visible.load(Ordering::SeqCst),
        )
    }

    /// Remembers the agent of the notification just sent.
    pub fn note_notified(&self, agent_id: &str, now: DateTime<Utc>) {
        *lock(&self.last_notified) = Some(LastNotified {
            agent_id: agent_id.to_string(),
            at: now,
        });
    }

    /// On activation: the agent of a notification sent in the last 60 s,
    /// once (the slot is cleared).
    pub fn take_activation_select(&self, now: DateTime<Utc>) -> Option<String> {
        lifecycle::take_select(&mut lock(&self.last_notified), now)
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
    crate::tray::rebuild_tray(app, &rt.list_agents());
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

/// Emits a non-empty change batch, refreshes the tray and sends the
/// notifications.
fn publish(
    app: &AppHandle,
    rt: &Runtime,
    (changes, notifications): (Vec<Change>, Vec<Notification>),
) {
    if changes.is_empty() {
        return;
    }
    let _ = app.emit(CHANGED_EVENT, &changes);
    crate::tray::rebuild_tray(app, &rt.list_agents());
    for n in notifications {
        let sent = app
            .notification()
            .builder()
            .title(&n.title)
            .body(&n.body)
            .show();
        match sent {
            Ok(()) => rt.note_notified(&n.agent_id, Utc::now()),
            Err(e) => eprintln!("agent-viewer: notification failed: {e}"),
        }
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
            Ok(input) => publish(app, rt, rt.step(input, Utc::now())),
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
        publish(app, rt, rt.step(Input::Tick, Utc::now()));

        if watch.as_ref().is_some_and(WatchHandle::failed) {
            watch = None;
            rt.set_polling(true);
            next_rescan = Instant::now();
        }
        if rt.is_polling() && Instant::now() >= next_rescan {
            next_rescan = Instant::now() + RESCAN;
            watch = try_watch(rt, tx);
            publish(app, rt, rt.step(Input::Rescan, Utc::now()));
        }
    }
}
