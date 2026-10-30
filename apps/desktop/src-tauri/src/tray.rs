//! Menu bar icon: needs-you count as the title, and a menu of needs-you and
//! busy agents. [`tray_model`] is pure; [`rebuild_tray`] applies it and skips
//! the work when the model hasn't changed since the last rebuild.

use crate::lifecycle;
use av_core::model::Agent;
use std::sync::Mutex;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Wry};

pub const TRAY_ID: &str = "agent-viewer-tray";
pub const MAX_PER_GROUP: usize = 10;
pub const TITLE_CHARS: usize = 40;

/// Menu item ids. Agent items are `agent:<id>`.
pub const AGENT_PREFIX: &str = "agent:";
pub const OPEN_ID: &str = "tray-open";
pub const TRAY_SETTINGS_ID: &str = "tray-settings";
pub const APP_SETTINGS_ID: &str = "app-settings";
pub const QUIT_ID: &str = "tray-quit";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayItem {
    pub id: String,
    pub label: String,
}

/// `(title, needs-you items, busy items)`.
pub type TrayModel = (Option<String>, Vec<TrayItem>, Vec<TrayItem>);

fn item(a: &Agent) -> TrayItem {
    let title: String = a.title.chars().take(TITLE_CHARS).collect();
    TrayItem {
        id: a.id.clone(),
        label: format!("● {title} — {}", a.project_name),
    }
}

fn group(agents: &[Agent], pred: impl Fn(&Agent) -> bool) -> Vec<TrayItem> {
    let mut v: Vec<&Agent> = agents.iter().filter(|a| pred(a)).collect();
    // Most recent activity first; no activity last; id breaks ties.
    v.sort_by(|a, b| {
        b.last_activity
            .cmp(&a.last_activity)
            .then_with(|| a.id.cmp(&b.id))
    });
    v.into_iter().take(MAX_PER_GROUP).map(item).collect()
}

/// Title = needs-you count (`None` at 0). Needs-you agents, then busy agents
/// that don't need you, at most 10 each, by `last_activity` descending.
pub fn tray_model(agents: &[Agent]) -> TrayModel {
    let count = agents.iter().filter(|a| a.status.needs_you()).count();
    let title = (count > 0).then(|| count.to_string());
    let needs = group(agents, |a| a.status.needs_you());
    let busy = group(agents, |a| a.status.is_busy() && !a.status.needs_you());
    (title, needs, busy)
}

/// The last model applied to the tray, so unchanged updates are skipped.
#[derive(Default)]
pub struct TrayState {
    last: Mutex<Option<TrayModel>>,
}

impl TrayState {
    /// Records `model` as applied and returns `true` if it differs from the
    /// last one. The lock is released on return: the caller builds and sets
    /// the menu (which marshals onto the main thread) without holding it.
    pub fn claim(&self, model: &TrayModel) -> bool {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if last.as_ref() == Some(model) {
            return false;
        }
        *last = Some(model.clone());
        true
    }

    /// Forgets `model` if it is still the recorded one (its apply failed), so
    /// the next rebuild retries.
    pub fn forget(&self, model: &TrayModel) {
        let mut last = self.last.lock().unwrap_or_else(|e| e.into_inner());
        if last.as_ref() == Some(model) {
            *last = None;
        }
    }
}

fn build_menu(app: &AppHandle, model: &TrayModel) -> tauri::Result<Menu<Wry>> {
    let menu = Menu::new(app)?;
    let groups = [("Needs you", &model.1), ("Working", &model.2)];
    for (header, items) in groups {
        if items.is_empty() {
            continue;
        }
        menu.append(&MenuItem::new(app, header, false, None::<&str>)?)?;
        for it in items {
            let id = format!("{AGENT_PREFIX}{}", it.id);
            menu.append(&MenuItem::with_id(app, id, &it.label, true, None::<&str>)?)?;
        }
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        OPEN_ID,
        "Open Agent Viewer",
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        TRAY_SETTINGS_ID,
        "Settings…",
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(
        app,
        QUIT_ID,
        "Quit",
        true,
        None::<&str>,
    )?)?;
    Ok(menu)
}

/// Creates the menu bar icon (empty model). Call once from setup.
pub fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    app.manage(TrayState::default());
    let model: TrayModel = (None, vec![], vec![]);
    let menu = build_menu(app, &model)?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(include_bytes!(
            "../icons/tray-template.png"
        ))?)
        .icon_as_template(true)
        .tooltip("Agent Viewer")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .build(app)?;
    eprintln!("agent-viewer: tray built");
    Ok(())
}

/// Applies `tray_model(agents)` to the tray: title and menu. Does nothing if
/// the model equals the last one applied.
pub fn rebuild_tray(app: &AppHandle, agents: &[Agent]) {
    let Some(state) = app.try_state::<TrayState>() else {
        return;
    };
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    let model = tray_model(agents);
    // Compare and record under the lock, then build/set without it: those
    // calls block on the main thread, which may itself be waiting on us.
    if !state.claim(&model) {
        return;
    }
    let _ = tray.set_title(model.0.as_deref());
    match build_menu(app, &model) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
        }
        Err(e) => {
            state.forget(&model);
            eprintln!("agent-viewer: tray menu rebuild failed: {e}");
        }
    }
}

/// The default app menu with `Settings…` (⌘,) after `About` in the app submenu.
pub fn app_menu(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let menu = Menu::default(app)?;
    let settings = MenuItem::with_id(app, APP_SETTINGS_ID, "Settings…", true, Some("CmdOrCtrl+,"))?;
    if let Some(sub) = menu
        .items()?
        .into_iter()
        .next()
        .and_then(|i| i.as_submenu().cloned())
    {
        insert_settings(&sub, &settings, app)?;
    } else {
        menu.append(&settings)?;
    }
    Ok(menu)
}

fn insert_settings(sub: &Submenu<Wry>, item: &MenuItem<Wry>, app: &AppHandle) -> tauri::Result<()> {
    // macOS app submenu: About, separator, Settings…, separator, Services, …
    sub.insert(item, 2)?;
    sub.insert(&PredefinedMenuItem::separator(app)?, 3)?;
    Ok(())
}

/// Handles clicks on tray and app menu items.
pub fn on_menu_event(app: &AppHandle, id: &str) {
    if let Some(agent_id) = id.strip_prefix(AGENT_PREFIX) {
        lifecycle::select_agent(app, agent_id);
        return;
    }
    match id {
        OPEN_ID => lifecycle::show_main(app),
        TRAY_SETTINGS_ID | APP_SETTINGS_ID => lifecycle::open_settings(app),
        QUIT_ID => app.exit(0),
        _ => {}
    }
}
