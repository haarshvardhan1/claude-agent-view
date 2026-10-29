mod common;
use av_core::status::{derive_status, AgentStatus};
use av_core::transcript::{PendingTool, TranscriptSummary};
use chrono::{DateTime, Duration, Utc};

fn now() -> DateTime<Utc> {
    "2026-10-29T14:00:00Z".parse().unwrap()
}

fn pending(secs: i64) -> TranscriptSummary {
    TranscriptSummary {
        pending_tool: Some(PendingTool { name: "Bash".into(), since: now() - Duration::seconds(secs) }),
        ..Default::default()
    }
}

#[test]
fn dead_is_exited() {
    let s = TranscriptSummary { last_is_error: true, ..Default::default() };
    assert_eq!(derive_status("busy", false, &s, now()), AgentStatus::Exited);
}

#[test]
fn error_beats_idle_and_busy() {
    let s = TranscriptSummary { last_is_error: true, ..Default::default() };
    assert_eq!(derive_status("idle", true, &s, now()), AgentStatus::Error);
    assert_eq!(derive_status("busy", true, &s, now()), AgentStatus::Error);
}

#[test]
fn idle_is_idle() {
    assert_eq!(derive_status("idle", true, &TranscriptSummary::default(), now()), AgentStatus::Idle);
}

#[test]
fn busy_with_old_pending_tool_is_maybe_waiting() {
    assert_eq!(derive_status("busy", true, &pending(6), now()), AgentStatus::MaybeWaiting);
}

#[test]
fn busy_with_exactly_five_seconds_is_working() {
    assert_eq!(derive_status("busy", true, &pending(5), now()), AgentStatus::Working);
}

#[test]
fn busy_with_fresh_pending_tool_is_working() {
    assert_eq!(derive_status("busy", true, &pending(2), now()), AgentStatus::Working);
}

#[test]
fn busy_without_pending_is_working() {
    assert_eq!(derive_status("busy", true, &TranscriptSummary::default(), now()), AgentStatus::Working);
}

#[test]
fn unknown_status_passes_through() {
    let st = derive_status("parked", true, &TranscriptSummary::default(), now());
    assert_eq!(st, AgentStatus::Unknown("parked".into()));
    assert_eq!(st.label(), "parked");
}

#[test]
fn labels() {
    assert_eq!(AgentStatus::Working.label(), "working");
    assert_eq!(AgentStatus::MaybeWaiting.label(), "waiting?");
    assert_eq!(AgentStatus::Idle.label(), "idle");
    assert_eq!(AgentStatus::Error.label(), "error");
    assert_eq!(AgentStatus::Exited.label(), "exited");
}
