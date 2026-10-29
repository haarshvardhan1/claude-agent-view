//! Process liveness with pid-reuse detection. Only runs `ps`; never signals processes.

use crate::registry::SessionEntry;
use std::process::Command;

pub trait ProcessProbe {
    /// Start time string of the process, or `None` when the pid is not running.
    fn proc_start(&self, pid: u32) -> Option<String>;
}

pub struct SystemProbe;

impl ProcessProbe for SystemProbe {
    fn proc_start(&self, pid: u32) -> Option<String> {
        let out = Command::new("ps")
            .args(["-o", "lstart=", "-p", &pid.to_string()])
            .env("LC_ALL", "C")
            .env("TZ", "UTC")
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        (!s.is_empty()).then_some(s)
    }
}

fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Registry `procStart` is recorded in UTC, so `ps` is run with TZ=UTC.
/// Alive iff the pid is running and, when the entry records `proc_start`, it matches; with no recorded `proc_start`, a running pid counts as alive.
pub fn is_alive(entry: &SessionEntry, probe: &dyn ProcessProbe) -> bool {
    match probe.proc_start(entry.pid) {
        None => false,
        Some(actual) => match &entry.proc_start {
            Some(expected) => normalize(&actual) == normalize(expected),
            None => true,
        },
    }
}
