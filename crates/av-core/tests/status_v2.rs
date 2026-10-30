use av_core::model::Status;
use av_core::status::{derive_status_v2, ActiveHook, HookSignal, StatusInputs};
use av_core::transcript::{PendingTool, TranscriptSummary};
use chrono::{DateTime, Duration, Utc};

fn now() -> DateTime<Utc> {
    "2026-10-30T12:00:00Z".parse().unwrap()
}

fn pending(name: &str, secs: i64) -> TranscriptSummary {
    TranscriptSummary {
        pending_tool: Some(PendingTool {
            name: name.into(),
            since: now() - Duration::seconds(secs),
        }),
        ..Default::default()
    }
}

fn run(
    alive: bool,
    reg: &str,
    s: &TranscriptSummary,
    hook: Option<&ActiveHook>,
    stopped: bool,
    hooks_installed: bool,
) -> Status {
    derive_status_v2(&StatusInputs {
        alive,
        registry_status: reg,
        summary: s,
        hook,
        stopped_at: stopped.then(now),
        hooks_installed,
        now: now(),
    })
}

#[test]
fn priority_table() {
    let perm = ActiveHook {
        signal: HookSignal::Permission {
            tool: Some("Bash".into()),
        },
        at: now(),
    };
    let wait = ActiveHook {
        signal: HookSignal::WaitingInput,
        at: now(),
    };
    let empty = TranscriptSummary::default();
    let err = TranscriptSummary {
        last_is_error: true,
        last_error_kind: Some("rate_limit".into()),
        ..Default::default()
    };
    let ended = TranscriptSummary {
        turn_ended_at: Some(now()),
        ..Default::default()
    };

    assert_eq!(
        run(false, "busy", &empty, Some(&perm), false, true),
        Status::Exited
    );
    assert_eq!(
        run(true, "busy", &err, Some(&perm), false, true),
        Status::NeedsPermission {
            tool: Some("Bash".into())
        }
    );
    assert_eq!(
        run(true, "idle", &err, None, false, true),
        Status::Error {
            kind: Some("rate_limit".into())
        }
    );
    assert_eq!(
        run(true, "busy", &empty, Some(&wait), false, true),
        Status::WaitingInput
    );
    assert_eq!(
        run(true, "idle", &ended, None, false, true),
        Status::WaitingInput
    );
    assert_eq!(
        run(true, "idle", &empty, None, true, true),
        Status::WaitingInput
    );
    assert_eq!(run(true, "idle", &empty, None, false, true), Status::Idle);
    assert_eq!(
        run(true, "busy", &pending("Bash", 6), None, false, false),
        Status::MaybeWaiting
    );
    assert_eq!(
        run(true, "busy", &pending("Bash", 6), None, false, true),
        Status::Working
    );
    assert_eq!(
        run(true, "busy", &pending("Agent", 60), None, false, false),
        Status::Working
    );
    assert_eq!(
        run(true, "busy", &pending("Bash", 4), None, false, false),
        Status::Working
    );
    assert_eq!(
        run(true, "busy", &empty, None, false, false),
        Status::Working
    );
    assert_eq!(
        run(true, "weird", &empty, None, false, true),
        Status::Unknown {
            reason: "weird".into()
        }
    );
}
