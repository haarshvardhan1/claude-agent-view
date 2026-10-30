//! Lists a session's subagents (`<session>/subagents/agent-<id>.jsonl`) with status (read-only).

use crate::paths::subagents_dir;
use crate::transcript::{summarize_transcript, TranscriptSummary};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

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

/// The subagent id of an `agent-<id>.jsonl` path.
fn subagent_id(jsonl: &Path) -> Option<&str> {
    jsonl
        .file_name()?
        .to_str()?
        .strip_prefix("agent-")?
        .strip_suffix(".jsonl")
}

/// `(agent-<id>.jsonl, agent-<id>.meta.json if present)` for every subagent
/// transcript under the session's subagents dir, in directory order.
pub fn subagent_files(transcript: &Path) -> Vec<(PathBuf, Option<PathBuf>)> {
    let Ok(rd) = fs::read_dir(subagents_dir(transcript)) else {
        return Vec::new();
    };
    rd.filter_map(Result::ok)
        .filter_map(|e| {
            let path = e.path();
            let id = subagent_id(&path)?.to_string();
            let meta = path.with_file_name(format!("agent-{id}.meta.json"));
            Some((path, meta.is_file().then_some(meta)))
        })
        .collect()
}

/// Builds one subagent's info from its summary; unreadable/corrupt meta is
/// treated as missing.
pub fn subagent_info(
    jsonl: &Path,
    meta: Option<&Path>,
    summary: TranscriptSummary,
    parent_alive: bool,
) -> SubagentInfo {
    let id = subagent_id(jsonl).unwrap_or_default().to_string();
    let meta: Option<Meta> = meta
        .and_then(|m| fs::read_to_string(m).ok())
        .and_then(|s| serde_json::from_str(&s).ok());
    let done = summary.handed_back || summary.last_stop_reason.as_deref() == Some("end_turn");
    let status = if done {
        SubagentStatus::Done
    } else if parent_alive {
        SubagentStatus::Running
    } else {
        SubagentStatus::Stopped
    };
    SubagentInfo {
        agent_type: meta
            .as_ref()
            .and_then(|m| m.agent_type.clone())
            .unwrap_or_else(|| "unknown".into()),
        description: meta
            .as_ref()
            .and_then(|m| m.description.clone())
            .unwrap_or_else(|| id.clone()),
        background: meta.as_ref().and_then(|m| m.request_shape.as_deref()) == Some("background"),
        agent_id: id,
        status,
        summary,
    }
}

/// One entry per `agent-<id>.jsonl`, newest `last_activity` first (None last).
/// Unreadable transcripts are skipped; unreadable/corrupt meta is treated as missing.
pub fn read_subagents(transcript: &Path, parent_alive: bool) -> Vec<SubagentInfo> {
    let mut out: Vec<SubagentInfo> = subagent_files(transcript)
        .into_iter()
        .filter_map(|(jsonl, meta)| {
            let summary = summarize_transcript(&jsonl).ok()?;
            Some(subagent_info(
                &jsonl,
                meta.as_deref(),
                summary,
                parent_alive,
            ))
        })
        .collect();
    out.sort_by_key(|o| std::cmp::Reverse(o.summary.last_activity));
    out
}
