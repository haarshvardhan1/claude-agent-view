//! Pure tray model: grouping, caps, ordering, labels and the title count.

use av_core::model::{Agent, Context, Status, Tokens};
use chrono::{DateTime, Duration, Utc};
use desktop_lib::tray::{TrayItem, tray_model};

fn t0() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-30T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn agent(id: &str, status: Status, activity_secs: i64) -> Agent {
    Agent {
        id: id.into(),
        pid: 1,
        cwd: "/work/proj".into(),
        project_name: "proj".into(),
        git_branch: None,
        title: format!("title {id}"),
        entrypoint: "cli".into(),
        model: None,
        status,
        status_since: t0(),
        started_at: t0(),
        last_activity: Some(t0() + Duration::seconds(activity_secs)),
        context: Context {
            used: None,
            limit: None,
        },
        tokens: Tokens::default(),
        pending_tool: None,
        subagents: vec![],
        transcript_path: None,
        skipped_lines: 0,
    }
}

#[test]
fn tray_model_groups_and_caps() {
    let mut agents = Vec::new();
    for i in 0..12 {
        let status = match i % 3 {
            0 => Status::NeedsPermission { tool: None },
            1 => Status::WaitingInput,
            _ => Status::Error { kind: None },
        };
        agents.push(agent(&format!("n{i}"), status, i));
    }
    for i in 0..3 {
        agents.push(agent(&format!("w{i}"), Status::Working, i));
    }
    for i in 0..2 {
        agents.push(agent(&format!("i{i}"), Status::Idle, 100 + i));
    }

    let (title, needs, busy) = tray_model(&agents);
    assert_eq!(title.as_deref(), Some("12"));
    assert_eq!(needs.len(), 10);
    assert_eq!(busy.len(), 3);
    // Most recent activity first: n11 down to n2 (n0, n1 are cut).
    let ids: Vec<&str> = needs.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(
        ids,
        ["n11", "n10", "n9", "n8", "n7", "n6", "n5", "n4", "n3", "n2"]
    );
    assert_eq!(
        busy,
        vec![
            TrayItem {
                id: "w2".into(),
                label: "● title w2 — proj".into()
            },
            TrayItem {
                id: "w1".into(),
                label: "● title w1 — proj".into()
            },
            TrayItem {
                id: "w0".into(),
                label: "● title w0 — proj".into()
            },
        ]
    );

    // Zero needs-you: no title; idle agents never listed.
    let (title, needs, busy) = tray_model(&agents[12..]);
    assert_eq!(title, None);
    assert!(needs.is_empty());
    assert_eq!(busy.len(), 3);
}

#[test]
fn tray_model_cuts_titles_and_skips_maybe_waiting_count() {
    let mut a = agent("a", Status::MaybeWaiting, 0);
    a.title = "é".repeat(50);
    let mut b = agent("b", Status::Working, 5);
    b.last_activity = None;
    let (title, needs, busy) = tray_model(&[a, b]);
    // `maybeWaiting` never counts toward needs-you, but is busy.
    assert_eq!(title, None);
    assert!(needs.is_empty());
    assert_eq!(busy[0].label, format!("● {} — proj", "é".repeat(40)));
    // No activity sorts last.
    assert_eq!(busy[1].id, "b");
}
