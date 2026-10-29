use crate::transcript::TranscriptSummary;
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

/// A busy session whose pending tool call is older than this may be waiting on the user.
pub const MAYBE_WAITING_AFTER_SECS: i64 = 5;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum AgentStatus {
    Working,
    MaybeWaiting,
    Idle,
    Error,
    Exited,
    Unknown(String),
}

impl AgentStatus {
    pub fn label(&self) -> &str {
        match self {
            AgentStatus::Working => "working",
            AgentStatus::MaybeWaiting => "waiting?",
            AgentStatus::Idle => "idle",
            AgentStatus::Error => "error",
            AgentStatus::Exited => "exited",
            AgentStatus::Unknown(s) => s,
        }
    }
}

/// First match wins: dead, errored, idle, busy-with-stale-pending-tool, busy, otherwise unknown.
pub fn derive_status(
    registry_status: &str,
    alive: bool,
    summary: &TranscriptSummary,
    now: DateTime<Utc>,
) -> AgentStatus {
    if !alive {
        return AgentStatus::Exited;
    }
    if summary.last_is_error {
        return AgentStatus::Error;
    }
    match registry_status {
        "idle" => AgentStatus::Idle,
        "busy" => match &summary.pending_tool {
            Some(p) if now - p.since > Duration::seconds(MAYBE_WAITING_AFTER_SECS) => {
                AgentStatus::MaybeWaiting
            }
            _ => AgentStatus::Working,
        },
        other => AgentStatus::Unknown(other.to_string()),
    }
}
