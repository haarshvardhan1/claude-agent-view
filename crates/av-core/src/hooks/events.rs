//! The hook events file: `av hook` appends one JSON line per Claude Code
//! hook event; the app tails it.

use crate::status::HookSignal;
use crate::tailio::{read_appended, TailPos};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::{json, Map, Value};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const EVENTS_REL: &str = "agent-viewer/events.jsonl";
const MAX_LINE_BYTES: usize = 4096;
const RAW_MAX_CHARS: usize = 512;

pub fn events_path(claude_dir: &Path) -> PathBuf {
    claude_dir.join(EVENTS_REL)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookKind {
    Notification(HookSignal),
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookEvent {
    pub session_id: String,
    pub kind: HookKind,
    pub received_at: DateTime<Utc>,
}

fn permission_tool(message: &str) -> Option<String> {
    message
        .split_once("permission to use ")
        .and_then(|(_, rest)| rest.split_whitespace().next())
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '_'))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
}

pub fn classify_notification(notification_type: Option<&str>, message: &str) -> Option<HookSignal> {
    match notification_type {
        Some("permission_prompt") => Some(HookSignal::Permission {
            tool: permission_tool(message),
        }),
        Some("idle_prompt") => Some(HookSignal::WaitingInput),
        Some(_) => None,
        None => {
            if message.contains("needs your permission") {
                let tool = permission_tool(message);
                Some(HookSignal::Permission { tool })
            } else if message.contains("waiting for your input") {
                Some(HookSignal::WaitingInput)
            } else {
                None
            }
        }
    }
}

pub fn parse_event_line(line: &str) -> Option<HookEvent> {
    let v: Value = serde_json::from_str(line).ok()?;
    let session_id = v.get("session_id")?.as_str()?.to_string();
    let received_at = DateTime::parse_from_rfc3339(v.get("receivedAt")?.as_str()?)
        .ok()?
        .with_timezone(&Utc);
    let kind = match v.get("hook_event_name")?.as_str()? {
        "Stop" => HookKind::Stop,
        "Notification" => HookKind::Notification(classify_notification(
            v.get("notification_type").and_then(Value::as_str),
            v.get("message").and_then(Value::as_str).unwrap_or(""),
        )?),
        _ => return None,
    };
    Some(HookEvent {
        session_id,
        kind,
        received_at,
    })
}

fn serialize_line(obj: &Map<String, Value>) -> String {
    let mut s = Value::Object(obj.clone()).to_string();
    s.push('\n');
    s
}

/// Appends one event line (stdin JSON plus `receivedAt`) in a single write,
/// keeping the line, newline included, within 4096 bytes.
pub fn write_event(path: &Path, stdin_json: &str, now: DateTime<Utc>) -> std::io::Result<()> {
    let mut obj = match serde_json::from_str::<Value>(stdin_json) {
        Ok(Value::Object(m)) => m,
        _ => {
            let raw: String = stdin_json.chars().take(RAW_MAX_CHARS).collect();
            let mut m = Map::new();
            m.insert("raw".into(), Value::String(raw));
            m
        }
    };
    obj.insert(
        "receivedAt".into(),
        Value::String(now.to_rfc3339_opts(SecondsFormat::AutoSi, true)),
    );

    let mut line = serialize_line(&obj);
    while line.len() > MAX_LINE_BYTES {
        let excess = line.len() - MAX_LINE_BYTES;
        let msg = match obj.get("message") {
            Some(Value::String(m)) if !m.is_empty() => m.clone(),
            _ => break,
        };
        let mut cut = msg.len().saturating_sub(excess.max(1));
        while !msg.is_char_boundary(cut) {
            cut -= 1;
        }
        obj.insert("message".into(), Value::String(msg[..cut].to_string()));
        line = serialize_line(&obj);
    }
    if line.len() > MAX_LINE_BYTES {
        let mut m = Map::new();
        for k in ["session_id", "hook_event_name", "notification_type"] {
            if let Some(v) = obj.get(k).filter(|v| v.is_string()) {
                m.insert(k.into(), v.clone());
            }
        }
        m.insert("receivedAt".into(), obj["receivedAt"].clone());
        line = serialize_line(&m);
        if line.len() > MAX_LINE_BYTES {
            line = serialize_line(
                json!({"receivedAt": obj["receivedAt"]})
                    .as_object()
                    .unwrap(),
            );
        }
    }

    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut f = OpenOptions::new().append(true).create(true).open(path)?;
    f.write_all(line.as_bytes())
}

/// Incremental reader of the events file; same offset/inode/carry rules as
/// `TranscriptTail`.
#[derive(Debug)]
pub struct EventsTail {
    path: PathBuf,
    pos: TailPos,
}

impl EventsTail {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            pos: TailPos::default(),
        }
    }

    pub fn read_new(&mut self) -> std::io::Result<Vec<HookEvent>> {
        let (lines, _reset) = read_appended(&self.path, &mut self.pos)?;
        Ok(lines.iter().filter_map(|l| parse_event_line(l)).collect())
    }
}

/// Meant to run only at app start: an append landing during the rename would be lost.
/// If the file exceeds `max_bytes`, keeps the last `max_bytes` after the
/// first newline in that window (whole lines only). Atomic temp + rename.
pub fn trim_events_file(path: &Path, max_bytes: u64) -> std::io::Result<()> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    if len <= max_bytes {
        return Ok(());
    }
    f.seek(SeekFrom::Start(len - max_bytes))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let keep = match buf.iter().position(|&b| b == b'\n') {
        Some(i) => &buf[i + 1..],
        None => &buf[buf.len()..],
    };
    let tmp = path.with_extension("jsonl.tmp");
    fs::write(&tmp, keep)?;
    fs::rename(&tmp, path)
}
