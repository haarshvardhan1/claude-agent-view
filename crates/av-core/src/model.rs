//! Frozen IPC model types consumed by the desktop app.
//!
//! All structs serialize with camelCase keys. Enums are internally tagged:
//! `Status` and `TranscriptEntry` use `"kind"`, `Change` uses `"type"`.

use chrono::{DateTime, Utc};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: String,
    pub pid: u32,
    pub cwd: String,
    pub project_name: String,
    pub git_branch: Option<String>,
    pub title: String,
    pub entrypoint: String,
    pub model: Option<String>,
    pub status: Status,
    pub status_since: DateTime<Utc>,
    pub started_at: DateTime<Utc>,
    pub last_activity: Option<DateTime<Utc>>,
    pub context: Context,
    pub tokens: Tokens,
    pub pending_tool: Option<PendingToolView>,
    pub subagents: Vec<Subagent>,
    pub transcript_path: Option<String>,
    pub skipped_lines: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Subagent {
    pub id: String,
    pub agent_type: String,
    pub description: String,
    pub background: bool,
    pub status: SubagentState,
    pub last_activity: Option<DateTime<Utc>>,
    pub tokens: Tokens,
    pub pending_tool: Option<PendingToolView>,
    pub transcript_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SubagentState {
    Running,
    Done,
    Stopped,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Status {
    Exited,
    NeedsPermission { tool: Option<String> },
    Error {
        /// Serialized as `errorKind`: serde forbids a field named `kind` under the `"kind"` tag.
        #[serde(rename = "errorKind")]
        kind: Option<String>,
    },
    WaitingInput,
    MaybeWaiting,
    Working,
    Idle,
    Unknown { reason: String },
}

impl Status {
    /// Statuses that need the user's attention.
    pub fn needs_you(&self) -> bool {
        matches!(
            self,
            Status::NeedsPermission { .. } | Status::WaitingInput | Status::Error { .. }
        )
    }

    /// Statuses where the session is actively doing (or blocked on) work.
    pub fn is_busy(&self) -> bool {
        matches!(
            self,
            Status::Working | Status::MaybeWaiting | Status::NeedsPermission { .. }
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    pub used: Option<u64>,
    pub limit: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_write: u64,
    pub cache_read: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingToolView {
    pub name: String,
    pub since: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Change {
    AgentAdded { agent: Agent },
    AgentUpdated { agent: Agent },
    AgentRemoved { id: String },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum TranscriptEntry {
    UserPrompt {
        text: String,
        timestamp: Option<DateTime<Utc>>,
    },
    AssistantText {
        text: String,
        timestamp: Option<DateTime<Utc>>,
    },
    Thinking {
        text: String,
        timestamp: Option<DateTime<Utc>>,
    },
    ToolCall {
        id: String,
        name: String,
        summary: String,
        input: String,
        result: Option<String>,
        is_error: bool,
        timestamp: Option<DateTime<Utc>>,
    },
    /// A tool result whose call is not in the same page.
    ToolResult {
        tool_use_id: String,
        result: String,
        is_error: bool,
        timestamp: Option<DateTime<Utc>>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptPage {
    pub entries: Vec<TranscriptEntry>,
    pub start_offset: u64,
    pub end_offset: u64,
}
