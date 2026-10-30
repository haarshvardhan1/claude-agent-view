//! Tauri commands: thin wrappers over [`Runtime`] methods so the logic is
//! testable without a window.

use crate::runtime::Runtime;
use crate::settings::AppSettings;
use av_core::model::{Agent, TranscriptPage};
use av_core::transcript_view::PageRequest;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

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

#[tauri::command]
pub fn set_settings(rt: Rt<'_>, settings: AppSettings) -> Result<(), CmdError> {
    rt.set_settings(settings)
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
