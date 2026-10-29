//! Lists a session's subagents (`<session>/subagents/agent-<id>.jsonl`) with status (read-only).

use crate::paths::subagents_dir;
use crate::transcript::{summarize_transcript, TranscriptSummary};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum SubagentStatus {
    Running,
    Done,
    Stopped,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubagentInfo {
    pub agent_id: String,
    pub agent_type: String,
    pub description: String,
    pub background: bool,
    pub status: SubagentStatus,
    pub summary: TranscriptSummary,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    agent_type: Option<String>,
    description: Option<String>,
    request_shape: Option<String>,
}

/// One entry per `agent-<id>.jsonl`, newest `last_activity` first (None last).
/// Unreadable transcripts are skipped; unreadable/corrupt meta is treated as missing.
pub fn read_subagents(transcript: &Path, parent_alive: bool) -> Vec<SubagentInfo> {
    let Ok(rd) = fs::read_dir(subagents_dir(transcript)) else {
        return Vec::new();
    };
    let mut out: Vec<SubagentInfo> = rd
        .filter_map(Result::ok)
        .filter_map(|e| {
            let path = e.path();
            let name = path.file_name()?.to_str()?;
            let id = name.strip_prefix("agent-")?.strip_suffix(".jsonl")?.to_string();
            let summary = summarize_transcript(&path).ok()?;
            let meta: Option<Meta> = fs::read_to_string(path.with_file_name(format!("agent-{id}.meta.json")))
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok());
            let done = summary.handed_back || summary.last_stop_reason.as_deref() == Some("end_turn");
            let status = if done {
                SubagentStatus::Done
            } else if parent_alive {
                SubagentStatus::Running
            } else {
                SubagentStatus::Stopped
            };
            Some(SubagentInfo {
                agent_type: meta.as_ref().and_then(|m| m.agent_type.clone()).unwrap_or_else(|| "unknown".into()),
                description: meta.as_ref().and_then(|m| m.description.clone()).unwrap_or_else(|| id.clone()),
                background: meta.as_ref().and_then(|m| m.request_shape.as_deref()) == Some("background"),
                agent_id: id,
                status,
                summary,
            })
        })
        .collect();
    out.sort_by(|a, b| b.summary.last_activity.cmp(&a.summary.last_activity));
    out
}
