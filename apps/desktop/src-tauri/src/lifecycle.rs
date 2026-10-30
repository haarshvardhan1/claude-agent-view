//! Window lifecycle: hide-on-close, Dock reopen, focus tracking, the settings
//! window, and selecting the agent of a recent notification on activation.
//!
//! The rules are pure functions ([`take_select`], [`focused_agent`]) so they
//! are testable without a window.

use crate::runtime::Runtime;
use chrono::{DateTime, Duration, Utc};
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

pub const MAIN_LABEL: &str = "main";
pub const SETTINGS_LABEL: &str = "settings";
pub const SELECT_AGENT_EVENT: &str = "ui://select-agent";
/// Activation within this many seconds of a notification selects its agent.
pub const ACTIVATION_WINDOW_SECS: i64 = 60;

/// The most recent notification sent: which agent, and when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LastNotified {
    pub agent_id: String,
    pub at: DateTime<Utc>,
}

/// On app activation: the agent to select if the last notification was sent
/// at most 60 s before `now`. Always clears the slot, so it is used once.
pub fn take_select(slot: &mut Option<LastNotified>, now: DateTime<Utc>) -> Option<String> {
    let last = slot.take()?;
    (now - last.at <= Duration::seconds(ACTIVATION_WINDOW_SECS)).then_some(last.agent_id)
}

/// The agent the user is looking at: the selected agent, but only while the
/// main window is both visible and focused. Passed to the notifier.
pub fn focused_agent(selected: Option<&str>, focused: bool, visible: bool) -> Option<String> {
    if focused && visible {
        selected.map(str::to_string)
    } else {
        None
    }
}

fn runtime(app: &AppHandle) -> Option<Arc<Runtime>> {
    app.try_state::<Arc<Runtime>>().map(|s| s.inner().clone())
}

/// Shows, unminimizes and focuses the main window.
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN_LABEL) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        if let Some(rt) = runtime(app) {
            rt.set_main_visible(true);
        }
    }
}

/// Shows the main window and asks the UI to select `agent_id`.
pub fn select_agent(app: &AppHandle, agent_id: &str) {
    show_main(app);
    let _ = app.emit_to(MAIN_LABEL, SELECT_AGENT_EVENT, agent_id.to_string());
}

/// The app was activated (main window focused, or Dock reopen): select the
/// agent of a notification sent in the last 60 s, once.
pub fn on_activated(app: &AppHandle) {
    let Some(rt) = runtime(app) else { return };
    if let Some(id) = rt.take_activation_select(Utc::now()) {
        select_agent(app, &id);
    }
}

/// Main window `CloseRequested`: hide instead of closing.
pub fn on_main_close_requested(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN_LABEL) {
        let _ = w.hide();
    }
    if let Some(rt) = runtime(app) {
        rt.set_main_visible(false);
        rt.set_main_focused(false);
    }
}

/// Main window `Focused(focused)`.
pub fn on_main_focus(app: &AppHandle, focused: bool) {
    if let Some(rt) = runtime(app) {
        rt.set_main_focused(focused);
        if focused {
            rt.set_main_visible(true);
        }
    }
    if focused {
        on_activated(app);
    }
}

/// Opens the settings window, or focuses it if already open. Closing it
/// closes it (it is not hidden like the main window).
pub fn open_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(SETTINGS_LABEL) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let built = WebviewWindowBuilder::new(
        app,
        SETTINGS_LABEL,
        WebviewUrl::App("index.html#settings".into()),
    )
    .title("Settings")
    .inner_size(560.0, 520.0)
    .resizable(false)
    .build();
    match built {
        Ok(w) => {
            let _ = w.set_focus();
        }
        Err(e) => eprintln!("agent-viewer: cannot open settings window: {e}"),
    }
}
