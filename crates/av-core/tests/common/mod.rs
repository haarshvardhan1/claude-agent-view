#![allow(dead_code)]

use std::fs;
use std::path::Path;

/// Write a realistic session registry file to `<home>/sessions/<pid>.json`.
pub fn write_session(home: &Path, pid: u32, session_id: &str, cwd: &str, status: &str) {
    write_session_at(home, pid, session_id, cwd, status, 1793281174333);
}

pub fn write_session_at(
    home: &Path,
    pid: u32,
    session_id: &str,
    cwd: &str,
    status: &str,
    started_at: i64,
) {
    let dir = home.join("sessions");
    fs::create_dir_all(&dir).unwrap();
    let body = serde_json::json!({
        "pid": pid,
        "sessionId": session_id,
        "cwd": cwd,
        "startedAt": started_at,
        "procStart": "Thu Oct 29 12:36:53 2026",
        "version": "2.1.312",
        "peerProtocol": 1,
        "kind": "interactive",
        "entrypoint": "cli",
        "name": format!("agent-{pid}"),
        "status": status,
        "updatedAt": started_at + 20000,
    });
    fs::write(dir.join(format!("{pid}.json")), body.to_string()).unwrap();
}

use av_core::liveness::ProcessProbe;
use av_core::registry::SessionEntry;
use std::collections::HashMap;

pub struct FakeProbe(pub HashMap<u32, String>);

impl ProcessProbe for FakeProbe {
    fn proc_start(&self, pid: u32) -> Option<String> {
        self.0.get(&pid).cloned()
    }
}

pub fn entry(pid: u32, proc_start: Option<&str>) -> SessionEntry {
    serde_json::from_value(serde_json::json!({
        "pid": pid,
        "sessionId": "s",
        "procStart": proc_start,
    }))
    .unwrap()
}
