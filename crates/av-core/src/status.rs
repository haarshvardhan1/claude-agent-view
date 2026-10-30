use crate::model::Status;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookSignal {
    Permission { tool: Option<String> },
    WaitingInput,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveHook {
    pub signal: HookSignal,
    pub at: DateTime<Utc>,
}

pub struct StatusInputs<'a> {
    pub alive: bool,
    pub registry_status: &'a str,
    pub summary: &'a TranscriptSummary,
    pub hook: Option<&'a ActiveHook>,
    pub stopped_at: Option<DateTime<Utc>>,
    pub hooks_installed: bool,
    pub now: DateTime<Utc>,
}

/// Priority, first match wins: exited, needsPermission, error, waitingInput,
/// maybeWaiting (hooks not installed only), working, idle, unknown.
pub fn derive_status_v2(i: &StatusInputs) -> Status {
    if !i.alive {
        return Status::Exited;
    }
    if let Some(ActiveHook {
        signal: HookSignal::Permission { tool },
        ..
    }) = i.hook
    {
        return Status::NeedsPermission { tool: tool.clone() };
    }
    if i.summary.last_is_error {
        return Status::Error {
            kind: i.summary.last_error_kind.clone(),
        };
    }
    let hook_waiting = matches!(
        i.hook,
        Some(ActiveHook {
            signal: HookSignal::WaitingInput,
            ..
        })
    );
    let idle_done = i.registry_status == "idle"
        && (i.summary.turn_ended_at.is_some() || i.stopped_at.is_some());
    if hook_waiting || idle_done {
        return Status::WaitingInput;
    }
    match i.registry_status {
        "idle" => Status::Idle,
        "busy" => match &i.summary.pending_tool {
            Some(p)
                if !i.hooks_installed
                    && p.name != "Agent"
                    && i.now - p.since > Duration::seconds(MAYBE_WAITING_AFTER_SECS) =>
            {
                Status::MaybeWaiting
            }
            _ => Status::Working,
        },
        other => Status::Unknown {
            reason: other.to_string(),
        },
    }
}
