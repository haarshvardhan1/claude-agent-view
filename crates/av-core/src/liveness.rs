//! Process liveness with pid-reuse detection. Only runs `ps`; never signals processes.

use crate::registry::SessionEntry;
use std::collections::HashMap;
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

    /// `check` for several pids at once, one result per pid in order. The
    /// default calls `check` per pid; `SystemProbe` runs a single `ps`.
    fn check_many(&self, pids: &[u32]) -> Vec<Result<Option<String>, ProbeError>> {
        pids.iter().map(|&pid| self.check(pid)).collect()
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

    /// One `ps -o pid=,lstart= -p a,b,c` for all pids. A pid missing from the
    /// output is not running. `ps` exits non-zero, silently, when none are.
    fn check_many(&self, pids: &[u32]) -> Vec<Result<Option<String>, ProbeError>> {
        if pids.is_empty() {
            return Vec::new();
        }
        let list = pids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        let out = match Command::new("ps")
            .args(["-o", "pid=,lstart=", "-p", &list])
            .env("LC_ALL", "C")
            .env("TZ", "UTC")
            .output()
        {
            Ok(out) => out,
            Err(e) => return pids.iter().map(|_| Err(ProbeError(e.to_string()))).collect(),
        };
        // `ps` rejects the whole list over one bad pid (e.g. "process id too
        // large"); then fall back to one `ps` per pid.
        if !out.status.success() && !out.stderr.is_empty() && pids.len() > 1 {
            return pids.iter().map(|&pid| self.check(pid)).collect();
        }
        let found = parse_pid_lstart(&String::from_utf8_lossy(&out.stdout));
        pids.iter().map(|pid| Ok(found.get(pid).cloned())).collect()
    }
}

/// Parses `ps -o pid=,lstart=` output into pid -> start time.
pub fn parse_pid_lstart(out: &str) -> HashMap<u32, String> {
    out.lines()
        .filter_map(|line| {
            let (pid, start) = line.trim().split_once(char::is_whitespace)?;
            let start = start.trim();
            let pid = pid.parse().ok()?;
            (!start.is_empty()).then(|| (pid, start.to_string()))
        })
        .collect()
}

fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Alive iff the pid is running and, when the entry records `proc_start`, it matches; with no recorded `proc_start`, a running pid counts as alive. `None` when the probe fails.
pub fn check_alive(entry: &SessionEntry, probe: &dyn ProcessProbe) -> Option<bool> {
    alive_from(entry, probe.check(entry.pid))
}

/// `check_alive` given an already-run probe result for `entry.pid`.
pub fn alive_from(
    entry: &SessionEntry,
    probed: Result<Option<String>, ProbeError>,
) -> Option<bool> {
    let actual = probed.ok()?;
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
