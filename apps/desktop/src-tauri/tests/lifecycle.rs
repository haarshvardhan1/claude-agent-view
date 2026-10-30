//! Pure lifecycle rules: activation selects a recent notification's agent;
//! the notifier's focused agent.

use chrono::{DateTime, Duration, Utc};
use desktop_lib::commands::permission_name;
use desktop_lib::lifecycle::{LastNotified, focused_agent, take_select};
use desktop_lib::runtime::Runtime;
use desktop_lib::settings::AppSettings;
use tauri::plugin::PermissionState;

fn t0() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-30T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn slot() -> Option<LastNotified> {
    Some(LastNotified {
        agent_id: "a1".into(),
        at: t0(),
    })
}

#[test]
fn activation_within_60s_selects_once() {
    let mut s = slot();
    assert_eq!(
        take_select(&mut s, t0() + Duration::seconds(60)).as_deref(),
        Some("a1")
    );
    assert_eq!(s, None);
    // Cleared after use: a second activation selects nothing.
    assert_eq!(take_select(&mut s, t0() + Duration::seconds(61)), None);
}

#[test]
fn activation_after_60s_does_not_select_and_clears() {
    let mut s = slot();
    assert_eq!(take_select(&mut s, t0() + Duration::seconds(61)), None);
    assert_eq!(s, None);
}

#[test]
fn activation_without_notification_selects_nothing() {
    assert_eq!(take_select(&mut None, t0()), None);
}

#[test]
fn focused_agent_requires_focus_and_visibility() {
    assert_eq!(focused_agent(Some("a"), true, true).as_deref(), Some("a"));
    assert_eq!(focused_agent(Some("a"), false, true), None);
    assert_eq!(focused_agent(Some("a"), true, false), None);
    assert_eq!(focused_agent(None, true, true), None);
}

#[test]
fn runtime_tracks_focus_and_last_notified() {
    let dir = tempfile::tempdir().unwrap();
    let rt = Runtime::new(
        dir.path().join("claude"),
        dir.path().join("settings"),
        AppSettings::default(),
        Box::new(av_core::SystemProbe),
    );
    rt.set_selected_agent(Some("a".into()));
    assert_eq!(rt.focused_agent(), None, "not focused yet");
    rt.set_main_focused(true);
    assert_eq!(rt.focused_agent().as_deref(), Some("a"));
    rt.set_main_visible(false);
    assert_eq!(rt.focused_agent(), None, "hidden window");

    rt.note_notified("b", t0());
    assert_eq!(
        rt.take_activation_select(t0() + Duration::seconds(5))
            .as_deref(),
        Some("b")
    );
    assert_eq!(rt.take_activation_select(t0() + Duration::seconds(6)), None);
}

#[test]
fn permission_names() {
    assert_eq!(permission_name(Some(PermissionState::Granted)), "granted");
    assert_eq!(permission_name(Some(PermissionState::Denied)), "denied");
    assert_eq!(permission_name(Some(PermissionState::Prompt)), "unknown");
    assert_eq!(permission_name(None), "unknown");
}
