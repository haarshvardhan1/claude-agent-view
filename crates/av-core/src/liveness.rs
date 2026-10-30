//! Process liveness with pid-reuse detection. Only runs `ps`; never signals processes.

use crate::registry::SessionEntry;
use std::process::Command;

/// The probe itself failed (e.g. `ps` could not be spawned); liveness is unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeError(pub String);

pub trait ProcessProbe {
    /// Start time string of the process, or `None` when the pid is not running.
    fn proc_start(&self, pid: u32) -> Option<String>;

    /// Like `proc_start` but distinguishes "not running" (`Ok(None)`) from a probe failure.
    fn check(&self, pid: u32) -> Result<Option<String>, ProbeError> {
        Ok(self.proc_start(pid))
    }
}

/// Runs `ps`. Registry `procStart` is recorded in UTC while `ps -o lstart`
/// prints local time, so `ps` is run with `TZ=UTC LC_ALL=C` to compare equal.
pub struct SystemProbe;

impl ProcessProbe for SystemProbe {
    fn proc_start(&self, pid: u32) -> Option<String> {
        self.check(pid).ok().flatten()
    }

    fn check(&self, pid: u32) -> Result<Option<String>, ProbeError> {
        let out = Command::new("ps")
            .args(["-o", "lstart=", "-p", &pid.to_string()])
            .env("LC_ALL", "C")
            .env("TZ", "UTC")
            .output()
            .map_err(|e| ProbeError(e.to_string()))?;
        if !out.status.success() {
            return Ok(None);
        }
        let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok((!s.is_empty()).then_some(s))
    }
}

fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Alive iff the pid is running and, when the entry records `proc_start`, it matches; with no recorded `proc_start`, a running pid counts as alive. `None` when the probe fails.
pub fn check_alive(entry: &SessionEntry, probe: &dyn ProcessProbe) -> Option<bool> {
    let actual = probe.check(entry.pid).ok()?;
    Some(match actual {
        None => false,
        Some(actual) => match &entry.proc_start {
            Some(expected) => normalize(&actual) == normalize(expected),
            None => true,
        },
    })
}

/// `check_alive` with a probe failure counted as not alive.
pub fn is_alive(entry: &SessionEntry, probe: &dyn ProcessProbe) -> bool {
    check_alive(entry, probe).unwrap_or(false)
}
