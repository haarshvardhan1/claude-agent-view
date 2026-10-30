//! Notification rules engine: turns engine `Change` lists into notifications.
//!
//! Pure logic. The desktop shell calls `on_changes` and sends the results.

use crate::model::{Agent, Change, Status, SubagentState};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::mem::discriminant;

const RATE_LIMIT_SECS: i64 = 10;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct NotifySettings {
    pub needs_permission: bool,
    pub waiting_input: bool,
    pub finished: bool,
    pub error: bool,
    pub subagent_finished: bool,
    pub finished_after_secs: u64,
}

impl Default for NotifySettings {
    fn default() -> Self {
        Self {
            needs_permission: true,
            waiting_input: true,
            finished: true,
            error: true,
            subagent_finished: true,
            finished_after_secs: 30,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NotifyKind {
    NeedsPermission,
    WaitingInput,
    Finished,
    Error,
    SubagentFinished,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Notification {
    pub agent_id: String,
    pub kind: NotifyKind,
    pub title: String,
    pub body: String,
}

struct AgentState {
    /// Last non-Unknown status (Unknown is transparent).
    status: Status,
    busy_since: Option<DateTime<Utc>>,
    subagents: HashMap<String, SubagentState>,
}

#[derive(Default)]
pub struct Notifier {
    agents: HashMap<String, AgentState>,
    last_fired: HashMap<(String, NotifyKind), DateTime<Utc>>,
}

fn subagent_map(agent: &Agent) -> HashMap<String, SubagentState> {
    agent
        .subagents
        .iter()
        .map(|s| (s.id.clone(), s.status))
        .collect()
}

impl Notifier {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_changes(
        &mut self,
        changes: &[Change],
        settings: &NotifySettings,
        focused_agent: Option<&str>,
        now: DateTime<Utc>,
    ) -> Vec<Notification> {
        let mut out = Vec::new();
        for change in changes {
            match change {
                Change::AgentAdded { agent } => self.record(agent, now),
                Change::AgentRemoved { id } => {
                    self.agents.remove(id);
                    self.last_fired.retain(|(a, _), _| a != id);
                }
                Change::AgentUpdated { agent } => {
                    if !self.agents.contains_key(&agent.id) {
                        self.record(agent, now);
                        continue;
                    }
                    for (kind, body) in self.candidates(agent, settings, now) {
                        if let Some(n) = self.admit(agent, kind, body, settings, focused_agent, now)
                        {
                            out.push(n);
                        }
                    }
                }
            }
        }
        out
    }

    fn record(&mut self, agent: &Agent, now: DateTime<Utc>) {
        let busy = agent.status.is_busy();
        let prev = self.agents.get(&agent.id);
        let status = match (&agent.status, prev) {
            (Status::Unknown { .. }, Some(p)) => p.status.clone(),
            (s, _) => s.clone(),
        };
        let busy = if matches!(agent.status, Status::Unknown { .. }) {
            status.is_busy()
        } else {
            busy
        };
        let busy_since = if busy {
            prev.and_then(|p| p.busy_since).or(Some(now))
        } else {
            None
        };
        self.agents.insert(
            agent.id.clone(),
            AgentState {
                status,
                busy_since,
                subagents: subagent_map(agent),
            },
        );
    }

    /// Computes (kind, body) candidates for an update and advances the state.
    fn candidates(
        &mut self,
        agent: &Agent,
        settings: &NotifySettings,
        now: DateTime<Utc>,
    ) -> Vec<(NotifyKind, String)> {
        let mut c = Vec::new();
        let state = self.agents.get_mut(&agent.id).expect("checked by caller");

        // Subagents: background Running -> Done.
        for sub in &agent.subagents {
            if sub.background
                && sub.status == SubagentState::Done
                && state.subagents.get(&sub.id) == Some(&SubagentState::Running)
            {
                c.push((
                    NotifyKind::SubagentFinished,
                    format!("Subagent finished: {}", sub.description),
                ));
            }
        }
        state.subagents = subagent_map(agent);

        let new = &agent.status;
        if matches!(new, Status::Unknown { .. }) {
            return c;
        }
        let old = state.status.clone();
        let busy_since = state.busy_since;
        // Advance state.
        if new.is_busy() {
            if !old.is_busy() {
                state.busy_since = Some(now);
            }
        } else {
            state.busy_since = None;
        }
        state.status = new.clone();

        if discriminant(&old) == discriminant(new) {
            return c;
        }
        match new {
            Status::NeedsPermission { tool } => c.push((
                NotifyKind::NeedsPermission,
                match tool {
                    Some(t) => format!("Needs permission: {t}"),
                    None => "Needs permission".to_string(),
                },
            )),
            Status::Error { kind } => c.push((
                NotifyKind::Error,
                match kind {
                    Some(k) => format!("API error: {k}"),
                    None => "API error".to_string(),
                },
            )),
            Status::Idle | Status::WaitingInput => {
                let busy_for = old
                    .is_busy()
                    .then(|| busy_since.map(|b| now - b))
                    .flatten()
                    .filter(|d| *d >= Duration::seconds(settings.finished_after_secs as i64));
                if let Some(d) = busy_for {
                    let secs = d.num_seconds();
                    c.push((
                        NotifyKind::Finished,
                        format!("Finished after {}m {}s", secs / 60, secs % 60),
                    ));
                } else if matches!(new, Status::WaitingInput) {
                    c.push((NotifyKind::WaitingInput, "Waiting for your input".into()));
                }
            }
            _ => {}
        }
        c
    }

    fn admit(
        &mut self,
        agent: &Agent,
        kind: NotifyKind,
        body: String,
        settings: &NotifySettings,
        focused_agent: Option<&str>,
        now: DateTime<Utc>,
    ) -> Option<Notification> {
        let enabled = match kind {
            NotifyKind::NeedsPermission => settings.needs_permission,
            NotifyKind::WaitingInput => settings.waiting_input,
            NotifyKind::Finished => settings.finished,
            NotifyKind::Error => settings.error,
            NotifyKind::SubagentFinished => settings.subagent_finished,
        };
        if !enabled || focused_agent == Some(agent.id.as_str()) {
            return None;
        }
        let recent = |k: NotifyKind| {
            self.last_fired
                .get(&(agent.id.clone(), k))
                .is_some_and(|t| now - *t < Duration::seconds(RATE_LIMIT_SECS))
        };
        if recent(kind) || (kind == NotifyKind::WaitingInput && recent(NotifyKind::Finished)) {
            return None;
        }
        self.last_fired.insert((agent.id.clone(), kind), now);
        Some(Notification {
            agent_id: agent.id.clone(),
            kind,
            title: agent.title.clone(),
            body,
        })
    }
}
