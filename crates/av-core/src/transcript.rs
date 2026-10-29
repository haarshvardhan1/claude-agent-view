//! Streaming summarizer for session transcripts (`*.jsonl`).
//!
//! Transcripts can be tens of MB, so the file is read one line at a time and
//! never held in memory as a whole.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

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

#[derive(Debug, Clone, Default, Serialize)]
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

fn title_from(text: &str) -> String {
    let first = text.lines().next().unwrap_or("").trim();
    first.chars().take(TITLE_MAX_CHARS).collect()
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

pub fn summarize_transcript(path: &Path) -> std::io::Result<TranscriptSummary> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut s = TranscriptSummary::default();
    let mut ai_title: Option<String> = None;
    let mut first_prompt: Option<String> = None;
    let mut open: Vec<(String, String, DateTime<Utc>)> = Vec::new(); // (id, name, since)
    let mut usage_by_msg: HashMap<String, TokenTotals> = HashMap::new();

    let mut buf = Vec::new();
    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            break;
        }
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(rec) = serde_json::from_str::<Value>(line) else {
            s.skipped_lines += 1;
            continue;
        };
        let rtype = rec.get("type").and_then(Value::as_str).unwrap_or("");

        if rtype == "ai-title" {
            if let Some(t) = rec.get("aiTitle").and_then(Value::as_str) {
                ai_title = Some(t.to_string());
            }
            continue;
        }
        if !matches!(rtype, "user" | "assistant" | "system") {
            continue;
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
        s.last_is_error = match rtype {
            "system" => rec.get("level").and_then(Value::as_str) == Some("error"),
            "assistant" => rec.get("isApiErrorMessage").and_then(Value::as_bool) == Some(true),
            _ => false,
        };

        match rtype {
            "assistant" => {
                let Some(msg) = msg else { continue };
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
                        usage_by_msg.insert(id.to_string(), usage_totals(u));
                    }
                }
                if let Some(blocks) = msg.get("content").and_then(Value::as_array) {
                    for b in blocks {
                        if b.get("type").and_then(Value::as_str) != Some("tool_use") {
                            continue;
                        }
                        let name = b.get("name").and_then(Value::as_str).unwrap_or("");
                        if name == "SubagentHandback" {
                            s.handed_back = true;
                        }
                        if let (Some(id), Some(t)) = (b.get("id").and_then(Value::as_str), ts) {
                            open.push((id.to_string(), name.to_string(), t));
                        }
                    }
                }
            }
            "user" => {
                let Some(msg) = msg else { continue };
                // tool_result blocks close their tool but are not prompts.
                if let Some(blocks) = msg.get("content").and_then(Value::as_array) {
                    for b in blocks {
                        if b.get("type").and_then(Value::as_str) == Some("tool_result") {
                            if let Some(id) = b.get("tool_use_id").and_then(Value::as_str) {
                                open.retain(|(oid, _, _)| oid != id);
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
                    continue;
                }
                if let Some(text) = user_text(msg) {
                    if text.starts_with('<') {
                        continue;
                    }
                    open.clear();
                    if first_prompt.is_none() {
                        first_prompt = Some(title_from(text));
                    }
                }
            }
            _ => {}
        }
    }

    s.title = ai_title.or(first_prompt).filter(|t| !t.is_empty());
    s.pending_tool = open
        .pop()
        .map(|(_, name, since)| PendingTool { name, since });
    for t in usage_by_msg.values() {
        s.tokens.input += t.input;
        s.tokens.output += t.output;
        s.tokens.cache_read += t.cache_read;
        s.tokens.cache_write += t.cache_write;
    }
    Ok(s)
}
