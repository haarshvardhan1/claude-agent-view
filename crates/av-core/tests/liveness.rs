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

// Only detects the TZ bug on hosts whose local zone is not UTC; on a UTC host
// it passes either way.
#[test]
fn system_probe_reports_utc() {
    let s = SystemProbe
        .proc_start(std::process::id())
        .expect("own pid is running");
    let norm = s.split_whitespace().collect::<Vec<_>>().join(" ");
    let t = chrono::NaiveDateTime::parse_from_str(&norm, "%a %b %e %H:%M:%S %Y").unwrap();
    let diff = (chrono::Utc::now().naive_utc() - t).num_seconds().abs();
    assert!(
        diff < 600,
        "proc start {norm} is {diff}s from now; not UTC?"
    );
}

struct ErrProbe;
impl ProcessProbe for ErrProbe {
    fn proc_start(&self, _pid: u32) -> Option<String> {
        None
    }
    fn check(&self, _pid: u32) -> Result<Option<String>, av_core::liveness::ProbeError> {
        Err(av_core::liveness::ProbeError("boom".into()))
    }
}

#[test]
fn probe_error_is_unknown() {
    let e = entry(42, Some("Thu Oct 29 12:36:53 2026"));
    assert_eq!(av_core::liveness::check_alive(&e, &ErrProbe), None);
    assert!(!is_alive(&e, &ErrProbe));
    let ok = probe(42, "Thu Oct 29 12:36:53 2026");
    assert_eq!(av_core::liveness::check_alive(&e, &ok), Some(true));
}
