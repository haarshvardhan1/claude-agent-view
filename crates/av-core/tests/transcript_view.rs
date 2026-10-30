mod common;
use av_core::model::TranscriptEntry as E;
use av_core::transcript_view::{read_page, tool_summary, PageRequest, FIELD_MAX};
use common::*;
use std::fs;
use std::io::Write;
use std::path::PathBuf;

fn write(dir: &tempfile::TempDir, lines: &[String]) -> PathBuf {
    let p = dir.path().join("t.jsonl");
    fs::write(&p, lines.join("\n") + "\n").unwrap();
    p
}
fn prompts(n: usize) -> Vec<String> {
    (0..n).map(|i| user_prompt(&format!("p{i}"), T0)).collect()
}
fn text_of(e: &E) -> &str {
    match e {
        E::UserPrompt { text, .. } | E::AssistantText { text, .. } | E::Thinking { text, .. } => {
            text
        }
        E::ToolCall { name, .. } => name,
    }
}

#[test]
fn tail_returns_last_n() {
    let d = tempfile::tempdir().unwrap();
    let p = write(&d, &prompts(300));
    let page = read_page(&p, PageRequest::Tail, 200).unwrap();
    assert_eq!(page.entries.len(), 200);
    assert_eq!(text_of(page.entries.last().unwrap()), "p299");
    assert_eq!(page.end_offset, fs::metadata(&p).unwrap().len());
}

#[test]
fn before_pages_back() {
    let d = tempfile::tempdir().unwrap();
    let p = write(&d, &prompts(300));
    let tail = read_page(&p, PageRequest::Tail, 200).unwrap();
    let before = read_page(&p, PageRequest::Before(tail.start_offset), 200).unwrap();
    assert_eq!(before.entries.len(), 100);
    assert_eq!(text_of(&before.entries[0]), "p0");
    assert_eq!(text_of(&before.entries[99]), "p99");
    assert_eq!(before.start_offset, 0);
    assert_eq!(before.end_offset, tail.start_offset);
}

#[test]
fn after_returns_new() {
    let d = tempfile::tempdir().unwrap();
    let p = write(&d, &prompts(5));
    let tail = read_page(&p, PageRequest::Tail, 200).unwrap();
    let none = read_page(&p, PageRequest::After(tail.end_offset), 200).unwrap();
    assert!(none.entries.is_empty());
    assert_eq!(
        (none.start_offset, none.end_offset),
        (tail.end_offset, tail.end_offset)
    );
    let mut f = fs::OpenOptions::new().append(true).open(&p).unwrap();
    writeln!(f, "{}", user_prompt("n1", T0)).unwrap();
    writeln!(f, "{}", user_prompt("n2", T0)).unwrap();
    write!(f, "{{\"type\":\"user\",\"mess").unwrap(); // partial
    let page = read_page(&p, PageRequest::After(tail.end_offset), 200).unwrap();
    assert_eq!(page.entries.len(), 2);
    assert_eq!(text_of(&page.entries[1]), "n2");
    let t2 = read_page(&p, PageRequest::Tail, 200).unwrap();
    assert_eq!(t2.end_offset, page.end_offset);
    assert_eq!(t2.entries.len(), 7);
}

#[test]
fn after_past_eof_is_empty() {
    let d = tempfile::tempdir().unwrap();
    let p = write(&d, &prompts(2));
    let page = read_page(&p, PageRequest::After(999_999), 200).unwrap();
    assert!(page.entries.is_empty());
    assert_eq!((page.start_offset, page.end_offset), (999_999, 999_999));
}

#[test]
fn tool_call_paired() {
    let d = tempfile::tempdir().unwrap();
    let p = write(
        &d,
        &[
            assistant_text_and_tool(
                "running",
                "t1",
                "Bash",
                serde_json::json!({"command": "npm test\nmore"}),
                T0,
            ),
            tool_result_with(
                "t1",
                serde_json::json!([{"type":"text","text":"a"},{"type":"text","text":"b"}]),
                true,
                T0,
            ),
            tool_result_with("zzz", serde_json::json!("orphan"), false, T0),
        ],
    );
    let page = read_page(&p, PageRequest::Tail, 200).unwrap();
    assert_eq!(page.entries.len(), 2);
    assert!(matches!(&page.entries[0], E::AssistantText { text, .. } if text == "running"));
    match &page.entries[1] {
        E::ToolCall {
            id,
            summary,
            result,
            is_error,
            ..
        } => {
            assert_eq!(id, "t1");
            assert_eq!(summary, "npm test");
            assert_eq!(result.as_deref(), Some("a\nb"));
            assert!(*is_error);
        }
        e => panic!("{e:?}"),
    }
}

#[test]
fn unpaired_call_across_pages_has_no_result() {
    let d = tempfile::tempdir().unwrap();
    let p = write(
        &d,
        &[
            assistant_tool_use("m", "t1", "Bash", T0),
            user_prompt("a", T0),
            tool_result("t1", T0),
        ],
    );
    let page = read_page(&p, PageRequest::Tail, 1).unwrap();
    assert_eq!(page.entries.len(), 1); // just the prompt; result dropped
}

#[test]
fn record_with_several_blocks_is_kept_whole() {
    let d = tempfile::tempdir().unwrap();
    let p = write(
        &d,
        &[
            user_prompt("first", T0),
            assistant_text_and_tool(
                "x",
                "t1",
                "Read",
                serde_json::json!({"file_path": "/a"}),
                T0,
            ),
        ],
    );
    let page = read_page(&p, PageRequest::Tail, 1).unwrap();
    assert_eq!(page.entries.len(), 2);
    assert_eq!(page.start_offset, user_prompt("first", T0).len() as u64 + 1);
}

#[test]
fn hidden_types_skipped() {
    let d = tempfile::tempdir().unwrap();
    let p = write(
        &d,
        &[
            system_error(T0),
            r#"{"type":"attachment","attachment":{}}"#.into(),
            r#"{"type":"file-history-snapshot","snapshot":{}}"#.into(),
            user_meta("m", T0),
            user_prompt("<command-name>x", T0),
            "not json".into(),
        ],
    );
    let page = read_page(&p, PageRequest::Tail, 200).unwrap();
    assert!(page.entries.is_empty());
}

#[test]
fn huge_line_is_cut() {
    let d = tempfile::tempdir().unwrap();
    let big = "é".repeat(2_600_000); // 5.2 MB
    let p = write(
        &d,
        &[
            user_prompt("before", T0),
            assistant_tool_use("m", "t1", "Bash", T0),
            tool_result_with("t1", serde_json::json!(big), false, T0),
            user_prompt("after", T0),
        ],
    );
    let page = read_page(&p, PageRequest::Tail, 200).unwrap();
    assert_eq!(page.entries.len(), 3);
    assert_eq!(text_of(&page.entries[0]), "before");
    assert_eq!(text_of(&page.entries[2]), "after");
    match &page.entries[1] {
        E::ToolCall {
            result: Some(r), ..
        } => {
            assert_eq!(r.len(), FIELD_MAX + "…".len());
            assert!(r.ends_with('…'));
        }
        e => panic!("{e:?}"),
    }
    let one = read_page(&p, PageRequest::Tail, 3).unwrap();
    assert_eq!(one.start_offset, 0);
}

#[test]
fn prompt_and_text_not_cut_but_thinking_is() {
    let d = tempfile::tempdir().unwrap();
    let long = "a".repeat(10_000);
    let think = serde_json::json!({"type":"assistant","message":{"content":[
        {"type":"thinking","thinking": long.clone()},{"type":"text","text": long.clone()}]}})
    .to_string();
    let p = write(&d, &[user_prompt(&long, T0), think]);
    let page = read_page(&p, PageRequest::Tail, 200).unwrap();
    assert_eq!(text_of(&page.entries[0]).len(), 10_000);
    assert_eq!(text_of(&page.entries[1]).len(), FIELD_MAX + 3);
    assert_eq!(text_of(&page.entries[2]).len(), 10_000);
}

#[test]
fn summaries() {
    assert_eq!(
        tool_summary("Grep", &serde_json::json!({"pattern": "foo"})),
        "foo"
    );
    assert_eq!(
        tool_summary("Edit", &serde_json::json!({"file_path": "/x"})),
        "/x"
    );
    assert_eq!(
        tool_summary("Agent", &serde_json::json!({"description": "d"})),
        "d"
    );
    assert_eq!(
        tool_summary("WebFetch", &serde_json::json!({"url": "u"})),
        "u"
    );
    assert_eq!(
        tool_summary("Other", &serde_json::json!({"n": 1, "s": "first"})),
        "first"
    );
    assert_eq!(
        tool_summary("Bash", &serde_json::json!({"command": "x".repeat(300)}))
            .chars()
            .count(),
        120
    );
}
