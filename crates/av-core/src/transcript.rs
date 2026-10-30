//! Streaming summarizer for session transcripts (`*.jsonl`).
//!
//! Transcripts can be tens of MB, so the file is read one line at a time and
//! never held in memory as a whole.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

const TITLE_MAX_CHARS: usize = 80;
const SYNTHETIC_MODEL: &str = "<synthetic>";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct TokenTotals {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl TokenTotals {
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PendingTool {
    pub name: String,
    pub since: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TranscriptSummary {
    pub title: Option<String>,
    pub git_branch: Option<String>,
    pub model: Option<String>,
    pub last_activity: Option<DateTime<Utc>>,
    pub pending_tool: Option<PendingTool>,
    pub last_stop_reason: Option<String>,
    pub last_is_error: bool,
    pub handed_back: bool,
    pub tokens: TokenTotals,
    pub skipped_lines: usize,
    pub agent_name: Option<String>,
    /// input + cache_creation + cache_read of the last non-synthetic assistant usage.
    pub context_used: Option<u64>,
    pub max_context_used: u64,
    pub turn_ended_at: Option<DateTime<Utc>>,
    pub last_error_kind: Option<String>,
}

/// Text of a user message if it is a plain prompt (string content, or a list
/// with a `text` block). Returns None for tool results and the like.
fn user_text(msg: &Value) -> Option<&str> {
    match msg.get("content")? {
        Value::String(s) => Some(s),
        Value::Array(blocks) => blocks
            .iter()
            .find(|b| b.get("type").and_then(Value::as_str) == Some("text"))
            .and_then(|b| b.get("text"))
            .and_then(Value::as_str),
        _ => None,
    }
}

/// First line of a prompt as a title; None if there is no usable text.
fn title_from(text: &str) -> Option<String> {
    let first = text.trim_start().lines().next().unwrap_or("").trim();
    (!first.is_empty()).then(|| first.chars().take(TITLE_MAX_CHARS).collect())
}

fn usage_totals(u: &Value) -> TokenTotals {
    let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
    TokenTotals {
        input: n("input_tokens"),
        output: n("output_tokens"),
        cache_read: n("cache_read_input_tokens"),
        cache_write: n("cache_creation_input_tokens"),
    }
}

/// Incremental per-line state behind [`TranscriptSummary`].
#[derive(Debug, Clone, Default)]
pub struct SummaryState {
    s: TranscriptSummary,
    ai_title: Option<String>,
    first_prompt: Option<String>,
    open: Vec<(String, String, DateTime<Utc>)>, // (id, name, since)
    usage_by_msg: HashMap<String, TokenTotals>,
}

impl SummaryState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply_line(&mut self, line: &str) {
    let s = &mut self.s;
        let Ok(rec) = serde_json::from_str::<Value>(line) else {
    s.skipped_lines += 1;
        return;
    };
    let rtype = rec.get("type").and_then(Value::as_str).unwrap_or("");

    if rtype == "agent-name" {
        if let Some(n) = rec.get("agentName").and_then(Value::as_str) {
            s.agent_name = Some(n.to_string());
        }
        return;
    }
    if rtype == "ai-title" {
        if let Some(t) = rec.get("aiTitle").and_then(Value::as_str) {
            self.ai_title = Some(t.to_string());
        }
        return;
    }
    if !matches!(rtype, "user" | "assistant" | "system") {
        return;
    }

    let ts = rec
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map(|t| t.with_timezone(&Utc));
    if let Some(t) = ts {
        s.last_activity = Some(t);
    }
    if let Some(b) = rec.get("gitBranch").and_then(Value::as_str) {
        s.git_branch = Some(b.to_string());
    }

    let msg = rec.get("message");
    // System records other than level=error (e.g. the turn_duration record
    // written after every turn) leave the flag unchanged.
    match rtype {
        "system" => {
            if rec.get("level").and_then(Value::as_str) == Some("error") {
                s.last_is_error = true;
            }
            if rec.get("subtype").and_then(Value::as_str) == Some("turn_duration") {
                if let Some(t) = ts {
                    s.turn_ended_at = Some(t);
                }
            }
        }
        "assistant" => {
            s.last_is_error = rec.get("isApiErrorMessage").and_then(Value::as_bool) == Some(true);
            s.last_error_kind = if s.last_is_error {
                rec.get("error").and_then(Value::as_str).map(str::to_string)
            } else {
                None
            };
        }
        _ => {
            s.last_is_error = false;
            s.last_error_kind = None;
        }
    }

    match rtype {
        "assistant" => {
            let Some(msg) = msg else { return };
            let synthetic = msg.get("model").and_then(Value::as_str) == Some(SYNTHETIC_MODEL);
            if !synthetic {
                if let Some(m) = msg.get("model").and_then(Value::as_str) {
                    s.model = Some(m.to_string());
                }
                if let Some(r) = msg.get("stop_reason").and_then(Value::as_str) {
                    s.last_stop_reason = Some(r.to_string());
                }
                if let (Some(id), Some(u)) =
                    (msg.get("id").and_then(Value::as_str), msg.get("usage"))
                {
                    self.usage_by_msg.insert(id.to_string(), usage_totals(u));
                }
                if let Some(u) = msg.get("usage") {
                    let t = usage_totals(u);
                    let ctx = t.input + t.cache_write + t.cache_read;
                    s.context_used = Some(ctx);
                    s.max_context_used = s.max_context_used.max(ctx);
                }
            }
            if let Some(blocks) = msg.get("content").and_then(Value::as_array) {
                for b in blocks {
                    if b.get("type").and_then(Value::as_str) != Some("tool_use") {
                        return;
                    }
                    let name = b.get("name").and_then(Value::as_str).unwrap_or("");
                    if name == "SubagentHandback" {
                        s.handed_back = true;
                    }
                    if let (Some(id), Some(t)) = (b.get("id").and_then(Value::as_str), ts) {
                        self.open.push((id.to_string(), name.to_string(), t));
                    }
                }
            }
        }
        "user" => {
            let Some(msg) = msg else { return };
            // tool_result blocks close their tool but are not prompts.
            if let Some(blocks) = msg.get("content").and_then(Value::as_array) {
                for b in blocks {
                    if b.get("type").and_then(Value::as_str) == Some("tool_result") {
                        if let Some(id) = b.get("tool_use_id").and_then(Value::as_str) {
                            self.open.retain(|(oid, _, _)| oid != id);
                        }
                    }
                }
            }
            let is_meta = rec.get("isMeta").and_then(Value::as_bool) == Some(true);
            let has_tool_result = msg
                .get("content")
                .and_then(Value::as_array)
                .is_some_and(|bs| {
                    bs.iter()
                        .any(|b| b.get("type").and_then(Value::as_str) == Some("tool_result"))
                });
            if is_meta || has_tool_result {
                return;
            }
            if let Some(text) = user_text(msg) {
                if text.trim_start().starts_with('<') {
                    return;
                }
                self.open.clear();
                if self.first_prompt.is_none() {
                    self.first_prompt = title_from(text);
                }
            }
        }
        _ => {}
    }
    }

    pub fn summary(&self) -> TranscriptSummary {
        let mut s = self.s.clone();
        s.title = self
            .ai_title
            .clone()
            .or_else(|| self.first_prompt.clone())
            .filter(|t| !t.is_empty());
        s.pending_tool = self.open.last().map(|(_, name, since)| PendingTool {
            name: name.clone(),
            since: *since,
        });
        for t in self.usage_by_msg.values() {
            s.tokens.input += t.input;
            s.tokens.output += t.output;
            s.tokens.cache_read += t.cache_read;
            s.tokens.cache_write += t.cache_write;
        }
        s
    }
}

/// Reads a transcript incrementally: only bytes appended since the last call
/// are parsed. A changed inode or a shorter file resets all state.
#[derive(Debug)]
pub struct TranscriptTail {
    path: PathBuf,
    state: SummaryState,
    offset: u64,
    inode: Option<u64>,
    carry: Vec<u8>,
}

impl TranscriptTail {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            state: SummaryState::new(),
            offset: 0,
            inode: None,
            carry: Vec::new(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn offset(&self) -> u64 {
        self.offset
    }

    pub fn summary(&self) -> TranscriptSummary {
        self.state.summary()
    }

    /// Returns true if any complete line was applied or a reset happened.
    pub fn read_new(&mut self) -> std::io::Result<bool> {
        let mut file = File::open(&self.path)?;
        let meta = file.metadata()?;
        let (ino, len) = (meta.ino(), meta.len());
        let mut changed = false;
        if self.inode.is_some_and(|i| i != ino) || len < self.offset {
            self.state = SummaryState::new();
            self.offset = 0;
            self.carry.clear();
            changed = true;
        }
        self.inode = Some(ino);

        file.seek(SeekFrom::Start(self.offset))?;
        let mut new = Vec::new();
        file.read_to_end(&mut new)?;
        self.offset += new.len() as u64;

        let mut data = std::mem::take(&mut self.carry);
        data.extend_from_slice(&new);
        let end = data.iter().rposition(|&b| b == b'\n').map_or(0, |i| i + 1);
        for raw in data[..end].split(|&b| b == b'\n') {
            let line = String::from_utf8_lossy(raw);
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            self.state.apply_line(line);
            changed = true;
        }
        self.carry = data[end..].to_vec();
        Ok(changed)
    }
}

pub fn summarize_transcript(path: &Path) -> std::io::Result<TranscriptSummary> {
    let mut tail = TranscriptTail::new(path.to_path_buf());
    tail.read_new()?;
    // A one-shot read also counts a final line that has no trailing newline.
    let mut state = tail.state.clone();
    let line = String::from_utf8_lossy(&tail.carry);
    let line = line.trim();
    if !line.is_empty() {
        state.apply_line(line);
    }
    Ok(state.summary())
}
