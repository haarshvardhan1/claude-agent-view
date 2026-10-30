mod common;
use av_core::engine::{Engine, Input};
use av_core::hooks::events::{events_path, write_event};
use av_core::model::{Agent, Change, Status, SubagentState};
use av_core::paths::{encode_project_dir, subagents_dir};
use chrono::{DateTime, Duration, SecondsFormat, TimeZone, Utc};
use common::*;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

const CWD: &str = "/work/proj";

fn t0() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(T0)
        .unwrap()
        .with_timezone(&Utc)
}

/// T0 + `secs`, as a transcript timestamp string.
fn ts(secs: i64) -> String {
    (t0() + Duration::seconds(secs)).to_rfc3339_opts(SecondsFormat::Millis, true)
}

fn at(secs: i64) -> DateTime<Utc> {
    t0() + Duration::seconds(secs)
}

fn transcript(home: &Path, sid: &str) -> PathBuf {
    home.join("projects")
        .join(encode_project_dir(CWD))
        .join(format!("{sid}.jsonl"))
}

fn write_lines(path: &Path, lines: &[String]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, lines.join("\n") + "\n").unwrap();
}

fn append(path: &Path, line: &str) {
    let mut f = fs::OpenOptions::new().append(true).open(path).unwrap();
    writeln!(f, "{line}").unwrap();
}

fn permission_event(home: &Path, sid: &str, now: DateTime<Utc>) {
    let body = serde_json::json!({
        "session_id": sid, "hook_event_name": "Notification",
        "notification_type": "permission_prompt",
        "message": "Claude needs your permission to use Bash",
    });
    write_event(&events_path(home), &body.to_string(), now).unwrap();
}

fn stop_event(home: &Path, sid: &str, now: DateTime<Utc>) {
    let body = serde_json::json!({"session_id": sid, "hook_event_name": "Stop"});
    write_event(&events_path(home), &body.to_string(), now).unwrap();
}

fn engine(home: &Path, probe: &SharedProbe, hooks: bool) -> Engine {
    Engine::new(home.to_path_buf(), Box::new(probe.clone()), hooks)
}

/// The single `AgentUpdated` in `changes`, panicking otherwise.
fn one_update(changes: Vec<Change>) -> Agent {
    assert_eq!(changes.len(), 1, "expected one change, got {changes:#?}");
    match changes.into_iter().next().unwrap() {
        Change::AgentUpdated { agent } => agent,
        other => panic!("expected AgentUpdated, got {other:#?}"),
    }
}

fn status_of(e: &Engine, id: &str) -> Status {
    e.agents().into_iter().find(|a| a.id == id).unwrap().status
}

/// A live busy session `sid` at pid with a one-prompt transcript.
fn live_session(home: &Path, probe: &SharedProbe, pid: u32, sid: &str) -> PathBuf {
    write_session(home, pid, sid, CWD, "busy");
    probe.set_running(pid, true);
    let t = transcript(home, sid);
    write_lines(&t, &[user_prompt("hello", &ts(0))]);
    t
}

#[test]
fn load_adds_agents() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    live_session(home, &probe, 100, "a");
    live_session(home, &probe, 200, "b");
    let mut e = engine(home, &probe, true);
    let changes = e.load(at(10));
    assert_eq!(changes.len(), 2);
    assert!(changes
        .iter()
        .all(|c| matches!(c, Change::AgentAdded { .. })));
    let ids: Vec<_> = e.agents().into_iter().map(|a| a.id).collect();
    assert_eq!(ids, ["a", "b"]);
    assert_eq!(status_of(&e, "a"), Status::Working);
    assert_eq!(e.transcript_path("a", None), Some(transcript(home, "a")));
}

#[test]
fn lifecycle_sequence() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    let t = live_session(home, &probe, 100, "a");
    let mut e = engine(home, &probe, true);
    e.load(at(1));
    assert_eq!(status_of(&e, "a"), Status::Working);

    // Tool call appears in the transcript, then the permission hook fires.
    append(&t, &assistant_tool_use("m1", "tu1", "Bash", &ts(2)));
    let a = one_update(e.handle(Input::Transcript(t.clone()), at(3)));
    assert_eq!(a.status, Status::Working);
    permission_event(home, "a", at(4));
    let a = one_update(e.handle(Input::Events, at(4)));
    assert_eq!(
        a.status,
        Status::NeedsPermission {
            tool: Some("Bash".into())
        }
    );
    assert_eq!(a.status_since, at(4));

    // Approved: a later tool_result clears the hook.
    append(&t, &tool_result("tu1", &ts(6)));
    let a = one_update(e.handle(Input::Transcript(t.clone()), at(6)));
    assert_eq!(a.status, Status::Working);

    // Turn ends: registry idle and a Stop event, picked up in one rescan.
    write_session(home, 100, "a", CWD, "idle");
    stop_event(home, "a", at(8));
    let a = one_update(e.handle(Input::Rescan, at(8)));
    assert_eq!(a.status, Status::WaitingInput);

    // Process exits; the next probe is due 5 s after the last one.
    probe.set_running(100, false);
    assert!(e.handle(Input::Tick, at(10)).is_empty());
    let a = one_update(e.handle(Input::Tick, at(14)));
    assert_eq!(a.status, Status::Exited);
    assert_eq!(a.status_since, at(14));
}

#[test]
fn no_change_no_event() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    live_session(home, &probe, 100, "a");
    let mut e = engine(home, &probe, true);
    e.load(at(1));
    assert!(e.handle(Input::Tick, at(2)).is_empty());
    assert!(e.handle(Input::Tick, at(10)).is_empty()); // re-probed, still alive
    assert!(e.handle(Input::Rescan, at(11)).is_empty());
    assert!(e.handle(Input::Events, at(12)).is_empty());
}

#[test]
fn agent_tool_not_waiting() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    let t = live_session(home, &probe, 100, "a");
    append(&t, &assistant_tool_use("m1", "tu1", "Agent", &ts(1)));
    let mut e = engine(home, &probe, false);
    e.load(at(2));
    assert!(e.handle(Input::Tick, at(61)).is_empty());
    assert_eq!(status_of(&e, "a"), Status::Working);
}

#[test]
fn maybe_waiting_after_tick_when_hooks_off() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    let t = live_session(home, &probe, 100, "a");
    append(&t, &assistant_tool_use("m1", "tu1", "Bash", &ts(1)));
    let mut e = engine(home, &probe, false);
    e.load(at(2));
    assert_eq!(status_of(&e, "a"), Status::Working);
    let a = one_update(e.handle(Input::Tick, at(7)));
    assert_eq!(a.status, Status::MaybeWaiting);
    // Installing hooks does not disable the heuristic for a session that was
    // already running (it never loaded the hooks; R25).
    assert!(e.set_hooks_installed(true, at(8)).is_empty());
    assert_eq!(status_of(&e, "a"), Status::MaybeWaiting);
}

/// R25 (1): a session started before install, busy with Bash open 6 s, stays
/// `maybeWaiting` after install.
#[test]
fn hooks_inactive_for_session_started_before_install() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    let t = live_session(home, &probe, 100, "a"); // started before at(0)
    let mut e = engine(home, &probe, false);
    e.load(at(1));
    e.set_hooks_installed(true, at(2));
    append(&t, &assistant_tool_use("m1", "tu1", "Bash", &ts(3)));
    e.handle(Input::Transcript(t.clone()), at(3));
    assert_eq!(status_of(&e, "a"), Status::Working);
    let a = one_update(e.handle(Input::Tick, at(9)));
    assert_eq!(a.status, Status::MaybeWaiting);
}

/// R25 (2): the same session after one accepted hook event counts as hooked.
#[test]
fn hooks_active_after_session_delivers_event() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    let t = live_session(home, &probe, 100, "a");
    let mut e = engine(home, &probe, false);
    e.load(at(1));
    e.set_hooks_installed(true, at(2));
    permission_event(home, "a", at(3));
    let a = one_update(e.handle(Input::Events, at(3)));
    assert!(matches!(a.status, Status::NeedsPermission { .. }));
    append(&t, &tool_result("tu0", &ts(4)));
    append(&t, &assistant_tool_use("m1", "tu1", "Bash", &ts(5)));
    let a = one_update(e.handle(Input::Transcript(t.clone()), at(5)));
    assert_eq!(a.status, Status::Working);
    // Bash open 6 s: the heuristic is off for this session now.
    assert!(e.handle(Input::Tick, at(11)).is_empty());
    assert_eq!(status_of(&e, "a"), Status::Working);
}

/// R25 (3): a session started after install counts as hooked.
#[test]
fn hooks_active_for_session_started_after_install() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    let mut e = engine(home, &probe, false);
    e.load(at(1));
    e.set_hooks_installed(true, at(2));
    write_session_at(home, 100, "a", CWD, "busy", at(5).timestamp_millis());
    probe.set_running(100, true);
    let t = transcript(home, "a");
    write_lines(
        &t,
        &[
            user_prompt("hello", &ts(5)),
            assistant_tool_use("m1", "tu1", "Bash", &ts(6)),
        ],
    );
    e.handle(Input::Registry, at(6));
    assert_eq!(status_of(&e, "a"), Status::Working);
    assert!(e.handle(Input::Tick, at(13)).is_empty());
    assert_eq!(status_of(&e, "a"), Status::Working);
}

/// Hooks already installed at load: the install time is the settings file's
/// mtime; sessions started before it keep the heuristic, later ones don't.
#[test]
fn hooks_since_at_load_is_settings_mtime() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    fs::write(home.join("settings.json"), "{}").unwrap();
    let now = Utc::now();
    let old = (now - Duration::hours(1)).timestamp_millis();
    let new = (now + Duration::hours(1)).timestamp_millis();
    for (pid, sid, started) in [(100, "old", old), (200, "new", new)] {
        write_session_at(home, pid, sid, CWD, "busy", started);
        probe.set_running(pid, true);
        let ts0 = (now - Duration::seconds(30)).to_rfc3339_opts(SecondsFormat::Millis, true);
        let ts1 = (now - Duration::seconds(20)).to_rfc3339_opts(SecondsFormat::Millis, true);
        write_lines(
            &transcript(home, sid),
            &[
                user_prompt("hi", &ts0),
                assistant_tool_use("m1", "tu1", "Bash", &ts1),
            ],
        );
    }
    let mut e = engine(home, &probe, true);
    e.load(now);
    assert_eq!(status_of(&e, "old"), Status::MaybeWaiting);
    assert_eq!(status_of(&e, "new"), Status::Working);
}

/// Uninstall then reinstall: the install time moves to the reinstall.
#[test]
fn reinstall_restamps_hooks_since() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    let mut e = engine(home, &probe, false);
    e.load(at(1));
    e.set_hooks_installed(true, at(2));
    e.set_hooks_installed(false, at(3));
    write_session_at(home, 100, "a", CWD, "busy", at(4).timestamp_millis());
    probe.set_running(100, true);
    write_lines(
        &transcript(home, "a"),
        &[
            user_prompt("hello", &ts(4)),
            assistant_tool_use("m1", "tu1", "Bash", &ts(5)),
        ],
    );
    e.handle(Input::Registry, at(5));
    e.set_hooks_installed(true, at(6));
    // Started while hooks were off, so it never loaded them.
    e.handle(Input::Tick, at(12));
    assert_eq!(status_of(&e, "a"), Status::MaybeWaiting);
}

#[test]
fn unreadable_transcript_is_unknown() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    write_session(home, 100, "a", CWD, "busy");
    probe.set_running(100, true);
    fs::create_dir_all(transcript(home, "a")).unwrap();
    let mut e = engine(home, &probe, true);
    e.load(at(1));
    assert!(matches!(status_of(&e, "a"), Status::Unknown { .. }));
}

#[test]
fn missing_transcript_attaches_later() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    write_session(home, 100, "a", CWD, "busy");
    probe.set_running(100, true);
    let mut e = engine(home, &probe, true);
    e.load(at(1));
    assert_eq!(status_of(&e, "a"), Status::Working);
    assert_eq!(e.transcript_path("a", None), None);

    let t = transcript(home, "a");
    write_lines(&t, &[user_prompt("late start", &ts(2))]);
    let a = one_update(e.handle(Input::Transcript(t.clone()), at(3)));
    assert_eq!(a.last_activity, Some(at(2)));
    assert_eq!(a.transcript_path, Some(t.display().to_string()));
}

#[test]
fn stale_events_ignored_on_load() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    live_session(home, &probe, 100, "a");
    permission_event(home, "a", at(5));
    let mut e = engine(home, &probe, true);
    e.load(at(5) + Duration::minutes(11));
    assert_eq!(status_of(&e, "a"), Status::Working);
}

#[test]
fn recent_event_replayed_on_load() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    live_session(home, &probe, 100, "a");
    permission_event(home, "a", at(5));
    let mut e = engine(home, &probe, true);
    e.load(at(5) + Duration::minutes(9));
    assert!(matches!(status_of(&e, "a"), Status::NeedsPermission { .. }));
}

#[test]
fn replayed_event_does_not_resurrect_cleared_hook() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    let t = live_session(home, &probe, 100, "a");
    let mut e = engine(home, &probe, true);
    e.load(at(1));
    permission_event(home, "a", at(2));
    e.handle(Input::Events, at(2));
    append(&t, &tool_result("tu1", &ts(3)));
    e.handle(Input::Transcript(t.clone()), at(3));
    assert_eq!(status_of(&e, "a"), Status::Working);

    // The events file is replaced (trimmed): the old event is re-delivered.
    let ev = events_path(home);
    let body = fs::read(&ev).unwrap();
    fs::remove_file(&ev).unwrap();
    fs::write(&ev, body).unwrap();
    assert!(e.handle(Input::Events, at(4)).is_empty());
    assert_eq!(status_of(&e, "a"), Status::Working);
}

#[test]
fn event_for_unknown_session_ignored() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    live_session(home, &probe, 100, "a");
    let mut e = engine(home, &probe, true);
    e.load(at(1));
    permission_event(home, "zzz", at(2));
    assert!(e.handle(Input::Events, at(2)).is_empty());
}

#[test]
fn session_id_change_same_pid() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    live_session(home, &probe, 100, "a");
    let mut e = engine(home, &probe, true);
    e.load(at(1));

    // `/clear` in the same terminal: 100.json now names session b.
    write_session(home, 100, "b", CWD, "idle");
    let changes = e.handle(Input::Registry, at(2));
    assert_eq!(changes.len(), 2, "{changes:#?}");
    let mut saw_exit = false;
    let mut saw_add = false;
    for c in changes {
        match c {
            Change::AgentUpdated { agent } if agent.id == "a" => {
                assert_eq!(agent.status, Status::Exited);
                saw_exit = true;
            }
            Change::AgentAdded { agent } if agent.id == "b" => {
                assert_eq!(agent.status, Status::Idle);
                assert_eq!(agent.transcript_path, None);
                saw_add = true;
            }
            other => panic!("unexpected {other:#?}"),
        }
    }
    assert!(saw_exit && saw_add);
}

#[test]
fn exited_removed_after_24h() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    // Dead at load: exited_at = registry updatedAt.
    write_session(home, 100, "a", CWD, "idle");
    let updated = Utc.timestamp_millis_opt(1793281174333 + 20000).unwrap();
    let mut e = engine(home, &probe, true);
    let changes = e.load(updated + Duration::hours(1));
    assert_eq!(changes.len(), 1);
    assert_eq!(status_of(&e, "a"), Status::Exited);

    let limit = updated + Duration::hours(24);
    assert!(e.handle(Input::Tick, limit).is_empty());
    let changes = e.handle(Input::Tick, limit + Duration::seconds(1));
    assert_eq!(changes, vec![Change::AgentRemoved { id: "a".into() }]);
    assert!(e.agents().is_empty());
    // Its registry file still exists; it must not come back.
    assert!(e
        .handle(Input::Registry, limit + Duration::seconds(2))
        .is_empty());
    assert!(e
        .handle(Input::Rescan, limit + Duration::seconds(3))
        .is_empty());
}

#[test]
fn old_dead_session_skipped_at_load() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    write_session(home, 100, "a", CWD, "idle");
    let updated = Utc.timestamp_millis_opt(1793281174333 + 20000).unwrap();
    let mut e = engine(home, &probe, true);
    assert!(e.load(updated + Duration::hours(25)).is_empty());
    assert!(e
        .handle(Input::Registry, updated + Duration::hours(26))
        .is_empty());
}

#[test]
fn probe_error_keeps_state() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    live_session(home, &probe, 100, "a");
    let mut e = engine(home, &probe, true);
    e.load(at(1));
    probe.set_running(100, false);
    probe.set_fail(true);
    assert!(e.handle(Input::Tick, at(10)).is_empty());
    assert_eq!(status_of(&e, "a"), Status::Working);
}

#[test]
fn subagent_appears() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    let t = live_session(home, &probe, 100, "a");
    let mut e = engine(home, &probe, true);
    e.load(at(1));

    write_subagent(
        &t,
        "x",
        &[
            user_prompt("go", &ts(2)),
            assistant_tool_use("m1", "tu1", "Read", &ts(3)),
        ],
        None,
    );
    let sub = subagents_dir(&t).join("agent-x.jsonl");
    let a = one_update(e.handle(Input::Transcript(sub.clone()), at(3)));
    assert_eq!(a.subagents.len(), 1);
    assert_eq!(a.subagents[0].id, "x");
    assert_eq!(a.subagents[0].status, SubagentState::Running);
    assert_eq!(e.transcript_path("a", Some("x")), Some(sub));

    // Parent exits: the open subagent becomes stopped.
    probe.set_running(100, false);
    let a = one_update(e.handle(Input::Tick, at(7)));
    assert_eq!(a.status, Status::Exited);
    assert_eq!(a.subagents[0].status, SubagentState::Stopped);
}

#[test]
fn unrelated_path_ignored() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    live_session(home, &probe, 100, "a");
    let mut e = engine(home, &probe, true);
    e.load(at(1));
    let other = home.join("projects/x/other.jsonl");
    write_lines(&other, &[user_prompt("hi", &ts(2))]);
    assert!(e.handle(Input::Transcript(other), at(2)).is_empty());
}

#[test]
fn unreadable_registry_keeps_state() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    live_session(home, &probe, 100, "a");
    let mut e = engine(home, &probe, true);
    e.load(at(1));
    permission_event(home, "a", at(2));
    let a = one_update(e.handle(Input::Events, at(2)));
    assert!(matches!(a.status, Status::NeedsPermission { .. }));

    // Torn read: the registry file is garbage for one round.
    let reg = home.join("sessions/100.json");
    let good = fs::read(&reg).unwrap();
    fs::write(&reg, "{\"pid\": 10").unwrap();
    let a = one_update(e.handle(Input::Registry, at(3)));
    assert!(matches!(a.status, Status::Unknown { .. }), "{:?}", a.status);
    assert!(e.handle(Input::Registry, at(4)).is_empty());
    // Still alive (no exited_at): a Tick does not expire or exit it.
    assert!(e.handle(Input::Tick, at(5)).is_empty());

    // Restored: the hook survived.
    fs::write(&reg, good).unwrap();
    let a = one_update(e.handle(Input::Registry, at(6)));
    assert_eq!(
        a.status,
        Status::NeedsPermission {
            tool: Some("Bash".into())
        }
    );
}

#[test]
fn exit_clears_stopped_at() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    live_session(home, &probe, 100, "a");
    write_session(home, 100, "a", CWD, "idle");
    let mut e = engine(home, &probe, true);
    e.load(at(1));
    stop_event(home, "a", at(2));
    let a = one_update(e.handle(Input::Events, at(2)));
    assert_eq!(a.status, Status::WaitingInput);

    probe.set_running(100, false);
    let a = one_update(e.handle(Input::Tick, at(7)));
    assert_eq!(a.status, Status::Exited);

    // The session comes back (changed registry entry, process running):
    // the Stop from before the exit no longer counts.
    probe.set_running(100, true);
    write_session_at(home, 100, "a", CWD, "idle", 1793281174333 + 1);
    let a = one_update(e.handle(Input::Registry, at(8)));
    assert_eq!(a.status, Status::Idle);
}

#[test]
fn tick_probes_due_sessions_in_one_batch() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    live_session(home, &probe, 100, "a");
    live_session(home, &probe, 200, "b");
    live_session(home, &probe, 300, "c");
    let mut e = engine(home, &probe, true);
    e.load(at(1));
    probe.take_batches();

    // Not due yet: no probe at all.
    e.handle(Input::Tick, at(3));
    assert!(probe.take_batches().is_empty());

    // All three due: one batched call; a dead pid becomes exited.
    probe.set_running(200, false);
    let changes = e.handle(Input::Tick, at(6));
    assert_eq!(probe.take_batches(), vec![vec![100, 200, 300]]);
    assert_eq!(one_update(changes).status, Status::Exited);
    assert_eq!(status_of(&e, "a"), Status::Working);

    // Exited sessions are not probed again.
    e.handle(Input::Tick, at(12));
    assert_eq!(probe.take_batches(), vec![vec![100, 300]]);
}

#[test]
fn failed_batch_probe_keeps_every_session_alive() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    for (pid, sid) in [(100, "a"), (200, "b"), (300, "c")] {
        live_session(home, &probe, pid, sid);
    }
    let mut e = engine(home, &probe, true);
    e.load(at(1));
    probe.take_batches();

    // The batched probe fails for every pid: liveness is unknown, so nobody
    // flips to exited (spec §7), even though the pids are gone meanwhile.
    for pid in [100, 200, 300] {
        probe.set_running(pid, false);
    }
    probe.set_fail(true);
    assert!(e.handle(Input::Tick, at(10)).is_empty());
    assert_eq!(probe.take_batches(), vec![vec![100, 200, 300]]);
    for sid in ["a", "b", "c"] {
        assert_eq!(status_of(&e, sid), Status::Working);
    }

    // Once the probe works again, the dead pids are seen as exited.
    probe.set_fail(false);
    let changes = e.handle(Input::Tick, at(16));
    assert_eq!(changes.len(), 3);
    for sid in ["a", "b", "c"] {
        assert_eq!(status_of(&e, sid), Status::Exited);
    }
}

/// `Input::HooksInstalled` is the same as `set_hooks_installed`.
#[test]
fn hooks_installed_input_matches_setter() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path();
    let probe = SharedProbe::default();
    let mut e = engine(home, &probe, false);
    e.load(at(1));
    e.handle(Input::HooksInstalled(true), at(2));
    write_session_at(home, 100, "a", CWD, "busy", at(3).timestamp_millis());
    probe.set_running(100, true);
    write_lines(
        &transcript(home, "a"),
        &[
            user_prompt("hello", &ts(3)),
            assistant_tool_use("m1", "tu1", "Bash", &ts(4)),
        ],
    );
    e.handle(Input::Registry, at(4));
    e.handle(Input::Tick, at(11));
    assert_eq!(status_of(&e, "a"), Status::Working);
    // Removing hooks turns the heuristic back on.
    let a = one_update(e.handle(Input::HooksInstalled(false), at(12)));
    assert_eq!(a.status, Status::MaybeWaiting);
}
