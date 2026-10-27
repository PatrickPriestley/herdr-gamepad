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
    let mut c = Core::new(Btn::RStick);
    assert_eq!(on_snapshot_failure(&mut c, Input::Press(Btn::Up), "no server"), None);
    assert_eq!(on_snapshot_failure(&mut c, Input::Press(Btn::A), "no server"), None);
    let out = on_snapshot_failure(&mut c, Input::Transcript("x".into()), "no server").unwrap();
    assert!(matches!(&out, Input::TranscriptFailed(e) if e.contains("Herdr unavailable") && e.contains("no server")), "{out:?}");
    assert_eq!(on_snapshot_failure(&mut c, Input::Disconnected, "no server"), Some(Input::Disconnected));
    assert_eq!(on_snapshot_failure(&mut c, Input::TranscriptFailed("e".into()), "no server"), Some(Input::TranscriptFailed("e".into())));
    assert_eq!(on_snapshot_failure(&mut c, Input::Release(Btn::RStick), "no server"), Some(Input::Release(Btn::RStick)));
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
        let command = "herdr agent prompt";
        let fake = Fake::new(vec![
            (command, if succeeds { Ok("") } else { Err("delivery failed") }),
            ("herdr notification show", Ok("")),
        ]);
        apply_actions(&Herdr { runner: &fake }, actions, |_| panic!("unexpected recording"));
        let calls = fake.calls();
        assert_eq!(calls.len(), 2, "one delivery and exactly one outcome: {calls:?}");
        assert_eq!(calls[0], format!("{command} w5:p1 hello"));
        let outcome = if !succeeds { "Herdr refused" } else { "Sent" };
        assert!(calls[1].starts_with(&format!("herdr notification show {outcome} --body ")), "{calls:?}");
    }
}

#[test]
fn prompt_delivery_feedback_tracks_the_command_result() { check_transcript_feedback(Status::Idle); }

#[test]
fn working_prompt_delivery_feedback_tracks_the_command_result() { check_transcript_feedback(Status::Working); }

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

#[test]
fn snapshot_failure_disarms_pending_close() {
    let s = crate::engine_tests::snap();
    let mut c = Core::new(Btn::RStick);
    assert_eq!(c.handle(Input::Press(Btn::Menu), &s), vec![toast("Close actions", "left face: close pane; top face: close space; right face: cancel", Sound::None)]);
    assert!(matches!(c.handle(Input::Press(Btn::X), &s).first(), Some(Action::Toast { title, .. }) if title == "Confirm close"));
    on_snapshot_failure(&mut c, Input::Press(Btn::A), "offline");
    assert_eq!(c.handle(Input::Press(Btn::A), &s), vec![toast("Respond mode", "D-pad moves; bottom confirms, right escapes; Back / Select leaves", Sound::None)]);
}

#[test]
fn close_requests_do_not_claim_completion_and_failure_is_reported() {
    use crate::herdr::Herdr;
    use crate::herdr_tests::Fake;
    for choice in [Btn::X, Btn::Y] { for success in [false, true] {
        let s = crate::engine_tests::snap(); let mut c = Core::new(Btn::RStick);
        assert_eq!(c.handle(Input::Press(Btn::Menu), &s), vec![toast("Close actions", "left face: close pane; top face: close space; right face: cancel", Sound::None)]);
        c.handle(Input::Press(choice), &s);
        let actions = c.handle(Input::Press(Btn::A), &s);
        let command = if choice == Btn::X { "herdr pane close w5:p1" } else { "herdr workspace close w5" };
        let fake = Fake::new(vec![(command, if success {Ok("")} else {Err("target gone")}), ("herdr notification show", Ok(""))]);
        apply_actions(&Herdr { runner: &fake }, actions, |_| panic!("unexpected recording"));
        let expected = if success { vec![command] } else { vec![command, "herdr notification show Herdr refused --body target gone --sound request"] };
        assert_eq!(fake.calls(), expected);
    }}
}

#[test]
fn mapping_explains_close_selection_confirmation_and_voice_collision() {
    for text in ["CLOSE ACTIONS", "Left face button       Select close pane", "Top face button        Select close space", "Bottom face button     Confirm selected close", "Menu / Options opens close actions; if voice uses Menu, use left stick click."] {
        assert!(MAPPING.contains(text), "mapping is missing {text}");
    }
}

#[test]
fn completed_transcript_does_not_need_a_controller_event() {
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(Input::Transcript("ready".into())).unwrap();
    let mut polled = 0;
    let inputs = next_inputs(|| { polled += 1; None }, &rx);
    assert_eq!(inputs, vec![Input::Transcript("ready".into())]);
    assert_eq!(polled, 1);
    // Paired with the real gilrs timing probe and channel tests above. The closure passed
    // to next_inputs must not hide gilrs's unbounded filtered blocking wait.
    let source = include_str!("serve.rs");
    let daemon = source.split("/// Apply one input").next().unwrap();
    assert!(daemon.contains("gilrs.next_event()"), "use gilrs nonblocking poll");
    assert!(!daemon.contains("next_event_blocking"));
}

#[test]
fn completed_transcript_precedes_a_new_recording_press() {
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(Input::Transcript("ready".into())).unwrap();
    let mut events = [Some(Input::Press(Btn::RStick))].into_iter();
    assert_eq!(next_inputs(|| events.next(), &rx),
        vec![Input::Transcript("ready".into()), Input::Press(Btn::RStick)]);
}

#[test]
fn an_idle_tick_is_throttled_and_poll_is_called_once() {
    let (_tx, rx) = std::sync::mpsc::channel();
    let mut calls = 0;
    let start = std::time::Instant::now();
    let inputs = next_inputs(|| { calls += 1; None }, &rx);
    assert_eq!(calls, 1);
    assert!(start.elapsed() >= std::time::Duration::from_millis(8), "idle polling must not spin");
    assert!(inputs.is_empty());
}

#[test]
fn ignored_controller_motion_does_not_queue_seconds_of_button_lag() {
    let (_tx, rx) = std::sync::mpsc::channel();
    let mut events = std::iter::repeat_with(|| None).take(200)
        .chain(std::iter::once(Some(Input::Press(Btn::A))));
    let started = std::time::Instant::now();
    let mut inputs = vec![];
    for _ in 0..201 {
        inputs.extend(next_inputs(|| events.next(), &rx));
        if !inputs.is_empty() { break; }
    }
    assert_eq!(inputs, vec![Input::Press(Btn::A)]);
    assert!(started.elapsed() < std::time::Duration::from_millis(250),
        "200 ignored events delayed the button by {:?}", started.elapsed());
}

#[test]
fn a_continuous_ignored_event_stream_has_a_finite_tick_budget() {
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(Input::Transcript("first".into())).unwrap();
    let mut polls = 0;
    let inputs = next_inputs(|| { polls += 1; Some(None) }, &rx);
    assert_eq!(polls, 64, "drain a bounded batch, even when every event is ignored");
    assert_eq!(inputs, vec![Input::Transcript("first".into())]);
    tx.send(Input::Transcript("second".into())).unwrap();
    let inputs = next_inputs(|| Some(None), &rx);
    assert_eq!(inputs, vec![Input::Transcript("second".into())]);
}

#[test]
fn a_tick_keeps_button_order_after_ready_voice_results() {
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(Input::Transcript("ready".into())).unwrap();
    let mut events = [None, Some(Input::Press(Btn::RStick)), None,
        Some(Input::Release(Btn::RStick)), Some(Input::Press(Btn::A))].into_iter();
    assert_eq!(next_inputs(|| events.next(), &rx), vec![Input::Transcript("ready".into()),
        Input::Press(Btn::RStick), Input::Release(Btn::RStick), Input::Press(Btn::A)]);
}
