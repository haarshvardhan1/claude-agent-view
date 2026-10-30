//! Process liveness with pid-reuse detection. Only runs `ps`; never signals processes.

use crate::registry::SessionEntry;
use std::collections::HashMap;
use std::process::Command;

/// The probe itself failed (`ps` could not be spawned, or it exited non-zero
/// with an error on stderr); liveness is unknown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeError(pub String);

/// Why `run_ps` failed; only `Failed` is worth retrying per pid.
enum PsError {
    Spawn(ProbeError),
    Failed(ProbeError),
}

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
        let s = run_ps("lstart=", &pid.to_string()).map_err(|e| match e {
            PsError::Spawn(e) | PsError::Failed(e) => e,
        })?;
        let s = s.trim().to_string();
        Ok((!s.is_empty()).then_some(s))
    }

    /// One `ps -o pid=,lstart= -p a,b,c` for all pids. A pid missing from the
    /// output is not running.
    fn check_many(&self, pids: &[u32]) -> Vec<Result<Option<String>, ProbeError>> {
        if pids.is_empty() {
            return Vec::new();
        }
        let list = pids.iter().map(u32::to_string).collect::<Vec<_>>().join(",");
        match run_ps("pid=,lstart=", &list) {
            Ok(out) => {
                let found = parse_pid_lstart(&out);
                pids.iter().map(|pid| Ok(found.get(pid).cloned())).collect()
            }
            // `ps` rejects the whole list over one bad pid (e.g. "process id
            // too large"); retry per pid so only that one reports an error.
            Err(PsError::Failed(_)) if pids.len() > 1 => {
                pids.iter().map(|&pid| self.check(pid)).collect()
            }
            Err(PsError::Spawn(e) | PsError::Failed(e)) => {
                pids.iter().map(|_| Err(e.clone())).collect()
            }
        }
    }
}

/// Runs `ps -o <fields> -p <list>` in UTC / C locale and classifies the result.
fn run_ps(fields: &str, list: &str) -> Result<String, PsError> {
    let out = Command::new("ps")
        .args(["-o", fields, "-p", list])
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .output()
        .map_err(|e| PsError::Spawn(ProbeError(e.to_string())))?;
    classify_ps(out.status.success(), &out.stdout, &out.stderr).map_err(PsError::Failed)
}

/// Success, or a silent non-zero exit (how `ps` reports "no such pid"), is
/// `Ok(stdout)`. A non-zero exit with nothing on stdout and a message on
/// stderr is a probe failure: liveness is unknown, not "dead".
pub fn classify_ps(success: bool, stdout: &[u8], stderr: &[u8]) -> Result<String, ProbeError> {
    let out = String::from_utf8_lossy(stdout).into_owned();
    let err = String::from_utf8_lossy(stderr);
    if !success && out.trim().is_empty() && !err.trim().is_empty() {
        return Err(ProbeError(err.trim().to_string()));
    }
    Ok(out)
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
