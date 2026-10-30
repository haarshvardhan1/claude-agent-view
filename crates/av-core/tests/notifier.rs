use av_core::model::{Agent, Change, Context, Status, Subagent, SubagentState, Tokens};
use av_core::notifier::{Notifier, NotifyKind, NotifySettings};
use chrono::{DateTime, Duration, Utc};

fn t0() -> DateTime<Utc> {
    "2026-10-30T10:00:00Z".parse().unwrap()
}
fn at(secs: i64) -> DateTime<Utc> {
    t0() + Duration::seconds(secs)
}

fn sub(id: &str, background: bool, status: SubagentState) -> Subagent {
    Subagent {
        id: id.into(),
        agent_type: "Explore".into(),
        description: "look around".into(),
        background,
        status,
        last_activity: None,
        tokens: Tokens::default(),
        pending_tool: None,
        transcript_path: "/tmp/s.jsonl".into(),
    }
}

fn agent(id: &str, status: Status, subs: Vec<Subagent>) -> Agent {
    Agent {
        id: id.into(),
        pid: 1,
        cwd: "/p".into(),
        project_name: "p".into(),
        git_branch: None,
        title: "Fix bug".into(),
        entrypoint: "cli".into(),
        model: None,
        status,
        status_since: t0(),
        started_at: t0(),
        last_activity: None,
        context: Context {
            used: None,
            limit: None,
        },
        tokens: Tokens::default(),
        pending_tool: None,
        subagents: subs,
        transcript_path: None,
        skipped_lines: 0,
    }
}

fn added(id: &str, s: Status) -> Change {
    Change::AgentAdded {
        agent: agent(id, s, vec![]),
    }
}
fn upd(id: &str, s: Status) -> Change {
    Change::AgentUpdated {
        agent: agent(id, s, vec![]),
    }
}
fn perm() -> Status {
    Status::NeedsPermission {
        tool: Some("Bash".into()),
    }
}

struct H {
    n: Notifier,
    s: NotifySettings,
}
impl H {
    fn new() -> Self {
        Self {
            n: Notifier::new(),
            s: NotifySettings::default(),
        }
    }
    fn go(&mut self, c: Change, secs: i64) -> Vec<av_core::notifier::Notification> {
        self.n.on_changes(&[c], &self.s, None, at(secs))
    }
}

#[test]
fn settings_defaults_and_serde() {
    let d = NotifySettings::default();
    assert!(d.needs_permission && d.waiting_input && d.finished && d.error && d.subagent_finished);
    assert_eq!(d.finished_after_secs, 30);
    let s: NotifySettings =
        serde_json::from_str(r#"{"finished":false,"finishedAfterSecs":5}"#).unwrap();
    assert!(!s.finished && s.error);
    assert_eq!(s.finished_after_secs, 5);
}

#[test]
fn needs_permission_with_and_without_tool() {
    let mut h = H::new();
    h.go(added("a", Status::Working), 0);
    let n = h.go(upd("a", perm()), 1);
    assert_eq!(n.len(), 1);
    assert_eq!(n[0].kind, NotifyKind::NeedsPermission);
    assert_eq!(n[0].title, "Fix bug");
    assert_eq!(n[0].body, "Needs permission: Bash");
    h.go(upd("a", Status::Working), 2);
    let n = h.go(upd("a", Status::NeedsPermission { tool: None }), 20);
    assert_eq!(n[0].body, "Needs permission");
}

#[test]
fn error_with_and_without_kind() {
    let mut h = H::new();
    h.go(added("a", Status::Working), 0);
    let n = h.go(
        upd(
            "a",
            Status::Error {
                kind: Some("rate_limit".into()),
            },
        ),
        1,
    );
    assert_eq!(n[0].kind, NotifyKind::Error);
    assert_eq!(n[0].body, "API error: rate_limit");
    h.go(upd("a", Status::Working), 2);
    let n = h.go(upd("a", Status::Error { kind: None }), 20);
    assert_eq!(n[0].body, "API error");
}

#[test]
fn waiting_input_after_short_busy() {
    let mut h = H::new();
    h.go(added("a", Status::Working), 0);
    let n = h.go(upd("a", Status::WaitingInput), 5);
    assert_eq!(n.len(), 1);
    assert_eq!(n[0].kind, NotifyKind::WaitingInput);
    assert_eq!(n[0].body, "Waiting for your input");
}

#[test]
fn finished_body_format() {
    let mut h = H::new();
    h.go(added("a", Status::Idle), 0);
    h.go(upd("a", Status::Working), 10);
    let n = h.go(upd("a", Status::Idle), 55);
    assert_eq!(n[0].kind, NotifyKind::Finished);
    assert_eq!(n[0].body, "Finished after 0m 45s");
    h.go(upd("a", Status::Working), 100);
    let n = h.go(upd("a", Status::Idle), 100 + 185);
    assert_eq!(n[0].body, "Finished after 3m 5s");
}

#[test]
fn threshold_boundary() {
    let mut h = H::new();
    h.go(added("a", Status::Idle), 0);
    h.go(upd("a", Status::Working), 0);
    let n = h.go(upd("a", Status::WaitingInput), 29);
    assert_eq!(
        n.iter().map(|x| x.kind).collect::<Vec<_>>(),
        vec![NotifyKind::WaitingInput]
    );

    let mut h = H::new();
    h.go(added("a", Status::Idle), 0);
    h.go(upd("a", Status::Working), 0);
    let n = h.go(upd("a", Status::WaitingInput), 30);
    assert_eq!(
        n.iter().map(|x| x.kind).collect::<Vec<_>>(),
        vec![NotifyKind::Finished]
    );
}

#[test]
fn busy_since_kept_across_busy_changes() {
    let mut h = H::new();
    h.go(added("a", Status::Idle), 0);
    h.go(upd("a", Status::Working), 0);
    h.go(upd("a", Status::MaybeWaiting), 10);
    h.go(upd("a", perm()), 20);
    let n = h.go(upd("a", Status::Idle), 40);
    assert_eq!(n[0].kind, NotifyKind::Finished);
    assert_eq!(n[0].body, "Finished after 0m 40s");
}

#[test]
fn unknown_is_transparent() {
    let mut h = H::new();
    h.go(added("a", Status::Working), 0);
    assert_eq!(h.go(upd("a", perm()), 1).len(), 1);
    let unk = Status::Unknown {
        reason: "read".into(),
    };
    assert!(h.go(upd("a", unk), 5).is_empty());
    assert!(h.go(upd("a", perm()), 21).is_empty());
}

#[test]
fn unknown_keeps_busy_since() {
    let mut h = H::new();
    h.go(added("a", Status::Idle), 0);
    h.go(upd("a", Status::Working), 0);
    h.go(upd("a", Status::Unknown { reason: "x".into() }), 20);
    h.go(upd("a", Status::Working), 25);
    let n = h.go(upd("a", Status::Idle), 40);
    assert_eq!(n[0].body, "Finished after 0m 40s");
}

#[test]
fn exited_never_notifies() {
    let mut h = H::new();
    h.go(added("a", Status::Working), 0);
    assert!(h.go(upd("a", Status::Exited), 100).is_empty());
}

#[test]
fn removed_drops_state() {
    let mut h = H::new();
    h.go(added("a", Status::Working), 0);
    h.go(Change::AgentRemoved { id: "a".into() }, 1);
    // Re-seen via update: treated as first sight, no notification.
    assert!(h.go(upd("a", perm()), 2).is_empty());
}

#[test]
fn added_never_notifies() {
    let mut h = H::new();
    assert!(h.go(added("a", perm()), 0).is_empty());
    assert!(h.go(added("b", Status::Error { kind: None }), 0).is_empty());
    assert!(h.go(added("c", Status::WaitingInput), 0).is_empty());
}

#[test]
fn finished_suppresses_waiting_input() {
    let mut h = H::new();
    h.go(added("a", Status::Idle), 0);
    h.go(upd("a", Status::Working), 0);
    assert_eq!(
        h.go(upd("a", Status::Idle), 40)[0].kind,
        NotifyKind::Finished
    );
    assert!(h.go(upd("a", Status::WaitingInput), 45).is_empty());
}

#[test]
fn rate_limit_10s() {
    let mut h = H::new();
    h.go(added("a", Status::Working), 0);
    assert_eq!(h.go(upd("a", perm()), 1).len(), 1);
    h.go(upd("a", Status::Working), 2);
    assert!(h.go(upd("a", perm()), 6).is_empty());
    h.go(upd("a", Status::Working), 7);
    assert_eq!(h.go(upd("a", perm()), 17).len(), 1);
}

#[test]
fn dropped_notification_does_not_count_as_fired() {
    let mut h = H::new();
    h.go(added("a", Status::Working), 0);
    h.s.needs_permission = false;
    assert!(h.go(upd("a", perm()), 1).is_empty());
    h.s.needs_permission = true;
    h.go(upd("a", Status::Working), 2);
    assert_eq!(h.go(upd("a", perm()), 3).len(), 1);
}

#[test]
fn focused_suppresses() {
    let mut h = H::new();
    h.go(added("a", Status::Working), 0);
    let n = h.n.on_changes(&[upd("a", perm())], &h.s, Some("a"), at(1));
    assert!(n.is_empty());
    h.go(upd("a", Status::Working), 2);
    let n =
        h.n.on_changes(&[upd("a", perm())], &h.s, Some("other"), at(3));
    assert_eq!(n.len(), 1);
}

#[test]
fn setting_off_suppresses() {
    let mut h = H::new();
    h.go(added("a", Status::Working), 0);
    h.s.error = false;
    assert!(h.go(upd("a", Status::Error { kind: None }), 1).is_empty());
}

#[test]
fn maybe_waiting_silent() {
    let mut h = H::new();
    h.go(added("a", Status::Working), 0);
    assert!(h.go(upd("a", Status::MaybeWaiting), 10).is_empty());
}

fn sub_update(id: &str, subs: Vec<Subagent>) -> Change {
    Change::AgentUpdated {
        agent: agent(id, Status::Working, subs),
    }
}

#[test]
fn subagent_finished_background() {
    let mut h = H::new();
    h.go(
        Change::AgentAdded {
            agent: agent(
                "a",
                Status::Working,
                vec![sub("s", true, SubagentState::Running)],
            ),
        },
        0,
    );
    let n = h.go(
        sub_update("a", vec![sub("s", true, SubagentState::Done)]),
        5,
    );
    assert_eq!(n.len(), 1);
    assert_eq!(n[0].kind, NotifyKind::SubagentFinished);
    assert_eq!(n[0].title, "Fix bug");
    assert_eq!(n[0].body, "Subagent finished: look around");
}

#[test]
fn foreground_subagent_silent() {
    let mut h = H::new();
    h.go(
        Change::AgentAdded {
            agent: agent(
                "a",
                Status::Working,
                vec![sub("s", false, SubagentState::Running)],
            ),
        },
        0,
    );
    assert!(h
        .go(
            sub_update("a", vec![sub("s", false, SubagentState::Done)]),
            5
        )
        .is_empty());
}
