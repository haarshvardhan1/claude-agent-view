mod common;
use av_core::liveness::{is_alive, ProcessProbe, SystemProbe};
use common::{entry, FakeProbe};
use std::collections::HashMap;

fn probe(pid: u32, s: &str) -> FakeProbe {
    FakeProbe(HashMap::from([(pid, s.to_string())]))
}

#[test]
fn alive_when_start_matches() {
    let e = entry(42, Some("Thu Oct 29 12:36:53 2026"));
    assert!(is_alive(&e, &probe(42, "Thu Oct 29 12:36:53 2026")));
}

#[test]
fn pid_reused_is_not_alive() {
    let e = entry(42, Some("Thu Oct 29 12:36:53 2026"));
    assert!(!is_alive(&e, &probe(42, "Fri Oct 30 09:00:00 2026")));
}

#[test]
fn dead_pid_is_not_alive() {
    let e = entry(42, Some("Thu Oct 29 12:36:53 2026"));
    assert!(!is_alive(&e, &FakeProbe(HashMap::new())));
}

#[test]
fn padded_day_matches() {
    let e = entry(42, Some("Thu Oct  9 01:02:03 2026"));
    assert!(is_alive(&e, &probe(42, "Thu Oct 9 01:02:03 2026")));
}

#[test]
fn system_probe_sees_own_process() {
    assert!(SystemProbe.proc_start(std::process::id()).is_some());
}

#[test]
fn missing_proc_start_alive_if_running() {
    let e = entry(42, None);
    assert!(is_alive(&e, &probe(42, "Thu Oct 29 12:36:53 2026")));
    assert!(!is_alive(&e, &FakeProbe(HashMap::new())));
}
