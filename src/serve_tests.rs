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
