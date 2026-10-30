//! Paged reader behind the detail panel's live transcript.
//!
//! Reads backward in 64 KB blocks so only the lines a page needs are loaded.

use crate::model::{TranscriptEntry, TranscriptPage};
use crate::transcript::user_text;
use chrono::{DateTime, Utc};
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

pub const FIELD_MAX: usize = 4096;
const BLOCK: u64 = 64 * 1024;
const SUMMARY_MAX: usize = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageRequest {
    Tail,
    Before(u64),
    After(u64),
}

fn cut(s: &str) -> String {
    if s.len() <= FIELD_MAX {
        return s.to_string();
    }
    let mut n = FIELD_MAX;
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    format!("{}…", &s[..n])
}

pub fn tool_summary(name: &str, input: &Value) -> String {
    let key = match name {
        "Bash" => Some("command"),
        "Read" | "Write" | "Edit" | "NotebookEdit" => Some("file_path"),
        "Grep" | "Glob" => Some("pattern"),
        "Agent" => Some("description"),
        "WebFetch" => Some("url"),
        _ => None,
    };
    let s = key
        .and_then(|k| input.get(k))
        .and_then(Value::as_str)
        .or_else(|| {
            input
                .as_object()
                .and_then(|o| o.values().find_map(Value::as_str))
        })
        .unwrap_or("");
    s.lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(SUMMARY_MAX)
        .collect()
}

enum Item {
    Entry(TranscriptEntry),
    Result {
        id: String,
        content: String,
        is_error: bool,
    },
}

fn result_text(c: Option<&Value>) -> String {
    match c {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(bs)) => bs
            .iter()
            .filter_map(|b| b.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn parse_line(line: &[u8]) -> Vec<Item> {
    let Ok(rec) = serde_json::from_slice::<Value>(line) else {
        return vec![];
    };
    let ts: Option<DateTime<Utc>> = rec
        .get("timestamp")
        .and_then(Value::as_str)
        .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
        .map(|t| t.with_timezone(&Utc));
    let Some(msg) = rec.get("message") else {
        return vec![];
    };
    let mut out = Vec::new();
    match rec.get("type").and_then(Value::as_str) {
        Some("user") => {
            if let Some(bs) = msg.get("content").and_then(Value::as_array) {
                let mut has_result = false;
                for b in bs {
                    if b.get("type").and_then(Value::as_str) == Some("tool_result") {
                        has_result = true;
                        out.push(Item::Result {
                            id: b
                                .get("tool_use_id")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .into(),
                            content: cut(&result_text(b.get("content"))),
                            is_error: b.get("is_error").and_then(Value::as_bool) == Some(true),
                        });
                    }
                }
                if has_result {
                    return out;
                }
            }
            if rec.get("isMeta").and_then(Value::as_bool) == Some(true) {
                return out;
            }
            if let Some(t) = user_text(msg) {
                if !t.trim_start().starts_with('<') {
                    out.push(Item::Entry(TranscriptEntry::UserPrompt {
                        text: t.to_string(),
                        timestamp: ts,
                    }));
                }
            }
        }
        Some("assistant") => match msg.get("content") {
            Some(Value::String(t)) => out.push(Item::Entry(TranscriptEntry::AssistantText {
                text: t.clone(),
                timestamp: ts,
            })),
            Some(Value::Array(bs)) => {
                for b in bs {
                    let s = |k: &str| b.get(k).and_then(Value::as_str);
                    match s("type") {
                        Some("text") => out.push(Item::Entry(TranscriptEntry::AssistantText {
                            text: s("text").unwrap_or("").to_string(),
                            timestamp: ts,
                        })),
                        Some("thinking") => out.push(Item::Entry(TranscriptEntry::Thinking {
                            text: cut(s("thinking").unwrap_or("")),
                            timestamp: ts,
                        })),
                        Some("tool_use") => {
                            let name = s("name").unwrap_or("").to_string();
                            let input = b.get("input").cloned().unwrap_or(Value::Null);
                            out.push(Item::Entry(TranscriptEntry::ToolCall {
                                id: s("id").unwrap_or("").to_string(),
                                summary: tool_summary(&name, &input),
                                input: cut(
                                    &serde_json::to_string_pretty(&input).unwrap_or_default()
                                ),
                                name,
                                result: None,
                                is_error: false,
                                timestamp: ts,
                            }));
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        },
        _ => {}
    }
    out
}

/// Pair results with calls (same page only) and collect entries in order.
fn assemble(records: Vec<Vec<Item>>) -> Vec<TranscriptEntry> {
    let mut entries: Vec<TranscriptEntry> = Vec::new();
    for item in records.into_iter().flatten() {
        match item {
            Item::Entry(e) => entries.push(e),
            Item::Result {
                id,
                content,
                is_error,
            } => {
                for e in entries.iter_mut().rev() {
                    if let TranscriptEntry::ToolCall {
                        id: cid,
                        result,
                        is_error: err,
                        ..
                    } = e
                    {
                        if *cid == id && result.is_none() {
                            *result = Some(content);
                            *err = is_error;
                            break;
                        }
                    }
                }
            }
        }
    }
    entries
}

fn entry_count(items: &[Item]) -> usize {
    items.iter().filter(|i| matches!(i, Item::Entry(_))).count()
}

/// Walks complete lines backward from a line boundary.
struct BackLines<'a> {
    f: &'a mut File,
    buf: Vec<u8>, // file bytes [buf_start, end of last unread line)
    buf_start: u64,
}

impl<'a> BackLines<'a> {
    /// `end` must be 0 or just after a `\n`.
    fn new(f: &'a mut File, end: u64) -> Self {
        BackLines {
            f,
            buf: Vec::new(),
            buf_start: end,
        }
    }

    /// Next line backward as (start offset, bytes without the newline).
    fn next_line(&mut self) -> std::io::Result<Option<(u64, Vec<u8>)>> {
        loop {
            // The last byte of buf (if any) is the line's own '\n'.
            let search_end = self.buf.len().saturating_sub(1);
            if let Some(i) = self.buf[..search_end].iter().rposition(|&b| b == b'\n') {
                let mut line = self.buf.split_off(i + 1);
                line.pop();
                return Ok(Some((self.buf_start + i as u64 + 1, line)));
            }
            if self.buf_start == 0 {
                if self.buf.is_empty() {
                    return Ok(None);
                }
                let mut line = std::mem::take(&mut self.buf);
                line.pop();
                return Ok(Some((0, line)));
            }
            let n = BLOCK.min(self.buf_start);
            let new_start = self.buf_start - n;
            let mut block = vec![0u8; n as usize];
            self.f.seek(SeekFrom::Start(new_start))?;
            self.f.read_exact(&mut block)?;
            block.extend_from_slice(&self.buf);
            self.buf = block;
            self.buf_start = new_start;
        }
    }
}

/// Offset just after the last '\n' in `[0, off)` (0 if none).
fn last_boundary(f: &mut File, off: u64) -> std::io::Result<u64> {
    let mut pos = off;
    while pos > 0 {
        let n = BLOCK.min(pos);
        let mut block = vec![0u8; n as usize];
        f.seek(SeekFrom::Start(pos - n))?;
        f.read_exact(&mut block)?;
        if let Some(i) = block.iter().rposition(|&b| b == b'\n') {
            return Ok(pos - n + i as u64 + 1);
        }
        pos -= n;
    }
    Ok(0)
}

fn read_before(f: &mut File, off: u64, limit: usize) -> std::io::Result<TranscriptPage> {
    let end = last_boundary(f, off)?;
    let mut lines = BackLines::new(f, end);
    let mut records: Vec<Vec<Item>> = Vec::new();
    let mut count = 0;
    let mut start = end;
    while count < limit {
        let Some((s, line)) = lines.next_line()? else {
            start = 0;
            break;
        };
        start = s;
        let items = parse_line(&line);
        count += entry_count(&items);
        records.push(items);
    }
    records.reverse();
    Ok(TranscriptPage {
        entries: assemble(records),
        start_offset: start,
        end_offset: end,
    })
}

pub fn read_page(path: &Path, req: PageRequest, limit: usize) -> std::io::Result<TranscriptPage> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    match req {
        PageRequest::Tail => read_before(&mut f, len, limit),
        PageRequest::Before(off) => read_before(&mut f, off.min(len), limit),
        PageRequest::After(off) => {
            if off > len {
                return Ok(TranscriptPage {
                    entries: vec![],
                    start_offset: off,
                    end_offset: off,
                });
            }
            f.seek(SeekFrom::Start(off))?;
            let mut r = BufReader::new(f);
            let mut pos = off;
            let mut records = Vec::new();
            let mut buf = Vec::new();
            loop {
                buf.clear();
                let n = r.read_until(b'\n', &mut buf)?;
                if n == 0 || buf.last() != Some(&b'\n') {
                    break;
                }
                pos += n as u64;
                buf.pop();
                records.push(parse_line(&buf));
            }
            Ok(TranscriptPage {
                entries: assemble(records),
                start_offset: off,
                end_offset: pos,
            })
        }
    }
}
