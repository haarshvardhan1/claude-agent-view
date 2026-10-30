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

#[test]
fn parse_pid_lstart_reads_each_line() {
    let out = "    1 Thu Oct 29 13:09:51 2026    \n76254 Fri Oct  9 09:54:57 2026\n\ngarbage\n";
    let m = av_core::liveness::parse_pid_lstart(out);
    assert_eq!(m.len(), 2);
    assert_eq!(m[&1], "Thu Oct 29 13:09:51 2026");
    assert_eq!(m[&76254], "Fri Oct  9 09:54:57 2026");
}

#[test]
fn system_probe_check_many_matches_check() {
    let own = std::process::id();
    // A child that has exited and been reaped: its pid is not running.
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let dead = child.id();
    child.wait().unwrap();
    let r = SystemProbe.check_many(&[own, dead]);
    assert_eq!(r.len(), 2);
    assert_eq!(r[0], SystemProbe.check(own));
    assert_eq!(r[1], Ok(None));
    assert_eq!(SystemProbe.check_many(&[dead]), vec![Ok(None)]);
    assert!(SystemProbe.check_many(&[]).is_empty());
    // A pid `ps` rejects outright (above pid_max) must not hide the others,
    // and is a probe error (unknown), not "not running".
    let r = SystemProbe.check_many(&[own, 4_000_000]);
    assert_eq!(r[0], SystemProbe.check(own));
    assert!(r[0].as_ref().unwrap().is_some());
    assert!(r[1].is_err(), "got {:?}", r[1]);
    assert!(SystemProbe.check(4_000_000).is_err());
    assert!(SystemProbe.check_many(&[4_000_000])[0].is_err());
}

#[test]
fn default_check_many_loops_over_check() {
    let p = probe(42, "Thu Oct 29 12:36:53 2026");
    assert_eq!(
        p.check_many(&[42, 7]),
        vec![Ok(Some("Thu Oct 29 12:36:53 2026".to_string())), Ok(None)]
    );
}

#[test]
fn classify_ps_separates_no_such_pid_from_failure() {
    use av_core::liveness::classify_ps;
    // Success, and a silent non-zero exit ("no such pid"), are not errors.
    assert_eq!(classify_ps(true, b"x\n", b""), Ok("x\n".to_string()));
    assert_eq!(classify_ps(false, b"", b""), Ok(String::new()));
    // Non-zero with a message on stderr and nothing on stdout is a failure.
    assert!(classify_ps(false, b"", b"ps: process id too large: 4000000\n").is_err());
    assert!(classify_ps(false, b"  \n", b"ps: boom").is_err());
}
