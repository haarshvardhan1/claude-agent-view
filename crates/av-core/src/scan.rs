use crate::liveness::{is_alive, ProcessProbe};
use crate::paths::find_transcript;
use crate::registry::{read_registry, SessionEntry};
use crate::status::{derive_status, AgentStatus};
use crate::subagents::{read_subagents, SubagentInfo};
use crate::transcript::{summarize_transcript, TranscriptSummary};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct Agent {
    pub entry: SessionEntry,
    pub alive: bool,
    pub status: AgentStatus,
    pub title: String,
    pub summary: TranscriptSummary,
    pub subagents: Vec<SubagentInfo>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanResult {
    pub agents: Vec<Agent>,
    pub warnings: Vec<String>,
}

/// Discover all sessions under `claude_dir`. Transcript problems become warnings; the agent is still listed.
pub fn scan(claude_dir: &Path, probe: &dyn ProcessProbe, now: DateTime<Utc>) -> ScanResult {
    let (entries, mut warnings) = read_registry(claude_dir);
    let mut agents = Vec::with_capacity(entries.len());
    for entry in entries {
        let alive = is_alive(&entry, probe);
        let mut summary = TranscriptSummary::default();
        let mut subagents = Vec::new();
        if let Some(path) = find_transcript(claude_dir, &entry.cwd, &entry.session_id) {
            match summarize_transcript(&path) {
                Ok(s) => summary = s,
                Err(e) => warnings.push(format!(
                    "session {}: cannot read transcript {}: {e}",
                    entry.session_id,
                    path.display()
                )),
            }
            subagents = read_subagents(&path, alive);
        }
        let status = derive_status(&entry.status, alive, &summary, now);
        let title = summary.title.clone().unwrap_or_else(|| "(new session)".to_string());
        agents.push(Agent { entry, alive, status, title, summary, subagents });
    }
    ScanResult { agents, warnings }
}
