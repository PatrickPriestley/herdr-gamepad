use crate::engine::*;
use crate::engine_tests::snap;
fn record(c: &mut Core, text: &str, s: &Snapshot) -> Vec<Action> {
    assert_eq!(c.handle(Input::Press(Btn::RStick), s), vec![Action::StartRecording]);
    assert_eq!(c.handle(Input::Release(Btn::RStick), s), vec![Action::StopRecording]);
    c.handle(Input::Transcript(text.into()), s)
}
#[test]
fn preview_inserts_in_native_field_instead_of_retaining_a_popup_draft() {
    let mut c=Core::new(Btn::RStick); c.set_preview_enabled(true);
    let actions=record(&mut c,"review me",&snap());
    assert_eq!(actions.len(),1,"must emit native insertion instead of retain popup draft");
    assert!(!matches!(actions[0],Action::Prompt{..}));
}
#[test]
fn off_after_a_preview_restores_automatic_submission() {
    let mut c=Core::new(Btn::RStick); c.set_preview_enabled(true);
    record(&mut c,"first",&snap()); c.set_preview_enabled(false);
    assert!(record(&mut c,"second",&snap()).iter().any(|a|matches!(a,Action::Prompt{..})));
}
fn native() -> Core { let mut c=Core::new(Btn::RStick); c.set_preview_enabled(true); c }
fn inserted(c:&mut Core) {
    record(c,"draft",&snap());
    c.insertion_result(snap().focused.unwrap(),Ok(()));
    c.begin_tick();
}
#[test]
fn acknowledged_insertion_submits_once_on_cross_without_reinserting() {
    let mut c=native(); inserted(&mut c);
    assert_eq!(c.handle(Input::Press(Btn::A),&snap()),vec![Action::SubmitText {target:snap().focused.unwrap()}]);
    assert!(c.handle(Input::Press(Btn::A),&snap()).is_empty());
    assert!(c.handle(Input::Release(Btn::A),&snap()).is_empty());
    assert!(c.handle(Input::Press(Btn::A),&snap()).iter().all(|a|!matches!(a,Action::SubmitText{..}|Action::SendKeys{..})));
}
#[test]
fn failed_or_stale_insertion_cannot_arm_submit() {
    let mut c=native(); record(&mut c,"draft",&snap());
    let mut other=snap().focused.unwrap(); other.pane_id="other".into();
    assert!(c.insertion_result(other,Ok(())).is_empty());
    let failed=c.insertion_result(snap().focused.unwrap(),Err("offline".into()));
    assert!(failed.iter().any(|a|matches!(a,Action::Toast {body,..} if body.contains("offline"))));
    assert!(c.insertion_result(snap().focused.unwrap(),Ok(())).is_empty());
    assert!(c.handle(Input::Press(Btn::A),&snap()).iter().all(|a|!matches!(a,Action::SubmitText{..}|Action::SendKeys{..})));
}
#[test]
fn preview_normalizes_whitespace_and_removes_terminal_controls() {
    let mut c=native();
    assert_eq!(record(&mut c,"  hello\n\tworld\r\u{1b}[201~\u{7}\u{85}é  ",&snap()),vec![Action::InsertText {target:snap().focused.unwrap(),text:"hello world [201~ é".into()}]);
    assert_eq!(record(&mut c,"\u{1b}\u{7}\n ",&snap()),vec![toast("Heard nothing","",Sound::None)]);
}
#[test]
fn preview_rejects_drift_before_insertion_and_submission() {
    for variant in 0..8 {
        let mut changed=snap(); let p=changed.focused.as_mut().unwrap();
        match variant {0=>p.pane_id="other".into(),1=>p.workspace_id="other".into(),2=>p.tab_id="other".into(),3=>p.agent=None,4=>p.agent=Some("other".into()),5=>p.status=Some(Status::Blocked),6=>p.status=None,_=>changed.focused=None}
        let mut c=native(); c.handle(Input::Press(Btn::RStick),&snap());c.handle(Input::Release(Btn::RStick),&snap());
        let out=c.handle(Input::Transcript("draft".into()),&changed);
        assert!(matches!(out.first(),Some(Action::Toast{..})));
        assert!(out.iter().all(|a|!matches!(a,Action::InsertText{..})));
        let mut c=native(); inserted(&mut c);
        assert!(c.handle(Input::Press(Btn::A),&changed).iter().all(|a|!matches!(a,Action::SubmitText{..}|Action::SendKeys{..})));
        c.handle(Input::Release(Btn::A),&changed);
        assert!(matches!(c.handle(Input::Press(Btn::A),&snap()).first(),Some(Action::SubmitText{..})));
    }
}
#[test]
fn off_clears_staged_state_without_erasing_native_text() {
    let mut c=native();inserted(&mut c);c.set_preview_enabled(false);
    assert!(c.handle(Input::Press(Btn::A),&snap()).iter().all(|a|!matches!(a,Action::SubmitText{..}|Action::SendKeys{..})));
    assert!(record(&mut c,"automatic",&snap()).iter().any(|a|matches!(a,Action::Prompt{..})));
}
#[test]
fn recording_snapshots_preview_mode_but_next_recording_obeys_off() {
    let mut c=native(); c.handle(Input::Press(Btn::RStick),&snap()); c.set_preview_enabled(false);
    c.handle(Input::Release(Btn::RStick),&snap());
    assert!(matches!(c.handle(Input::Transcript("first".into()),&snap()).first(),Some(Action::InsertText{..})));
    c.insertion_result(snap().focused.unwrap(),Ok(()));
    assert!(record(&mut c,"second",&snap()).iter().any(|a|matches!(a,Action::Prompt{..})));
}
#[test]
fn cross_during_recording_waits_and_circle_leaves_native_text() {
    let mut c=native();inserted(&mut c);c.handle(Input::Press(Btn::RStick),&snap());
    assert!(c.handle(Input::Press(Btn::A),&snap()).iter().all(|a|!matches!(a,Action::SubmitText{..})));
    c.handle(Input::Release(Btn::A),&snap());c.handle(Input::Release(Btn::RStick),&snap());c.handle(Input::TranscriptFailed("failed".into()),&snap());
    assert!(matches!(c.handle(Input::Press(Btn::A),&snap()).first(),Some(Action::SubmitText{..})));
    let mut c=native();inserted(&mut c);
    assert!(c.handle(Input::Press(Btn::B),&snap()).iter().all(|a|matches!(a,Action::Toast{..})));
    assert!(c.handle(Input::Press(Btn::A),&snap()).iter().all(|a|!matches!(a,Action::SubmitText{..}|Action::SendKeys{..})));
}
#[test]
fn empty_or_failed_recording_does_not_mutate_native_text() {
    let mut c=native(); inserted(&mut c);
    assert_eq!(record(&mut c," ",&snap()),vec![toast("Heard nothing","",Sound::None)]);
    c.handle(Input::Press(Btn::RStick),&snap());c.handle(Input::TranscriptFailed("microphone failed".into()),&snap());
    assert!(!c.voice_busy()); assert!(!c.preview_in_progress());
    assert!(matches!(c.handle(Input::Press(Btn::A),&snap()).first(),Some(Action::SubmitText{..})));
}

#[test]
fn preview_requires_agent_and_original_target_even_when_both_snapshots_match() {
    for missing in [false,true] {
        let mut s=snap();
        if missing { s.focused=None; } else { s.focused.as_mut().unwrap().agent=None; }
        let mut c=native();
        assert!(matches!(record(&mut c,"hello",&s).first(),Some(Action::Toast{..})));
    }
}
#[test]
fn preview_additional_recording_captures_current_target_and_busy_send_stays_blocked() {
    let mut c=native(); inserted(&mut c);
    c.handle(Input::Press(Btn::RStick),&snap()); c.handle(Input::Release(Btn::RStick),&snap());
    assert!(c.handle(Input::Press(Btn::A),&snap()).iter().all(|a|!matches!(a,Action::SubmitText{..})));
    c.handle(Input::Release(Btn::A),&snap());
    c.handle(Input::TranscriptFailed("failed".into()),&snap());
    let mut changed=snap(); changed.focused.as_mut().unwrap().pane_id="other".into();
    assert!(matches!(record(&mut c,"more",&changed).first(),Some(Action::InsertText{target,..}) if target.pane_id=="other"));
}
#[test]
fn disabling_cancels_pending_ack_and_preserves_ordinary_navigation() {
    let mut c=native();record(&mut c,"hello",&snap());c.set_preview_enabled(false);
    assert!(c.insertion_result(snap().focused.unwrap(),Ok(())).is_empty());
    assert!(c.handle(Input::Press(Btn::A),&snap()).iter().all(|a|!matches!(a,Action::SubmitText{..})));
    let mut c=native();inserted(&mut c);c.handle(Input::Press(Btn::Down),&snap());
    assert!(c.handle(Input::Press(Btn::A),&snap()).iter().all(|a|!matches!(a,Action::SubmitText{..})));
}
#[test]
fn saving_other_settings_while_preview_stays_on_keeps_staged_submit() {
    let mut c=native(); inserted(&mut c); c.set_preview_enabled(true);
    assert!(matches!(c.handle(Input::Press(Btn::A),&snap()).first(),Some(Action::SubmitText{..})));
}
#[test]
fn menu_release_ownership_cannot_leave_native_submit_latched() {
    let mut c=native(); inserted(&mut c);
    assert!(matches!(c.handle(Input::Press(Btn::A),&snap()).first(),Some(Action::SubmitText{..})));
    c.menu_exit(); // Opening Actions transfers held-key/release ownership to the menu.
    c.menu_exit(); // The menu consumed A's release before closing.
    inserted(&mut c);
    assert!(matches!(c.handle(Input::Press(Btn::A),&snap()).first(),Some(Action::SubmitText{..})));
}

#[test]
fn same_batch_cross_cannot_submit_newly_inserted_text() {
    let mut c=native(); record(&mut c,"new draft",&snap());
    c.insertion_result(snap().focused.unwrap(),Ok(()));
    assert!(c.handle(Input::Press(Btn::A),&snap()).iter().all(|a|!matches!(a,Action::SubmitText{..})));
    c.begin_tick();
    assert!(c.handle(Input::Press(Btn::A),&snap()).is_empty());
    c.handle(Input::Release(Btn::A),&snap());
    assert!(matches!(c.handle(Input::Press(Btn::A),&snap()).first(),Some(Action::SubmitText{..})));
}
#[test]
fn submission_refusal_before_dispatch_rearms_only_matching_pending_target() {
    let mut c=native(); inserted(&mut c); let target=snap().focused.unwrap();
    assert!(c.submission_result(target.clone(),Some("early".into())).is_empty());
    c.handle(Input::Press(Btn::A),&snap());
    let mut other=target.clone();other.pane_id="other".into();
    assert!(c.submission_result(other,Some("stale".into())).is_empty());
    let out=c.submission_result(target.clone(),Some("focus changed".into()));
    assert!(matches!(out.first(),Some(Action::Toast {body,..}) if body=="focus changed"));
    assert!(c.submission_result(target.clone(),Some("duplicate".into())).is_empty());
    c.handle(Input::Release(Btn::A),&snap());
    assert!(matches!(c.handle(Input::Press(Btn::A),&snap()).first(),Some(Action::SubmitText{..})));
    c.submission_result(target.clone(),None);
    assert!(c.submission_result(target,Some("late failure".into())).is_empty());
    c.handle(Input::Release(Btn::A),&snap());
    assert!(c.handle(Input::Press(Btn::A),&snap()).iter().all(|a|!matches!(a,Action::SubmitText{..})));
}
#[test]
fn navigation_disconnect_and_menu_disarm_native_submit() {
    for button in [Btn::Up,Btn::Down,Btn::Left,Btn::Right,Btn::Lb,Btn::Rb,Btn::Lt,Btn::Rt,Btn::Y] {
        let mut c=native();inserted(&mut c); c.handle(Input::Press(button),&snap());
        assert!(c.handle(Input::Press(Btn::A),&snap()).iter().all(|a|!matches!(a,Action::SubmitText{..})),"{button:?}");
    }
    for disconnect in [false,true] {
        let mut c=native(); inserted(&mut c);
        if disconnect {c.handle(Input::Disconnected,&snap());} else {c.menu_exit();}
        assert!(c.handle(Input::Press(Btn::A),&snap()).iter().all(|a|!matches!(a,Action::SubmitText{..})));
    }
}
#[test]
fn shell_recording_explains_that_no_agent_was_selected() {
    let mut c=native();let mut s=snap();s.focused.as_mut().unwrap().agent=None;
    assert!(matches!(record(&mut c,"hello",&s).first(),Some(Action::Toast {body,..}) if body.contains("No agent was selected")));
}
#[test]
fn leaving_preview_invalidates_late_submission_refusals() {
    for kind in 0..4 {
        let mut c=native(); inserted(&mut c); let target=snap().focused.unwrap();
        c.handle(Input::Press(Btn::A),&snap());
        match kind {0=>c.menu_exit(),1=>{c.handle(Input::Disconnected,&snap());},2=>{c.handle(Input::Press(Btn::Lb),&snap());},_=>c.set_preview_enabled(false)}
        assert!(c.submission_result(target,Some("late".into())).is_empty());
    }
}

#[test]
fn square_clears_staged_input_once_without_submitting_or_zooming() {
    for agent in ["claude","codex"] {
        let mut s=snap();s.focused.as_mut().unwrap().agent=Some(agent.into());
        let mut c=native();record(&mut c,"delete this",&s);c.insertion_result(s.focused.clone().unwrap(),Ok(()));c.begin_tick();
        assert_eq!(c.handle(Input::Press(Btn::X),&s),vec![Action::ClearInput{target:s.focused.clone().unwrap()}]);
        assert_eq!(c.mode(),Mode::Navigate);
        assert!(c.handle(Input::Press(Btn::X),&s).is_empty());
        assert!(c.handle(Input::Press(Btn::A),&s).iter().all(|a|!matches!(a,Action::SubmitText{..}|Action::SendKeys{..})));
        c.handle(Input::Press(Btn::B),&s);c.handle(Input::Release(Btn::X),&s);
        assert_eq!(c.handle(Input::Press(Btn::X),&s),vec![Action::ToggleZoom(s.focused.unwrap().pane_id)]);
    }
}
#[test]
fn square_clears_in_respond_but_preserves_other_modes() {
    let s=snap();let mut c=Core::new(Btn::RStick);
    assert_eq!(c.handle(Input::Press(Btn::X),&s),vec![Action::ToggleZoom(s.focused.clone().unwrap().pane_id)]);
    c.handle(Input::Press(Btn::A),&s);
    assert_eq!(c.mode(),Mode::Respond);
    assert_eq!(c.handle(Input::Press(Btn::X),&s),vec![Action::ClearInput{target:s.focused.clone().unwrap()}]);
    assert_eq!(c.mode(),Mode::Navigate);
    let mut shell=s;shell.focused.as_mut().unwrap().agent=None;
    let mut c=Core::new(Btn::RStick);c.handle(Input::Press(Btn::A),&shell);
    assert_eq!(c.handle(Input::Press(Btn::X),&shell),vec![Action::SendPaneKeys{target:shell.focused.unwrap().pane_id,keys:vec![Key::Space]}]);
}
#[test]
fn square_rejects_unreviewed_or_busy_clear_and_requires_release() {
    for kind in 0..3 {
        let mut c=native();record(&mut c,"draft",&snap());c.insertion_result(snap().focused.unwrap(),Ok(()));
        if kind>0 {c.begin_tick();c.handle(Input::Press(Btn::RStick),&snap());}
        if kind==2 {c.handle(Input::Release(Btn::RStick),&snap());}
        let out=c.handle(Input::Press(Btn::X),&snap());
        assert!(out.iter().all(|a|!matches!(a,Action::ClearInput{..}|Action::ToggleZoom(_))));
        c.begin_tick();c.handle(Input::TranscriptFailed("done".into()),&snap());
        assert!(c.handle(Input::Press(Btn::X),&snap()).is_empty());
        c.handle(Input::Release(Btn::X),&snap());
        assert!(matches!(c.handle(Input::Press(Btn::X),&snap()).first(),Some(Action::ClearInput{..})));
    }
    let mut c=Core::new(Btn::RStick);c.handle(Input::Press(Btn::A),&snap());c.handle(Input::Press(Btn::RStick),&snap());
    assert!(c.handle(Input::Press(Btn::X),&snap()).iter().all(|a|!matches!(a,Action::ClearInput{..})));
}
#[test]
fn square_clear_rejects_target_drift_and_unsupported_editors() {
    for variant in 0..9 {
        let mut c=native();inserted(&mut c);let mut changed=snap();let p=changed.focused.as_mut().unwrap();
        match variant {0=>p.pane_id="other".into(),1=>p.workspace_id="other".into(),2=>p.tab_id="other".into(),3=>p.agent=Some("codex".into()),4=>p.agent=None,5=>p.status=Some(Status::Blocked),6=>p.status=Some(Status::Unknown),7=>p.status=None,_=>changed.focused=None}
        assert!(c.handle(Input::Press(Btn::X),&changed).iter().all(|a|!matches!(a,Action::ClearInput{..}|Action::ToggleZoom(_))));
    }
    for mode in 0..2 {
        let mut s=snap();s.focused.as_mut().unwrap().agent=Some("other".into());
        let mut c=native();
        if mode==0 {record(&mut c,"draft",&s);c.insertion_result(s.focused.clone().unwrap(),Ok(()));c.begin_tick();}
        else {c.handle(Input::Press(Btn::A),&s);}
        let actions=c.handle(Input::Press(Btn::X),&s);
        assert!(matches!(actions.first(),Some(Action::Toast{body,..}) if body.contains("Codex and Claude")));
    }
}
#[test]
fn respond_clear_uses_the_original_agent_identity() {
    for variant in 0..5 {
        let mut c=Core::new(Btn::RStick);c.handle(Input::Press(Btn::A),&snap());
        let mut s=snap();let p=s.focused.as_mut().unwrap();
        match variant {0=>p.pane_id="other".into(),1=>p.agent=Some("codex".into()),2=>p.agent=None,3=>p.status=Some(Status::Blocked),_=>s.focused=None}
        assert!(c.handle(Input::Press(Btn::X),&s).iter().all(|a|!matches!(a,Action::ClearInput{..}|Action::ToggleZoom(_))));
    }
}
#[test]
fn square_latch_resets_when_release_ownership_changes() {
    for reset in 0..3 {
        let mut c=native();inserted(&mut c);c.handle(Input::Press(Btn::X),&snap());
        match reset {0=>c.menu_exit(),1=>{c.handle(Input::Disconnected,&snap());},_=>c.snapshot_failed()}
        inserted(&mut c);
        assert!(matches!(c.handle(Input::Press(Btn::X),&snap()).first(),Some(Action::ClearInput{..})));
    }
}

#[test]
fn native_and_respond_hints_explain_square_clear() {
    let mut c=native();record(&mut c,"draft",&snap());
    let inserted=c.insertion_result(snap().focused.unwrap(),Ok(()));
    assert!(matches!(inserted.first(),Some(Action::Toast{body,..}) if body.contains("Square clears")));
    c.menu_exit();
    let entered=c.handle(Input::Press(Btn::A),&snap());
    assert!(matches!(entered.first(),Some(Action::Toast{body,..}) if body.contains("Square clears")&&body.contains("Codex/Claude")));
}
