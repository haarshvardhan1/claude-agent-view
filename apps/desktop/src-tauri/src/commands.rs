//! Tauri commands: thin wrappers over [`Runtime`] methods so the logic is
//! testable without a window.

use crate::runtime::{HookReport, Runtime, publish};
use crate::settings::AppSettings;
use av_core::hooks::install::HooksStatus;
use av_core::model::{Agent, TranscriptPage};
use av_core::transcript_view::PageRequest;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::plugin::PermissionState;
use tauri::{AppHandle, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_notification::NotificationExt;

/// Every command error reaches the UI as `{ code, message }`.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct CmdError {
    pub code: String,
    pub message: String,
}

impl CmdError {
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
        }
    }

    pub fn io(e: std::io::Error) -> Self {
        Self::new("io", e.to_string())
    }
}

/// Which transcript page to read: `{ "kind": "tail" }`,
/// `{ "kind": "before", "offset": n }` or `{ "kind": "after", "offset": n }`.
#[derive(Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PageReq {
    Tail,
    Before { offset: u64 },
    After { offset: u64 },
}

impl From<PageReq> for PageRequest {
    fn from(r: PageReq) -> Self {
        match r {
            PageReq::Tail => PageRequest::Tail,
            PageReq::Before { offset } => PageRequest::Before(offset),
            PageReq::After { offset } => PageRequest::After(offset),
        }
    }
}

type Rt<'a> = State<'a, Arc<Runtime>>;

#[tauri::command]
pub fn list_agents(rt: Rt<'_>) -> Result<Vec<Agent>, CmdError> {
    Ok(rt.list_agents())
}

#[tauri::command]
pub fn get_transcript(
    rt: Rt<'_>,
    agent_id: String,
    subagent_id: Option<String>,
    req: PageReq,
) -> Result<TranscriptPage, CmdError> {
    rt.get_transcript(&agent_id, subagent_id.as_deref(), req)
}

#[tauri::command]
pub fn get_settings(rt: Rt<'_>) -> Result<AppSettings, CmdError> {
    Ok(rt.settings())
}

/// Applies `launchAtLogin` through autostart first (nothing is saved if
/// that fails), then saves the settings.
#[tauri::command]
pub fn set_settings(app: AppHandle, rt: Rt<'_>, settings: AppSettings) -> Result<(), CmdError> {
    apply_launch_at_login(&app, settings.launch_at_login)?;
    rt.set_settings(settings)
}

fn apply_launch_at_login(app: &AppHandle, on: bool) -> Result<(), CmdError> {
    let autostart = app.autolaunch();
    // If the state can't be read, act anyway.
    if autostart.is_enabled().ok() == Some(on) {
        return Ok(());
    }
    let res = if on {
        autostart.enable()
    } else {
        autostart.disable()
    };
    res.map_err(|e| CmdError::new("autostart", e.to_string()))
}

/// `"granted" | "denied" | "unknown"`, from the notification plugin.
pub fn permission_name(state: Option<PermissionState>) -> &'static str {
    match state {
        Some(PermissionState::Granted) => "granted",
        Some(PermissionState::Denied) => "denied",
        _ => "unknown",
    }
}

#[tauri::command]
pub fn notification_permission(app: AppHandle) -> Result<String, CmdError> {
    let state = app.notification().permission_state().ok();
    Ok(permission_name(state).to_string())
}

#[tauri::command]
pub fn set_selected_agent(rt: Rt<'_>, id: Option<String>) -> Result<(), CmdError> {
    rt.set_selected_agent(id);
    Ok(())
}

#[tauri::command]
pub fn watcher_mode(rt: Rt<'_>) -> Result<String, CmdError> {
    Ok(rt.watcher_mode().to_string())
}

#[tauri::command]
pub fn hooks_status(rt: Rt<'_>) -> Result<HooksStatus, CmdError> {
    Ok(rt.hooks_status())
}

/// Also serves as Repair.
#[tauri::command]
pub fn install_hooks(app: AppHandle, rt: Rt<'_>) -> Result<HookReport, CmdError> {
    let (report, published) = rt.install_hooks(chrono::Utc::now())?;
    publish(&app, &rt, published);
    Ok(report)
}

#[tauri::command]
pub fn uninstall_hooks(app: AppHandle, rt: Rt<'_>) -> Result<HookReport, CmdError> {
    let (report, published) = rt.uninstall_hooks(chrono::Utc::now())?;
    publish(&app, &rt, published);
    Ok(report)
}
