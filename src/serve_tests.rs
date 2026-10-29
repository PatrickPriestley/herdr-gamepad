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
        apply_actions(&Herdr { runner: &fake }, actions, &mut crate::feedback::Feedback::default(), |_| panic!("unexpected recording"));
        let calls = fake.calls();
        assert_eq!(calls.len(), if succeeds {1} else {2}, "only failed delivery notifies: {calls:?}");
        assert_eq!(calls[0], format!("{command} w5:p1 hello"));
        if !succeeds {assert!(calls[1].starts_with("herdr notification show Herdr refused --body "), "{calls:?}");}
    }
}

#[test]
fn prompt_delivery_feedback_tracks_the_command_result() { check_transcript_feedback(Status::Idle); }

#[test]
fn working_prompt_delivery_feedback_tracks_the_command_result() { check_transcript_feedback(Status::Working); }

#[test]
fn quiet_mode_feedback_preserves_the_key_press() {
    use crate::herdr::Herdr;
    use crate::herdr_tests::Fake;
    let mut snap = crate::engine_tests::snap();
    snap.focused.as_mut().unwrap().status = Some(Status::Blocked);
    let mut core = Core::new(Btn::RStick);
    let mut actions = core.handle(Input::Press(Btn::A), &snap);
    actions.extend(core.handle(Input::Press(Btn::Down), &snap));
    let fake = Fake::new(vec![
        ("herdr notification show", Err("notifications unavailable")),
        ("herdr agent send-keys", Ok("")),
    ]);
    apply_actions(&Herdr { runner: &fake }, actions, &mut crate::feedback::Feedback::default(), |_| panic!("unexpected recording"));
    assert_eq!(fake.calls(), vec!["herdr agent send-keys w5:p1 enter", "herdr agent send-keys w5:p1 down"]);
}





#[test]
fn mapping_explains_close_selection_confirmation_and_voice_collision() {
    for text in ["ACTIONS MENU", "close actions require a separate confirmation", "if voice uses Menu, use left stick click."] {
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
    let inputs = next_inputs(|| {
        polls += 1;
        assert!(polls <= 64, "an event flood must yield before the next tick");
        Some(None)
    }, &rx);
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

#[test]
fn mapping_button_descriptions_align_with_navigation_rows() {
    let labels = ["Triangle (Xbox: Y)", "Square (Xbox: X)", "Cross (Xbox: A)", "Circle (Xbox: B)", "Create / Share (View)"];
    let rows: Vec<_> = MAPPING.lines().filter(|line| labels.iter().any(|label| line.starts_with(label))).collect();
    assert_eq!(rows.len(), 15);
    let expected = MAPPING.lines().find(|line| line.starts_with("D-pad ")).unwrap().find("Focus").unwrap();
    for row in rows {
        let after_label = &row[row.find(')').unwrap() + 1..];
        assert_eq!(row.len() - after_label.trim_start().len(), expected, "misaligned mapping row: {row}");
    }
}

#[test]
fn mapping_explains_non_agent_interact_keys_and_escape() {
    for text in ["INTERACT (NON-AGENT PANES)", "Enter Respond / Interact", "Send arrow keys to the pane", "Space / select", "Return to Navigate"] {
        assert!(MAPPING.contains(text), "mapping is missing {text}");
    }
}

#[test]
fn mapping_distinguishes_circle_navigation_from_share_escape() {
    assert_eq!(MAPPING.matches("Circle (Xbox: B)       Return to Navigate").count(),2);
    assert_eq!(MAPPING.matches("Create / Share (View)  Escape / back in the app").count(),2);
}

#[test]
fn mapping_explains_single_press_waiting_agent_confirmation() {
    assert!(MAPPING.contains("One Cross (Xbox: A) press answers a waiting agent, including from Navigate."));
    assert!(!MAPPING.contains("acknowledgement"));
}

#[test]
fn preview_transcript_cannot_insert_when_snapshot_fails() {
    let mut c=Core::new(Btn::RStick); c.set_preview_enabled(true);
    c.handle(Input::Press(Btn::RStick), &crate::engine_tests::snap());
    c.handle(Input::Release(Btn::RStick), &crate::engine_tests::snap());
    let input=on_snapshot_failure(&mut c,Input::Transcript("keep this".into()),"offline").unwrap();
    let actions=c.handle(input,&Snapshot::default());
    assert!(actions.iter().all(|a|matches!(a,Action::Toast{..})));
    assert!(!c.voice_busy());
}

#[test]
fn preview_log_records_readiness_without_transcript_content() {
    let mut core=Core::new(Btn::RStick);
    let input=Input::Transcript("private draft text".into());
    assert!(describe_for_log(&input,&core).contains("private draft text"));
    core.set_preview_enabled(true);
    core.handle(Input::Press(Btn::RStick),&crate::engine_tests::snap());
    assert_eq!(describe_for_log(&input,&core),"Transcript ready for preview");
    assert_eq!(describe_for_log(&Input::Release(Btn::RStick),&core),"Release(right stick click)");
}

#[test]
fn native_insertion_outcome_is_returned_without_a_submit() {
    use crate::herdr_tests::Fake;
    let target=crate::engine_tests::snap().focused.unwrap();
    for answer in [Ok("{}"),Err("offline")] {
        let pane=serde_json::json!({"result":{"panes":[{"focused":true,"pane_id":target.pane_id,"workspace_id":target.workspace_id,"tab_id":target.tab_id,"agent":target.agent,"agent_status":"idle"}]}}).to_string();
        let pane: &'static str=Box::leak(pane.into_boxed_str());
        let fake=Fake::new(vec![("herdr pane list",Ok(pane)),("herdr pane send-text",answer),("herdr notification show",Ok("{}"))]);
        let results=apply_actions(&crate::herdr::Herdr{runner:&fake},vec![Action::InsertText{target:target.clone(),text:"hello".into()}],&mut crate::feedback::Feedback::default(), |_|panic!("not recording"));
        assert_eq!(results,vec![NativeOutcome::Inserted(target.clone(),answer.map(|_|()).map_err(String::from))]);
        assert!(!fake.calls().iter().any(|s|s.contains("send-keys")||s.contains("agent prompt")));
    }
}

#[test]
fn native_submit_refusals_rearm_but_uncertain_delivery_never_retries() {
    use crate::herdr::NativeSubmitError;
    for refused in [true,false] {
        let snap=crate::engine_tests::snap(); let target=snap.focused.clone().unwrap();
        let mut core=Core::new(Btn::RStick);core.set_preview_enabled(true);
        core.handle(Input::Press(Btn::RStick),&snap);core.handle(Input::Release(Btn::RStick),&snap);
        core.handle(Input::Transcript("review".into()),&snap);
        finish_native(&mut core,NativeOutcome::Inserted(target.clone(),Ok(())));
        core.begin_tick();
        assert!(matches!(core.handle(Input::Press(Btn::A),&snap).first(),Some(Action::SubmitText{..})));
        let error=if refused {NativeSubmitError::Refused("moved".into())} else {NativeSubmitError::Unconfirmed("offline".into())};
        let notices=finish_native(&mut core,NativeOutcome::Submitted(target,Err(error)));
        assert!(notices.iter().any(|a|matches!(a,Action::Toast{..})));
        core.handle(Input::Release(Btn::A),&snap);core.begin_tick();
        assert_eq!(core.handle(Input::Press(Btn::A),&snap).iter().any(|a|matches!(a,Action::SubmitText{..})),refused);
    }
}

#[test]
fn temporary_snapshot_failure_preserves_native_submission() {
    let snap=crate::engine_tests::snap();
    let mut core=Core::new(Btn::RStick);core.set_preview_enabled(true);
    core.handle(Input::Press(Btn::RStick),&snap);core.handle(Input::Release(Btn::RStick),&snap);
    core.handle(Input::Transcript("review".into()),&snap);
    core.insertion_result(snap.focused.clone().unwrap(),Ok(()));core.begin_tick();
    assert_eq!(on_snapshot_failure(&mut core,Input::Press(Btn::A),"temporary"),None);
    core.handle(Input::Release(Btn::A),&snap);core.begin_tick();
    assert_eq!(core.handle(Input::Press(Btn::A),&snap),vec![Action::SubmitText{target:snap.focused.unwrap()}]);
}

#[test]
fn clear_feedback_reports_actual_command_result_without_enter() {
    use crate::herdr::{Herdr};
    use crate::herdr_tests::Fake;
    let target=crate::engine_tests::snap().focused.unwrap();
    let pane=serde_json::json!({"result":{"panes":[{"focused":true,"pane_id":target.pane_id,"workspace_id":target.workspace_id,"tab_id":target.tab_id,"agent":target.agent,"agent_status":"idle"}]}}).to_string();
    let pane: &'static str=Box::leak(pane.into_boxed_str());
    for result in [Ok("{}"),Err("offline")] {
        let fake=Fake::new(vec![("herdr pane list",Ok(pane)),("herdr agent send-keys",result)]);
        let outcomes=apply_actions(&Herdr{runner:&fake},vec![Action::ClearInput{target:target.clone()}],&mut crate::feedback::Feedback::default(), |_|panic!("not recording"));
        assert_eq!(outcomes.len(),1);
        let mut core=Core::new(Btn::RStick);
        let actions=finish_native(&mut core,outcomes.into_iter().next().unwrap());
        assert!(matches!(actions.as_slice(),[Action::Toast {title,..}] if title==if result.is_ok(){"Input line cleared"}else{"Clear unconfirmed"}));
        assert_eq!(fake.calls().last().unwrap(),&format!("herdr agent send-keys {} ctrl+a ctrl+k",target.pane_id));
    }
}
