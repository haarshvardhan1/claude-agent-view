//! Pure builders from internal data to the IPC `model::Agent`.

use crate::model::{self, Context, PendingToolView, Subagent, SubagentState, Tokens};
use crate::registry::SessionEntry;
use crate::subagents::{SubagentInfo, SubagentStatus};
use crate::transcript::{PendingTool, TokenTotals, TranscriptSummary};
use chrono::{DateTime, TimeZone, Utc};
use std::path::Path;

/// Context window size: `None` for an unknown model, 1M for `[1m]` models or
/// when any call exceeded 200k, else 200k.
pub fn context_limit(model: Option<&str>, max_used: u64) -> Option<u64> {
    let model = model?;
    if model.ends_with("[1m]") || max_used > 200_000 {
        Some(1_000_000)
    } else {
        Some(200_000)
    }
}

fn non_blank(s: &str) -> Option<&str> {
    (!s.trim().is_empty()).then_some(s)
}

/// agent name > registry name > transcript title > "(untitled)".
pub fn pick_title(summary: &TranscriptSummary, registry_name: &str) -> String {
    summary
        .agent_name
        .as_deref()
        .and_then(non_blank)
        .or_else(|| non_blank(registry_name))
        .or_else(|| summary.title.as_deref().and_then(non_blank))
        .unwrap_or("(untitled)")
        .to_string()
}

/// Last non-empty path segment, or `cwd` itself.
pub fn project_name(cwd: &str) -> String {
    cwd.split('/')
        .rfind(|s| !s.is_empty())
        .unwrap_or(cwd)
        .to_string()
}

fn tokens(t: &TokenTotals) -> Tokens {
    Tokens {
        input: t.input,
        output: t.output,
        cache_write: t.cache_write,
        cache_read: t.cache_read,
    }
}

fn pending(p: &Option<PendingTool>) -> Option<PendingToolView> {
    p.as_ref().map(|p| PendingToolView {
        name: p.name.clone(),
        since: p.since,
    })
}

pub fn build_subagent(info: &SubagentInfo, transcript_path: &Path) -> Subagent {
    Subagent {
        id: info.agent_id.clone(),
        agent_type: info.agent_type.clone(),
        description: info.description.clone(),
        background: info.background,
        status: match info.status {
            SubagentStatus::Running => SubagentState::Running,
            SubagentStatus::Done => SubagentState::Done,
            SubagentStatus::Stopped => SubagentState::Stopped,
        },
        last_activity: info.summary.last_activity,
        tokens: tokens(&info.summary.tokens),
        pending_tool: pending(&info.summary.pending_tool),
        transcript_path: transcript_path.display().to_string(),
    }
}

pub fn build_agent(
    entry: &SessionEntry,
    summary: &TranscriptSummary,
    subagents: Vec<Subagent>,
    status: model::Status,
    status_since: DateTime<Utc>,
    transcript_path: Option<&Path>,
) -> model::Agent {
    model::Agent {
        id: entry.session_id.clone(),
        pid: entry.pid,
        cwd: entry.cwd.clone(),
        project_name: project_name(&entry.cwd),
        git_branch: summary.git_branch.clone(),
        title: pick_title(summary, &entry.name),
        entrypoint: entry.entrypoint.clone(),
        model: summary.model.clone(),
        status,
        status_since,
        started_at: Utc
            .timestamp_millis_opt(entry.started_at_ms)
            .single()
            .unwrap_or_default(),
        last_activity: summary.last_activity,
        context: Context {
            used: summary.context_used,
            limit: context_limit(summary.model.as_deref(), summary.max_context_used),
        },
        tokens: tokens(&summary.tokens),
        pending_tool: pending(&summary.pending_tool),
        subagents,
        transcript_path: transcript_path.map(|p| p.display().to_string()),
        skipped_lines: summary.skipped_lines,
    }
}
