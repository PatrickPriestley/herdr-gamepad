use crate::{engine::*, dictation::DeliveryState};
use crate::engine_tests::snap;
fn record(c: &mut Core, text: &str, s: &Snapshot) -> Vec<Action> {
    assert_eq!(c.handle(Input::Press(Btn::RStick), s), vec![Action::StartRecording]);
    assert_eq!(c.handle(Input::Release(Btn::RStick), s), vec![Action::StopRecording]);
    c.handle(Input::Transcript(text.into()), s)
}
fn draft() -> Core {
    let mut c=Core::new(Btn::RStick); c.set_preview_enabled(true);
    assert!(record(&mut c, "original", &snap()).is_empty());
    assert_eq!(c.draft().unwrap().text,"original"); c
}
#[test]
fn preview_retains_text_while_default_still_sends() {
    let mut c=Core::new(Btn::RStick);
    assert!(matches!(record(&mut c,"auto",&snap())[0],Action::Prompt{..}));
    c.set_preview_enabled(true);
    assert!(record(&mut c,"  review me  ",&snap()).is_empty());
    assert_eq!(c.draft().unwrap().text,"review me");
    assert!(c.take_preview_request()); assert!(!c.take_preview_request());
}
#[test]
fn send_once_clears_only_after_matching_acknowledgement() {
    let mut c=draft(); let (id, action)=c.draft_send(&snap()).unwrap();
    assert!(matches!(action,Action::Prompt{ref text,..} if text=="original"));
    assert_eq!(c.draft().unwrap().state,DeliveryState::Sending);
    assert!(c.draft_send(&snap()).is_none()); assert!(!c.discard_draft());
    assert!(!matches!(c.handle(Input::Press(Btn::RStick),&snap()).first(),Some(Action::StartRecording)));
    c.draft_delivery(id+1,Ok(())); assert!(c.draft().is_some());
    c.draft_delivery(id,Ok(())); assert!(c.draft().is_none());
}
#[test]
fn unknown_delivery_keeps_text_without_retry_and_discard_is_explicit() {
    let mut c=draft(); let (id,_)=c.draft_send(&snap()).unwrap();
    c.draft_delivery(id,Err("connection lost".into()));
    assert!(matches!(c.draft().unwrap().state,DeliveryState::Unknown(_)));
    assert_eq!(c.draft().unwrap().text,"original"); assert!(c.draft_send(&snap()).is_none());
    c.draft_delivery(id,Ok(())); assert!(c.draft().is_some());
    assert!(c.discard_draft()); assert!(c.draft().is_none());
}
#[test]
fn drifted_or_unavailable_target_preserves_draft() {
    for variant in 0..7 {
        let mut c=draft(); let mut s=snap(); let p=s.focused.as_mut().unwrap();
        match variant { 0=>p.pane_id="other".into(),1=>p.workspace_id="other".into(),2=>p.tab_id="other".into(),3=>p.agent=None,4=>p.agent=Some("other".into()),5=>p.status=Some(Status::Blocked),_=>s.focused=None }
        assert!(c.draft_send(&s).is_none()); assert!(!c.draft().unwrap().error.is_empty());
        assert!(c.draft_send(&snap()).is_some());
    }
}
#[test]
fn replacement_failure_empty_and_disconnect_keep_draft_and_target() {
    let mut c=draft(); let target=c.draft().unwrap().target.clone();
    c.set_preview_enabled(false);
    assert!(record(&mut c," ",&snap()).iter().all(|a|!matches!(a,Action::Prompt{..})));
    assert_eq!(c.draft().unwrap().text,"original");
    c.handle(Input::Press(Btn::RStick),&snap()); c.handle(Input::Release(Btn::RStick),&snap());
    c.handle(Input::TranscriptFailed("failed".into()),&snap());
    assert_eq!(c.draft().unwrap().text,"original");
    c.handle(Input::Disconnected,&snap()); assert!(c.draft().is_some());
    let mut other=snap(); other.focused.as_mut().unwrap().pane_id="other".into();
    assert!(record(&mut c,"replacement",&other).is_empty());
    assert_eq!(c.draft().unwrap().text,"replacement"); assert_eq!(c.draft().unwrap().target,target);
    assert!(c.draft_send(&other).is_none()); assert!(c.draft_send(&snap()).is_some());
}
#[test]
fn recording_snapshots_preview_choice_and_busy_draft_cannot_send() {
    let mut c=Core::new(Btn::RStick); c.set_preview_enabled(true);
    c.handle(Input::Press(Btn::RStick),&snap()); c.set_preview_enabled(false);
    c.handle(Input::Release(Btn::RStick),&snap()); c.handle(Input::Transcript("one".into()),&snap());
    assert_eq!(c.draft().unwrap().text,"one");
    c.handle(Input::Press(Btn::RStick),&snap()); assert!(c.draft_send(&snap()).is_none());
    c.handle(Input::Release(Btn::RStick),&snap()); assert!(c.draft_send(&snap()).is_none());
    c.handle(Input::Transcript("two".into()),&snap()); assert!(c.draft_send(&snap()).is_some());
}

#[test]
fn recorder_failure_preserves_draft_and_allows_new_recording() {
    let mut c=draft(); c.handle(Input::Press(Btn::RStick),&snap());
    c.handle(Input::TranscriptFailed("microphone unavailable".into()),&snap());
    assert!(!c.voice_busy()); assert_eq!(c.draft().unwrap().text,"original");
    assert_eq!(c.handle(Input::Press(Btn::RStick),&snap()),vec![Action::StartRecording]);
}
#[test]
fn no_target_draft_does_not_retarget_when_rerecorded() {
    let mut c=Core::new(Btn::RStick); c.set_preview_enabled(true);
    record(&mut c,"first",&Snapshot::default());
    assert!(c.draft().is_some()); assert!(c.draft().unwrap().target.is_none());
    record(&mut c,"second",&snap());
    assert!(c.draft().unwrap().target.is_none()); assert!(c.draft_send(&snap()).is_none());
    assert!(c.discard_draft()); record(&mut c,"third",&snap()); assert!(c.draft_send(&snap()).is_some());
}
#[test]
fn rejection_before_dispatch_allows_retry_only_for_current_sending_draft() {
    let mut c=draft(); let id=c.draft().unwrap().id;
    c.draft_rejected(id,"early".into()); assert!(c.draft().unwrap().error.is_empty());
    c.draft_send(&snap()).unwrap(); c.draft_rejected(id+1,"stale".into());
    assert_eq!(c.draft().unwrap().state,DeliveryState::Sending);
    c.draft_rejected(id,"focus changed".into());
    assert_eq!(c.draft().unwrap().state,DeliveryState::Ready);
    assert_eq!(c.draft().unwrap().error,"focus changed"); assert!(c.draft_send(&snap()).is_some());
}

#[test]
fn discard_waits_for_replacement_recording_and_transcription() {
    let mut c = draft();
    assert_eq!(c.handle(Input::Press(Btn::RStick), &snap()), vec![Action::StartRecording]);
    assert!(!c.discard_draft(), "recording replacement must retain the previous draft");
    assert_eq!(c.draft().unwrap().text, "original");
    c.handle(Input::Release(Btn::RStick), &snap());
    assert!(!c.discard_draft(), "transcription must retain the previous draft");
    c.handle(Input::TranscriptFailed("failed".into()), &snap());
    assert!(c.discard_draft());
}

#[test]
fn a_matching_non_agent_pane_cannot_receive_a_voice_draft() {
    let mut c = Core::new(Btn::RStick);
    c.set_preview_enabled(true);
    let mut s = snap();
    s.focused.as_mut().unwrap().agent = None;
    assert!(record(&mut c, "not for a shell", &s).is_empty());
    assert_eq!(c.draft().unwrap().text, "not for a shell");
    assert!(c.draft_send(&s).is_none());
    assert!(!c.draft().unwrap().error.is_empty());
    assert!(c.discard_draft());
    assert!(record(&mut c, "for an agent", &snap()).is_empty());
    assert!(c.draft_send(&snap()).is_some());
}

#[test]
fn stale_draft_events_without_a_draft_are_ignored() {
    let mut c = Core::new(Btn::RStick);
    assert!(c.draft_send(&snap()).is_none());
    c.draft_delivery(1, Ok(()));
    c.draft_rejected(1, "stale".into());
    assert!(!c.discard_draft());
    c.set_preview_enabled(true);
    record(&mut c, "valid draft", &snap());
    let (id, _) = c.draft_send(&snap()).unwrap();
    c.draft_delivery(id, Ok(()));
    assert!(c.draft().is_none());
    c.draft_delivery(id, Ok(()));
    c.draft_rejected(id, "already completed".into());
    assert!(c.draft().is_none());
}

#[test]
fn replacement_failures_reopen_preview_and_clear_inflight_state() {
    let mut c = draft();
    assert!(c.take_preview_request());
    assert!(!c.preview_in_progress());
    record(&mut c, "  ", &snap());
    assert!(!c.preview_in_progress());
    assert!(c.take_preview_request());
    assert!(c.draft().unwrap().error.contains("Heard nothing"));
    c.handle(Input::Press(Btn::RStick), &snap());
    c.handle(Input::Release(Btn::RStick), &snap());
    c.handle(Input::TranscriptFailed("bad audio".into()), &snap());
    assert!(!c.preview_in_progress());
    assert!(c.take_preview_request());
    assert!(c.draft().unwrap().error.contains("bad audio"));
    assert_eq!(c.draft().unwrap().text, "original");
    assert!(record(&mut c, "replacement", &snap()).is_empty());
    assert!(!c.preview_in_progress());
    assert!(c.draft().unwrap().error.is_empty());
    assert_eq!(c.draft().unwrap().text, "replacement");
}

#[test]
fn empty_first_preview_does_not_claim_a_previous_draft_exists() {
    let mut c=Core::new(Btn::RStick); c.set_preview_enabled(true);
    assert_eq!(record(&mut c," ",&snap()),vec![toast("Heard nothing","",Sound::None)]);
    record(&mut c,"keep me",&snap());
    assert_eq!(record(&mut c," ",&snap()),vec![toast("Heard nothing","Previous draft kept",Sound::None)]);
}

#[test]
fn preflight_errors_only_update_the_matching_ready_draft() {
    let mut c=draft(); let id=c.draft().unwrap().id;
    c.draft_preflight_failed(id+1,"stale".into());
    assert!(c.draft().unwrap().error.is_empty());
    c.draft_preflight_failed(id,"offline".into());
    assert_eq!(c.draft().unwrap().error,"offline");
    c.draft_send(&snap()).unwrap();
    c.draft_preflight_failed(id,"stale".into());
    assert!(c.draft().unwrap().error.is_empty());
    c.draft_delivery(id,Err("unknown".into()));
    c.draft_preflight_failed(id,"stale".into());
    assert!(c.draft().unwrap().error.is_empty());
    c.discard_draft();
    c.draft_preflight_failed(id,"stale".into());
    assert!(c.draft().is_none());
}
