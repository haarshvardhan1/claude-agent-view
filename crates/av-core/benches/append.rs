//! Incremental update cost for one appended transcript line (spec §1: < 5 ms).
//!
//! Plain `Instant` loop, no criterion: a `TranscriptTail` that has already read
//! a ~2 MB transcript gets one assistant line appended, then `read_new` plus
//! `summary` (what the engine does per change). Run with
//! `cargo bench -p av-core --bench append`.

use av_core::transcript::TranscriptTail;
use std::fs::OpenOptions;
use std::io::Write;
use std::time::{Duration, Instant};

const ITERATIONS: u32 = 1_000;
const FIXTURE_BYTES: u64 = 2 * 1024 * 1024;
const TARGET: Duration = Duration::from_millis(5);

fn assistant_line(i: u64) -> String {
    serde_json::json!({
        "type": "assistant", "timestamp": "2026-10-29T13:39:54.792Z", "gitBranch": "main",
        "message": {
            "id": format!("msg_{i}"), "model": "claude-opus-5-5", "role": "assistant",
            "content": [{"type": "text", "text": "Working on it. ".repeat(20)}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 12, "output_tokens": 340,
                      "cache_creation_input_tokens": 900, "cache_read_input_tokens": 41000},
        },
    })
    .to_string()
}

fn tool_pair(i: u64) -> [String; 2] {
    let id = format!("toolu_{i}");
    [
        serde_json::json!({
            "type": "assistant", "timestamp": "2026-10-29T13:39:55.000Z", "gitBranch": "main",
            "message": {
                "id": format!("msg_t{i}"), "model": "claude-opus-5-5", "role": "assistant",
                "content": [{"type": "tool_use", "id": id, "name": "Read",
                             "input": {"file_path": "/repo/src/lib.rs"}}],
                "stop_reason": "tool_use",
                "usage": {"input_tokens": 3, "output_tokens": 40,
                          "cache_creation_input_tokens": 0, "cache_read_input_tokens": 41000},
            },
        })
        .to_string(),
        serde_json::json!({
            "type": "user", "timestamp": "2026-10-29T13:39:55.500Z",
            "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": id,
                 "content": "fn main() {}\n".repeat(60), "is_error": false}
            ]},
        })
        .to_string(),
    ]
}

fn main() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("s.jsonl");
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .expect("open fixture");
    let mut i = 0u64;
    let mut written = 0u64;
    while written < FIXTURE_BYTES {
        for line in tool_pair(i).iter().chain([assistant_line(i)].iter()) {
            writeln!(f, "{line}").unwrap();
            written += line.len() as u64 + 1;
        }
        i += 1;
    }
    f.flush().unwrap();

    let mut tail = TranscriptTail::new(path.clone());
    let t = Instant::now();
    tail.read_new().unwrap();
    let initial = t.elapsed();

    let mut total = Duration::ZERO;
    let mut max = Duration::ZERO;
    for n in 0..ITERATIONS {
        let line = assistant_line(1_000_000 + u64::from(n));
        let t = Instant::now();
        writeln!(f, "{line}").unwrap();
        assert!(tail.read_new().unwrap(), "appended line not read");
        std::hint::black_box(tail.summary());
        let d = t.elapsed();
        total += d;
        max = max.max(d);
    }
    let mean = total / ITERATIONS;
    println!(
        "append: fixture {:.2} MB ({} records), initial read {:.1} ms",
        written as f64 / 1_048_576.0,
        i * 3,
        initial.as_secs_f64() * 1e3
    );
    println!(
        "append: {ITERATIONS} iterations, mean {:.3} ms, max {:.3} ms (target < {} ms)",
        mean.as_secs_f64() * 1e3,
        max.as_secs_f64() * 1e3,
        TARGET.as_millis()
    );
    assert!(mean < TARGET, "mean {mean:?} >= {TARGET:?}");
    println!("append: PASS");
}
