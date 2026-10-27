use crate::core::*;

pub fn pane(id: &str, agent: Option<&str>, status: Option<Status>) -> Pane {
    let (ws, _) = id.split_once(':').unwrap();
    Pane { pane_id: id.into(), workspace_id: ws.into(), tab_id: format!("{ws}:t1"), agent: agent.map(String::from), status }
}

/// Three spaces (w5 #1 focused, w7 #2, w0 #3), two tabs in w5, focused pane w5:p1 with an idle claude.
pub fn snap() -> Snapshot {
    Snapshot {
        focused: Some(pane("w5:p1", Some("claude"), Some(Status::Idle))),
        workspaces: vec![
            Workspace { id: "w5".into(), number: 1, focused: true },
            Workspace { id: "w7".into(), number: 2, focused: false },
            Workspace { id: "w0".into(), number: 3, focused: false },
        ],
        tabs: vec![
            Tab { id: "w5:t1".into(), number: 1, focused: true },
            Tab { id: "w5:t7".into(), number: 7, focused: false },
        ],
        agents: vec![
            Agent { pane_id: "w5:p1".into(), workspace_id: "w5".into(), status: Status::Idle },
            Agent { pane_id: "w7:p1".into(), workspace_id: "w7".into(), status: Status::Done },
            Agent { pane_id: "w0:p3".into(), workspace_id: "w0".into(), status: Status::Blocked },
        ],
    }
}

fn core() -> Core { Core::new(Btn::RStick) }

#[test]
fn starts_in_navigate_mode() {
    assert_eq!(core().mode(), Mode::Navigate);
}

#[test]
fn dpad_focuses_the_neighbouring_pane() {
    let mut c = core();
    assert_eq!(c.handle(Input::Press(Btn::Left), &snap()), vec![Action::FocusDirection { pane_id: "w5:p1".into(), dir: Dir::Left }]);
    assert_eq!(c.handle(Input::Press(Btn::Down), &snap()), vec![Action::FocusDirection { pane_id: "w5:p1".into(), dir: Dir::Down }]);
    assert_eq!(c.mode(), Mode::Navigate);
}

#[test]
fn dpad_with_no_focused_pane_does_nothing() {
    let s = Snapshot { focused: None, ..snap() };
    assert_eq!(core().handle(Input::Press(Btn::Up), &s), vec![]);
}

#[test]
fn releases_of_ordinary_buttons_do_nothing() {
    assert_eq!(core().handle(Input::Release(Btn::Up), &snap()), vec![]);
}

#[test]
fn bumpers_step_spaces_by_number_and_wrap() {
    let mut c = core();
    assert_eq!(c.handle(Input::Press(Btn::Rb), &snap()), vec![Action::FocusWorkspace("w7".into())]);
    assert_eq!(c.handle(Input::Press(Btn::Lb), &snap()), vec![Action::FocusWorkspace("w0".into())], "previous from #1 wraps to #3");
    let mut s = snap();
    for w in &mut s.workspaces { w.focused = w.id == "w0"; }
    assert_eq!(c.handle(Input::Press(Btn::Rb), &s), vec![Action::FocusWorkspace("w5".into())], "next from #3 wraps to #1");
}

#[test]
fn bumpers_use_sidebar_numbers_not_list_order() {
    let mut s = snap();
    s.workspaces.reverse();
    assert_eq!(core().handle(Input::Press(Btn::Rb), &s), vec![Action::FocusWorkspace("w7".into())]);
}

#[test]
fn bumpers_do_nothing_with_one_space_or_no_focused_space() {
    let mut s = snap();
    s.workspaces.truncate(1);
    assert_eq!(core().handle(Input::Press(Btn::Rb), &s), vec![]);
    let mut s = snap();
    for w in &mut s.workspaces { w.focused = false; }
    assert_eq!(core().handle(Input::Press(Btn::Rb), &s), vec![]);
}

#[test]
fn triggers_step_tabs_and_wrap() {
    let mut c = core();
    assert_eq!(c.handle(Input::Press(Btn::Rt), &snap()), vec![Action::FocusTab("w5:t7".into())]);
    assert_eq!(c.handle(Input::Press(Btn::Lt), &snap()), vec![Action::FocusTab("w5:t7".into())], "previous from the first wraps to the last");
    let mut s = snap();
    s.tabs.truncate(1);
    assert_eq!(c.handle(Input::Press(Btn::Rt), &s), vec![], "one tab: nothing to step to");
}

#[test]
fn y_focuses_the_next_blocked_agent_after_the_focused_pane() {
    assert_eq!(core().handle(Input::Press(Btn::Y), &snap()), vec![Action::FocusAgent("w0:p3".into())]);
}

#[test]
fn y_prefers_blocked_over_done_even_when_done_comes_first() {
    // w7:p1 (done) sits before w0:p3 (blocked) in sidebar order; blocked wins.
    assert_eq!(core().handle(Input::Press(Btn::Y), &snap()), vec![Action::FocusAgent("w0:p3".into())]);
    let mut s = snap();
    s.agents.retain(|a| a.status != Status::Blocked);
    assert_eq!(core().handle(Input::Press(Btn::Y), &s), vec![Action::FocusAgent("w7:p1".into())], "no blocked: the first done");
}

#[test]
fn y_cycles_past_the_focused_pane_and_wraps() {
    let mut s = snap();
    s.focused = Some(pane("w0:p3", Some("claude"), Some(Status::Blocked)));
    s.agents.push(Agent { pane_id: "w7:p9".into(), workspace_id: "w7".into(), status: Status::Blocked });
    // Order: w5:p1, w7:p1, w7:p9, w0:p3. After w0:p3 the ring wraps: w7:p9 comes before w0:p3 itself.
    assert_eq!(core().handle(Input::Press(Btn::Y), &s), vec![Action::FocusAgent("w7:p9".into())]);
}

#[test]
fn y_toasts_when_no_agent_needs_you() {
    let mut s = snap();
    for a in &mut s.agents { a.status = Status::Working; }
    assert_eq!(core().handle(Input::Press(Btn::Y), &s), vec![toast("No agent needs you", "", Sound::None)]);
    s.agents.clear();
    assert_eq!(core().handle(Input::Press(Btn::Y), &s), vec![toast("No agent needs you", "", Sound::None)]);
}

#[test]
fn x_toggles_zoom_on_the_focused_pane() {
    assert_eq!(core().handle(Input::Press(Btn::X), &snap()), vec![Action::ToggleZoom("w5:p1".into())]);
    let s = Snapshot { focused: None, ..snap() };
    assert_eq!(core().handle(Input::Press(Btn::X), &s), vec![]);
}

#[test]
fn a_enters_respond_mode_without_sending_enter() {
    let mut c = core();
    let out = c.handle(Input::Press(Btn::A), &snap());
    assert_eq!(c.mode(), Mode::Respond);
    assert!(matches!(out.as_slice(), [Action::Toast { title, .. }] if title == "Respond mode"), "{out:?}");
}

#[test]
fn a_in_navigate_toasts_when_the_pane_has_no_agent() {
    let mut c = core();
    let s = Snapshot { focused: Some(pane("w5:p2", None, None)), ..snap() };
    let out = c.handle(Input::Press(Btn::A), &s);
    assert_eq!(c.mode(), Mode::Navigate);
    assert!(matches!(out.as_slice(), [Action::Toast { title, .. }] if title == "No agent here"), "{out:?}");
}

fn in_respond() -> Core {
    let mut c = core();
    c.handle(Input::Press(Btn::A), &snap());
    assert_eq!(c.mode(), Mode::Respond);
    c
}

fn keys(target: &str, key: Key) -> Vec<Action> {
    vec![Action::SendKeys { target: target.into(), keys: vec![key] }]
}

#[test]
fn respond_mode_sends_keys_to_the_focused_agent() {
    let mut c = in_respond();
    assert_eq!(c.handle(Input::Press(Btn::Up), &snap()), keys("w5:p1", Key::Up));
    assert_eq!(c.handle(Input::Press(Btn::Down), &snap()), keys("w5:p1", Key::Down));
    assert_eq!(c.handle(Input::Press(Btn::Left), &snap()), keys("w5:p1", Key::Left));
    assert_eq!(c.handle(Input::Press(Btn::Right), &snap()), keys("w5:p1", Key::Right));
    assert_eq!(c.handle(Input::Press(Btn::A), &snap()), keys("w5:p1", Key::Enter));
    assert_eq!(c.handle(Input::Press(Btn::B), &snap()), keys("w5:p1", Key::Esc));
    assert_eq!(c.mode(), Mode::Respond);
}

#[test]
fn view_leaves_respond_mode() {
    let mut c = in_respond();
    let out = c.handle(Input::Press(Btn::View), &snap());
    assert_eq!(c.mode(), Mode::Navigate);
    assert!(matches!(out.as_slice(), [Action::Toast { title, .. }] if title == "Navigate mode"), "{out:?}");
    assert_eq!(c.handle(Input::Press(Btn::View), &snap()), vec![], "View in Navigate does nothing");
}

#[test]
fn navigation_buttons_in_respond_mode_act_and_return_to_navigate() {
    for (b, expect) in [
        (Btn::Rb, Action::FocusWorkspace("w7".into())),
        (Btn::Rt, Action::FocusTab("w5:t7".into())),
        (Btn::Y, Action::FocusAgent("w0:p3".into())),
        (Btn::X, Action::ToggleZoom("w5:p1".into())),
    ] {
        let mut c = in_respond();
        let out = c.handle(Input::Press(b), &snap());
        assert_eq!(c.mode(), Mode::Navigate, "{b:?}");
        assert_eq!(out, vec![toast("Navigate mode", "", Sound::None), expect], "{b:?}");
    }
}

#[test]
fn respond_mode_with_no_agent_in_the_focused_pane_falls_back_to_navigate() {
    let mut c = in_respond();
    let s = Snapshot { focused: Some(pane("w5:p2", None, None)), ..snap() };
    let out = c.handle(Input::Press(Btn::A), &s);
    assert_eq!(c.mode(), Mode::Navigate);
    assert!(matches!(out.as_slice(), [Action::Toast { title, .. }] if title == "Navigate mode"), "{out:?}");
    assert!(!out.iter().any(|a| matches!(a, Action::SendKeys { .. })));
}

#[test]
fn menu_and_stick_clicks_do_nothing_in_either_mode() {
    let mut c = core();
    assert_eq!(c.handle(Input::Press(Btn::Menu), &snap()), vec![]);
    assert_eq!(c.handle(Input::Press(Btn::LStick), &snap()), vec![]);
    let mut c = in_respond();
    assert_eq!(c.handle(Input::Press(Btn::Menu), &snap()), vec![]);
    assert_eq!(c.handle(Input::Press(Btn::LStick), &snap()), vec![]);
}

fn blocked() -> Snapshot {
    Snapshot { focused: Some(pane("w5:p1", Some("claude"), Some(Status::Blocked))), ..snap() }
}

#[test]
fn dpad_on_a_blocked_agent_switches_to_respond_and_sends_the_key() {
    let mut c = core();
    let out = c.handle(Input::Press(Btn::Down), &blocked());
    assert_eq!(c.mode(), Mode::Respond);
    assert_eq!(out, vec![
        toast("Respond mode", "the agent is waiting for you", Sound::None),
        Action::SendKeys { target: "w5:p1".into(), keys: vec![Key::Down] },
    ]);
}

#[test]
fn a_on_a_blocked_agent_only_enters_respond_mode() {
    // An approval is always two deliberate presses: A to enter Respond, A again to confirm.
    let mut c = core();
    let out = c.handle(Input::Press(Btn::A), &blocked());
    assert_eq!(c.mode(), Mode::Respond);
    assert!(!out.iter().any(|a| matches!(a, Action::SendKeys { .. })), "{out:?}");
}

#[test]
fn dpad_on_a_working_agent_still_moves_focus() {
    let mut c = core();
    let s = Snapshot { focused: Some(pane("w5:p1", Some("claude"), Some(Status::Working))), ..snap() };
    assert_eq!(c.handle(Input::Press(Btn::Down), &s), vec![Action::FocusDirection { pane_id: "w5:p1".into(), dir: Dir::Down }]);
    assert_eq!(c.mode(), Mode::Navigate);
}

#[test]
fn holding_the_voice_button_records_and_releasing_stops() {
    let mut c = core();
    assert_eq!(c.handle(Input::Press(Btn::RStick), &snap()), vec![Action::StartRecording]);
    assert!(c.recording());
    assert_eq!(c.handle(Input::Press(Btn::RStick), &snap()), vec![], "a repeated press while recording is ignored");
    assert_eq!(c.handle(Input::Release(Btn::RStick), &snap()), vec![Action::StopRecording]);
    assert!(!c.recording());
    assert_eq!(c.handle(Input::Release(Btn::RStick), &snap()), vec![], "a release when not recording is ignored");
}

#[test]
fn the_voice_button_is_configurable() {
    let mut c = Core::new(Btn::Menu);
    assert_eq!(c.handle(Input::Press(Btn::Menu), &snap()), vec![Action::StartRecording]);
    assert_eq!(c.handle(Input::Press(Btn::RStick), &snap()), vec![], "the stick is an ordinary button now");
}

#[test]
fn recording_works_in_respond_mode_too() {
    let mut c = in_respond();
    assert_eq!(c.handle(Input::Press(Btn::RStick), &snap()), vec![Action::StartRecording]);
    assert_eq!(c.mode(), Mode::Respond);
}

#[test]
fn transcript_to_an_idle_or_done_agent_is_submitted() {
    let mut c = core();
    assert_eq!(c.handle(Input::Transcript("run the tests".into()), &snap()), vec![
        Action::Prompt { target: "w5:p1".into(), text: "run the tests".into() },
        toast("Sent", "run the tests", Sound::None),
    ]);
    let s = Snapshot { focused: Some(pane("w5:p1", Some("claude"), Some(Status::Done))), ..snap() };
    assert!(matches!(c.handle(Input::Transcript("ok".into()), &s).as_slice(), [Action::Prompt { .. }, _]));
}

#[test]
fn transcript_to_a_working_agent_is_typed_but_not_sent() {
    let s = Snapshot { focused: Some(pane("w5:p1", Some("claude"), Some(Status::Working))), ..snap() };
    assert_eq!(core().handle(Input::Transcript("  also check lint  ".into()), &s), vec![
        Action::SendText { pane_id: "w5:p1".into(), text: "also check lint".into() },
        toast("Typed, not sent", "agent is working: also check lint", Sound::None),
    ]);
}

#[test]
fn transcript_is_dropped_when_the_agent_waits_at_a_dialog_or_is_absent() {
    let out = core().handle(Input::Transcript("yes".into()), &blocked());
    assert_eq!(out, vec![toast("Not sent", "agent is waiting at a dialog: yes", Sound::Request)]);
    let s = Snapshot { focused: Some(pane("w5:p2", None, None)), ..snap() };
    let out = core().handle(Input::Transcript("yes".into()), &s);
    assert_eq!(out, vec![toast("Not sent", "no agent in the focused pane: yes", Sound::Request)]);
    let s = Snapshot { focused: None, ..snap() };
    assert_eq!(core().handle(Input::Transcript("yes".into()), &s), vec![toast("Not sent", "no agent in the focused pane: yes", Sound::Request)]);
}

#[test]
fn blank_transcript_is_dropped() {
    assert_eq!(core().handle(Input::Transcript("   ".into()), &snap()), vec![toast("Heard nothing", "", Sound::None)]);
}

#[test]
fn long_transcripts_are_previewed_in_the_toast() {
    let text = "a".repeat(80);
    let out = core().handle(Input::Transcript(text.clone()), &snap());
    assert_eq!(out[0], Action::Prompt { target: "w5:p1".into(), text: text.clone() }, "the full text is sent");
    assert_eq!(out[1], toast("Sent", &format!("{}…", "a".repeat(60)), Sound::None));
}

#[test]
fn controller_connection_changes_are_toasted() {
    assert_eq!(core().handle(Input::Connected, &snap()), vec![toast("Controller connected", "", Sound::None)]);
    assert_eq!(core().handle(Input::Disconnected, &snap()), vec![toast("Controller disconnected", "", Sound::None)]);
}
