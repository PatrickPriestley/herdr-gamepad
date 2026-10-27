use crate::engine::*;
use crate::serve::*;

#[test]
fn only_inputs_that_route_something_need_a_snapshot() {
    let voice = Btn::RStick;
    assert!(!needs_snapshot(&Input::Press(voice), voice), "starting a recording looks at nothing");
    assert!(needs_snapshot(&Input::Press(Btn::A), voice));
    assert!(needs_snapshot(&Input::Press(Btn::Up), voice));
    assert!(needs_snapshot(&Input::Release(voice), voice), "the release captures where the user spoke");
    assert!(!needs_snapshot(&Input::Release(Btn::A), voice));
    assert!(!needs_snapshot(&Input::Connected, voice));
    assert!(!needs_snapshot(&Input::Disconnected, voice));
    assert!(needs_snapshot(&Input::Transcript("x".into()), voice));
    assert!(!needs_snapshot(&Input::TranscriptFailed("x".into()), voice));
    assert!(needs_snapshot(&Input::Press(Btn::RStick), Btn::Menu), "the voice button is whatever the config says");
    assert!(!needs_snapshot(&Input::Press(Btn::Menu), Btn::Menu));
}

#[test]
fn without_a_snapshot_only_state_clearing_inputs_reach_the_core() {
    assert_eq!(on_snapshot_failure(Input::Press(Btn::Up), "no server"), None);
    assert_eq!(on_snapshot_failure(Input::Press(Btn::A), "no server"), None);
    let out = on_snapshot_failure(Input::Transcript("x".into()), "no server").unwrap();
    assert!(matches!(&out, Input::TranscriptFailed(e) if e.contains("Herdr unavailable") && e.contains("no server")), "{out:?}");
    assert_eq!(on_snapshot_failure(Input::Disconnected, "no server"), Some(Input::Disconnected));
    assert_eq!(on_snapshot_failure(Input::TranscriptFailed("e".into()), "no server"), Some(Input::TranscriptFailed("e".into())));
    assert_eq!(on_snapshot_failure(Input::Release(Btn::RStick), "no server"), Some(Input::Release(Btn::RStick)));
}

fn check_transcript_feedback(status: Status) {
    use crate::herdr::Herdr;
    use crate::herdr_tests::Fake;
    for succeeds in [false, true] {
        let mut snap = crate::engine_tests::snap();
        snap.focused.as_mut().unwrap().status = Some(status);
        let mut core = Core::new(Btn::RStick);
        core.handle(Input::Press(Btn::RStick), &snap);
        core.handle(Input::Release(Btn::RStick), &snap);
        let actions = core.handle(Input::Transcript("hello".into()), &snap);
        let command = if status == Status::Idle { "herdr agent prompt" } else { "herdr pane send-text" };
        let fake = Fake::new(vec![
            (command, if succeeds { Ok("") } else { Err("delivery failed") }),
            ("herdr notification show", Ok("")),
        ]);
        apply_actions(&Herdr { runner: &fake }, actions, |_| panic!("unexpected recording"));
        let calls = fake.calls();
        assert_eq!(calls.len(), 2, "one delivery and exactly one outcome: {calls:?}");
        assert_eq!(calls[0], format!("{command} w5:p1 hello"));
        let outcome = if !succeeds { "Herdr refused" } else if status == Status::Idle { "Sent" } else { "Typed, not sent" };
        assert!(calls[1].starts_with(&format!("herdr notification show {outcome} --body ")), "{calls:?}");
    }
}

#[test]
fn prompt_delivery_feedback_tracks_the_command_result() { check_transcript_feedback(Status::Idle); }

#[test]
fn typed_delivery_feedback_tracks_the_command_result() { check_transcript_feedback(Status::Working); }

#[test]
fn a_failed_mode_toast_does_not_discard_the_key_press() {
    use crate::herdr::Herdr;
    use crate::herdr_tests::Fake;
    let mut snap = crate::engine_tests::snap();
    snap.focused.as_mut().unwrap().status = Some(Status::Blocked);
    let actions = Core::new(Btn::RStick).handle(Input::Press(Btn::Down), &snap);
    let fake = Fake::new(vec![
        ("herdr notification show", Err("notifications unavailable")),
        ("herdr agent send-keys", Ok("")),
    ]);
    apply_actions(&Herdr { runner: &fake }, actions, |_| panic!("unexpected recording"));
    assert_eq!(fake.calls().last().map(String::as_str), Some("herdr agent send-keys w5:p1 down"));
}
