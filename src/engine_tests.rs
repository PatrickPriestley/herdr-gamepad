use crate::engine::*;

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
    let s = Snapshot { focused: Some(pane("w5:p2", None, None)), ..snap() };
    assert_eq!(core().handle(Input::Press(Btn::Y), &s), vec![Action::FocusAgent("w0:p3".into())], "a shell pane is focused");
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


fn in_respond() -> Core {
    let mut c = core();
    c.handle(Input::Press(Btn::A), &snap());
    assert_eq!(c.mode(), Mode::Respond);
    c
}

fn keys(target: &str, key: Key) -> Vec<Action> {
    vec![Action::SendKeys { target: target.into(), keys: vec![key] }]
}

/// Holds and releases the voice button with `s` focused, so the next transcript has a target.
fn spoke(c: &mut Core, s: &Snapshot) {
    assert_eq!(c.handle(Input::Press(Btn::RStick), s), vec![Action::StartRecording]);
    assert_eq!(c.handle(Input::Release(Btn::RStick), s), vec![Action::StopRecording]);
}

/// What a fresh core does with `text` spoken and transcribed with `s` focused throughout.
fn said(text: &str, s: &Snapshot) -> Vec<Action> {
    let mut c = core();
    spoke(&mut c, s);
    c.handle(Input::Transcript(text.into()), s)
}

#[test]
fn respond_mode_sends_keys_to_the_focused_agent() {
    let mut c = in_respond();
    assert_eq!(c.handle(Input::Press(Btn::Up), &snap()), keys("w5:p1", Key::Up));
    assert_eq!(c.handle(Input::Press(Btn::Down), &snap()), keys("w5:p1", Key::Down));
    assert_eq!(c.handle(Input::Press(Btn::Left), &snap()), keys("w5:p1", Key::Left));
    assert_eq!(c.handle(Input::Press(Btn::Right), &snap()), keys("w5:p1", Key::Right));
    assert_eq!(c.handle(Input::Press(Btn::A), &snap()), keys("w5:p1", Key::Enter));
    assert_eq!(c.handle(Input::Press(Btn::View), &snap()), keys("w5:p1", Key::Esc));
    assert_eq!(c.mode(), Mode::Respond);
}

#[test]
fn circle_leaves_respond_mode() {
    let mut c = in_respond();
    let out = c.handle(Input::Press(Btn::B), &snap());
    assert_eq!(c.mode(), Mode::Navigate);
    assert!(matches!(out.as_slice(), [Action::Toast { title, .. }] if title == "Navigate mode"), "{out:?}");
    assert_eq!(c.handle(Input::Press(Btn::B), &snap()), vec![], "Circle in Navigate does nothing");
}

#[test]
fn navigation_buttons_in_respond_mode_act_and_return_to_navigate() {
    for (b, expect) in [
        (Btn::Rb, Action::FocusWorkspace("w7".into())),
        (Btn::Rt, Action::FocusTab("w5:t7".into())),
        (Btn::Y, Action::FocusAgent("w0:p3".into())),
    ] {
        let mut c = in_respond();
        let out = c.handle(Input::Press(b), &snap());
        assert_eq!(c.mode(), Mode::Navigate, "{b:?}");
        assert_eq!(out, vec![toast("Navigate mode", "", Sound::None), expect], "{b:?}");
    }
}

#[test]
fn respond_mode_with_no_agent_in_the_focused_pane_falls_back_to_navigate() {
    // The agent left the pane Respond was entered on.
    let mut c = in_respond();
    let s = Snapshot { focused: Some(pane("w5:p1", None, None)), ..snap() };
    let out = c.handle(Input::Press(Btn::A), &s);
    assert_eq!(c.mode(), Mode::Navigate);
    assert_eq!(out, vec![toast("Navigate mode", "no agent in the focused pane", Sound::None)]);
}

#[test]
fn unused_stick_click_does_nothing_in_either_mode() {
    let mut c = core();
    assert_eq!(c.handle(Input::Press(Btn::LStick), &snap()), vec![]);
    let mut c = in_respond();
    assert_eq!(c.handle(Input::Press(Btn::LStick), &snap()), vec![]);
}

fn blocked() -> Snapshot {
    Snapshot { focused: Some(pane("w5:p1", Some("claude"), Some(Status::Blocked))), ..snap() }
}

#[test]
fn dpad_on_a_blocked_agent_stays_in_navigate_until_cross_enters() {
    let mut c = core();
    assert_eq!(c.handle(Input::Press(Btn::Down), &blocked()), vec![Action::FocusDirection { pane_id: "w5:p1".into(), dir: Dir::Down }]);
    assert_eq!(c.mode(), Mode::Navigate);
    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), blocked_entry("w5:p1"));
    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
}

#[test]
fn a_on_a_blocked_agent_enters_respond_and_sends_enter() {
    // One deliberate Cross both enters Respond and confirms the waiting agent.
    let mut c = core();
    let out = c.handle(Input::Press(Btn::A), &blocked());
    assert_eq!(c.mode(), Mode::Respond);
    assert_eq!(out, blocked_entry("w5:p1"));
    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter), "another deliberate Cross sends another Enter");
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
    assert_eq!(said("run the tests", &snap()), vec![
        Action::Prompt { target: "w5:p1".into(), text: "run the tests".into() },
        toast("Sent", "run the tests", Sound::None),
    ]);
    let s = Snapshot { focused: Some(pane("w5:p1", Some("claude"), Some(Status::Done))), ..snap() };
    assert!(matches!(said("ok", &s).as_slice(), [Action::Prompt { .. }, _]));
}

#[test]
fn transcript_to_a_working_agent_is_submitted() {
    let s = Snapshot { focused: Some(pane("w5:p1", Some("claude"), Some(Status::Working))), ..snap() };
    assert_eq!(said("  also check lint  ", &s), vec![
        Action::Prompt { target: "w5:p1".into(), text: "also check lint".into() },
        toast("Sent", "also check lint", Sound::None),
    ]);
}

#[test]
fn transcript_is_dropped_when_the_agent_waits_at_a_dialog_or_is_absent() {
    assert_eq!(said("yes", &blocked()), vec![toast("Not sent", "agent is waiting at a dialog: yes", Sound::Request)]);
    let s = Snapshot { focused: Some(pane("w5:p2", None, None)), ..snap() };
    assert_eq!(said("yes", &s), vec![toast("Not sent", "no agent in the focused pane: yes", Sound::Request)]);
    let s = Snapshot { focused: Some(pane("w5:p1", Some("claude"), Some(Status::Unknown))), ..snap() };
    assert_eq!(said("yes", &s), vec![toast("Not sent", "agent state unknown: yes", Sound::Request)]);
}

#[test]
fn blank_transcript_is_dropped() {
    assert_eq!(said("   ", &snap()), vec![toast("Heard nothing", "", Sound::None)]);
    let s = Snapshot { focused: None, ..snap() };
    assert_eq!(said("   ", &s), vec![toast("Heard nothing", "", Sound::None)], "blank wins over a missing target");
}

#[test]
fn long_transcripts_are_previewed_in_the_toast() {
    let text = "a".repeat(80);
    let out = said(&text, &snap());
    assert_eq!(out[0], Action::Prompt { target: "w5:p1".into(), text: text.clone() }, "the full text is sent");
    assert_eq!(out[1], toast("Sent", &format!("{}…", "a".repeat(60)), Sound::None));
}

#[test]
fn controller_connection_changes_are_toasted() {
    assert_eq!(core().handle(Input::Connected, &snap()), vec![toast("Controller connected", "", Sound::None)]);
    assert_eq!(core().handle(Input::Disconnected, &snap()), vec![toast("Controller disconnected", "", Sound::None)]);
}

// Added to kill the mutants cargo-mutants found on 2026-10-27.

#[test]
fn releasing_another_button_does_not_stop_the_recording() {
    let mut c = core();
    c.handle(Input::Press(Btn::RStick), &snap());
    assert_eq!(c.handle(Input::Release(Btn::Up), &snap()), vec![]);
    assert!(c.recording());
}

#[test]
fn dpad_up_and_right_map_to_their_own_directions() {
    let mut c = core();
    assert_eq!(c.handle(Input::Press(Btn::Up), &snap()), vec![Action::FocusDirection { pane_id: "w5:p1".into(), dir: Dir::Up }]);
    assert_eq!(c.handle(Input::Press(Btn::Right), &snap()), vec![Action::FocusDirection { pane_id: "w5:p1".into(), dir: Dir::Right }]);
}

#[test]
fn left_trigger_goes_backwards_among_three_tabs() {
    let mut s = snap();
    s.tabs = vec![
        Tab { id: "w5:t1".into(), number: 1, focused: false },
        Tab { id: "w5:t2".into(), number: 2, focused: true },
        Tab { id: "w5:t3".into(), number: 3, focused: false },
    ];
    assert_eq!(core().handle(Input::Press(Btn::Lt), &s), vec![Action::FocusTab("w5:t1".into())]);
    assert_eq!(core().handle(Input::Press(Btn::Rt), &s), vec![Action::FocusTab("w5:t3".into())]);
}

#[test]
fn y_picks_the_nearest_blocked_agent_in_sidebar_order() {
    // Focused w5:p1 (#1); blocked agents in #3 (w0:p3) and #2 (w7:p1), listed in that order: the one in #2 is next.
    let mut s = snap();
    s.agents = vec![
        Agent { pane_id: "w5:p1".into(), workspace_id: "w5".into(), status: Status::Idle },
        Agent { pane_id: "w0:p3".into(), workspace_id: "w0".into(), status: Status::Blocked },
        Agent { pane_id: "w7:p1".into(), workspace_id: "w7".into(), status: Status::Blocked },
    ];
    assert_eq!(core().handle(Input::Press(Btn::Y), &s), vec![Action::FocusAgent("w7:p1".into())]);
}

#[test]
fn a_60_character_transcript_is_not_truncated() {
    let text = "b".repeat(60);
    let out = said(&text, &snap());
    assert_eq!(out[1], toast("Sent", &text, Sound::None));
}

// Review fixes, 2026-10-27: Respond is tied to a pane and a dialog; recordings are cancelled on disconnect;
// a transcript goes to the pane focused when the user spoke.

fn focus_moved() -> Vec<Action> { vec![toast("Navigate mode", "focus moved to another pane", Sound::None)] }

#[test]
fn respond_leaves_when_focus_moves_to_another_pane() {
    // Respond was entered on idle w5:p1; the UI focus moved to a blocked agent in w5:p2. A must not answer it.
    let mut c = in_respond();
    let s = Snapshot { focused: Some(pane("w5:p2", Some("claude"), Some(Status::Blocked))), ..snap() };
    assert_eq!(c.handle(Input::Press(Btn::A), &s), focus_moved());
    assert_eq!(c.mode(), Mode::Navigate);
    assert_eq!(c.handle(Input::Press(Btn::A), &s), blocked_entry("w5:p2"), "the next deliberate press answers the newly focused blocked agent");
}

#[test]
fn respond_leaves_when_nothing_is_focused() {
    let mut c = in_respond();
    let s = Snapshot { focused: None, ..snap() };
    assert_eq!(c.handle(Input::Press(Btn::Down), &s), focus_moved());
    assert_eq!(c.mode(), Mode::Navigate);
}

#[test]
fn view_on_a_moved_focus_still_reports_the_move() {
    let mut c = in_respond();
    let s = Snapshot { focused: Some(pane("w5:p2", Some("claude"), Some(Status::Idle))), ..snap() };
    assert_eq!(c.handle(Input::Press(Btn::View), &s), focus_moved());
    assert_eq!(c.mode(), Mode::Navigate);
}

fn working() -> Snapshot {
    Snapshot { focused: Some(pane("w5:p1", Some("claude"), Some(Status::Working))), ..snap() }
}

#[test]
fn each_dialog_accepts_one_cross_press() {
    let mut c = in_respond();
    // Arrow keys and Cross are delivered directly in Respond.
    assert_eq!(c.handle(Input::Press(Btn::Down), &blocked()), keys("w5:p1", Key::Down));
    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
    // The agent works again; keys still go through.
    assert_eq!(c.handle(Input::Press(Btn::Down), &working()), keys("w5:p1", Key::Down));
    // A newly blocked agent accepts the next Cross immediately.
    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
    assert_eq!(c.mode(), Mode::Respond);
}

#[test]
fn a_on_a_new_dialog_sends_enter_immediately() {
    let mut c = in_respond();
    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
}

#[test]
fn share_escape_does_not_delay_the_next_enter() {
    let mut c = in_respond();
    assert_eq!(c.handle(Input::Press(Btn::View), &blocked()), keys("w5:p1", Key::Esc));
    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
}

#[test]
fn consecutive_cross_presses_each_send_exactly_one_enter() {
    // Each physical Cross press produces one Enter, even if status stays blocked.
    let mut c = in_respond();
    assert_eq!(c.handle(Input::Press(Btn::Down), &blocked()), keys("w5:p1", Key::Down));
    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
}

#[test]
fn an_enter_after_dpad_does_not_delay_the_next_enter() {
    let mut c = in_respond();
    c.handle(Input::Press(Btn::Down), &blocked());
    c.handle(Input::Press(Btn::A), &blocked());
    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter), "every Cross sends Enter");
}

#[test]
fn every_enter_after_blocked_entry_is_sent() {
    let mut c = core();
    c.handle(Input::Press(Btn::A), &blocked());
    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
}

#[test]
fn disconnecting_leaves_respond_and_allows_single_press_reentry() {
    let mut c = in_respond();
    assert_eq!(c.handle(Input::Press(Btn::Down), &blocked()), keys("w5:p1", Key::Down));
    assert_eq!(c.handle(Input::Disconnected, &blocked()), vec![toast("Controller disconnected", "", Sound::None)]);
    assert_eq!(c.mode(), Mode::Navigate);
    assert_eq!(c.handle(Input::Connected, &blocked()), vec![toast("Controller connected", "", Sound::None)]);
    assert_eq!(c.mode(), Mode::Navigate);
    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), blocked_entry("w5:p1"));
    assert_eq!(c.mode(), Mode::Respond);
}

#[test]
fn disconnecting_while_recording_in_respond_cancels_and_leaves() {
    let mut c = in_respond();
    c.handle(Input::Press(Btn::RStick), &snap());
    assert_eq!(c.handle(Input::Disconnected, &snap()), vec![
        Action::CancelRecording,
        toast("Controller disconnected", "recording discarded", Sound::Request),
    ]);
    assert_eq!(c.mode(), Mode::Navigate);
}

#[test]
fn leaving_respond_does_not_delay_a_later_confirmation() {
    let mut c = in_respond();
    c.handle(Input::Press(Btn::Down), &blocked());
    c.handle(Input::Press(Btn::B), &blocked());
    // Re-entering on a blocked pane sends Enter immediately.
    c.handle(Input::Press(Btn::A), &blocked());
    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
    // Entry on idle is consumed; a later blocked confirmation is immediate.
    c.handle(Input::Press(Btn::B), &snap());
    c.handle(Input::Press(Btn::A), &snap());
    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
}

#[test]
fn unused_stick_in_respond_does_not_affect_confirmation() {
    let mut c = in_respond();
    assert_eq!(c.handle(Input::Press(Btn::LStick), &blocked()), vec![]);
    assert_eq!(c.handle(Input::Press(Btn::A), &blocked()), keys("w5:p1", Key::Enter));
}

#[test]
fn disconnecting_while_recording_cancels_it() {
    let mut c = core();
    c.handle(Input::Press(Btn::RStick), &snap());
    assert_eq!(c.handle(Input::Disconnected, &snap()), vec![
        Action::CancelRecording,
        toast("Controller disconnected", "recording discarded", Sound::Request),
    ]);
    assert!(!c.recording());
    assert_eq!(c.handle(Input::Release(Btn::RStick), &snap()), vec![], "the release of the cancelled recording stops nothing");
    assert_eq!(c.handle(Input::Press(Btn::RStick), &snap()), vec![Action::StartRecording], "nothing is transcribing, so the next hold records");
}

#[test]
fn a_second_recording_cannot_start_while_transcribing() {
    let mut c = core();
    spoke(&mut c, &snap());
    assert_eq!(c.handle(Input::Press(Btn::RStick), &snap()), vec![toast("Still transcribing", "wait for the previous one", Sound::None)]);
    assert!(!c.recording());
    assert_eq!(c.handle(Input::Release(Btn::RStick), &snap()), vec![]);
}

#[test]
fn a_transcript_clears_the_way_for_the_next_recording() {
    let mut c = core();
    spoke(&mut c, &snap());
    assert!(matches!(c.handle(Input::Transcript("hi".into()), &snap()).as_slice(), [Action::Prompt { .. }, _]));
    assert_eq!(c.handle(Input::Press(Btn::RStick), &snap()), vec![Action::StartRecording]);
}

#[test]
fn a_failed_transcription_is_toasted_and_clears_the_way_for_the_next_recording() {
    let mut c = core();
    spoke(&mut c, &snap());
    assert_eq!(c.handle(Input::TranscriptFailed("whisper-cli: timed out after 90s".into()), &snap()), vec![
        toast("Transcription failed", "whisper-cli: timed out after 90s", Sound::Request),
    ]);
    assert_eq!(c.handle(Input::Press(Btn::RStick), &snap()), vec![Action::StartRecording]);
}

#[test]
fn the_transcript_target_is_captured_at_release_and_honoured_on_arrival() {
    let mut c = core();
    let p2 = Snapshot { focused: Some(pane("w5:p2", Some("claude"), Some(Status::Idle))), ..snap() };
    assert_eq!(c.handle(Input::Press(Btn::RStick), &snap()), vec![Action::StartRecording]);
    assert_eq!(c.handle(Input::Release(Btn::RStick), &p2), vec![Action::StopRecording]);
    assert_eq!(c.handle(Input::Transcript("hi".into()), &p2), vec![
        Action::Prompt { target: "w5:p2".into(), text: "hi".into() },
        toast("Sent", "hi", Sound::None),
    ]);
}

#[test]
fn a_transcript_is_dropped_when_focus_moved_since_the_user_spoke() {
    let mut c = core();
    spoke(&mut c, &snap());
    let s = Snapshot { focused: Some(pane("w7:p1", Some("claude"), Some(Status::Idle))), ..snap() };
    assert_eq!(c.handle(Input::Transcript("yes".into()), &s), vec![toast("Not sent", "focus moved since you spoke: yes", Sound::Request)]);
    let mut c = core();
    spoke(&mut c, &snap());
    let s = Snapshot { focused: None, ..snap() };
    assert_eq!(c.handle(Input::Transcript("yes".into()), &s), vec![toast("Not sent", "focus moved since you spoke: yes", Sound::Request)]);
}

#[test]
fn a_transcript_is_dropped_when_nothing_was_focused_when_the_user_spoke() {
    let mut c = core();
    spoke(&mut c, &Snapshot { focused: None, ..snap() });
    assert_eq!(c.handle(Input::Transcript("yes".into()), &snap()), vec![toast("Not sent", "nothing was focused when you spoke: yes", Sound::Request)]);
}

#[test]
fn a_transcript_target_is_used_once() {
    let mut c = core();
    spoke(&mut c, &snap());
    c.handle(Input::Transcript("one".into()), &snap());
    assert_eq!(c.handle(Input::Transcript("two".into()), &snap()), vec![toast("Not sent", "nothing was focused when you spoke: two", Sound::Request)]);
    let mut c = core();
    spoke(&mut c, &snap());
    c.handle(Input::TranscriptFailed("x".into()), &snap());
    assert_eq!(c.handle(Input::Transcript("two".into()), &snap()), vec![toast("Not sent", "nothing was focused when you spoke: two", Sound::Request)]);
}

#[test]
fn navigate_on_a_blocked_agent_moves_focus_in_every_direction() {
    for (b, dir) in [(Btn::Up, Dir::Up), (Btn::Down, Dir::Down), (Btn::Left, Dir::Left), (Btn::Right, Dir::Right)] {
        let mut c = core();
        assert_eq!(c.handle(Input::Press(b), &blocked()), vec![Action::FocusDirection { pane_id: "w5:p1".into(), dir }]);
        assert_eq!(c.mode(),Mode::Navigate);
    }
}

#[test]
fn controller_button_labels_are_physical_and_distinct() {
    let cases = [
        (Btn::A, "bottom face button"), (Btn::B, "right face button"),
        (Btn::X, "left face button"), (Btn::Y, "top face button"),
        (Btn::Lb, "left shoulder"), (Btn::Rb, "right shoulder"),
        (Btn::Lt, "left trigger"), (Btn::Rt, "right trigger"),
        (Btn::View, "Back / Select"), (Btn::Menu, "Menu / Options"),
        (Btn::Up, "D-pad up"), (Btn::Down, "D-pad down"),
        (Btn::Left, "D-pad left"), (Btn::Right, "D-pad right"),
        (Btn::LStick, "left stick click"), (Btn::RStick, "right stick click"),
    ];
    for (button, label) in cases { assert_eq!(button.label(), label); }
}

// Close-menu safety coverage now lives in menu_tests and menu_runtime_tests.

fn non_agent() -> Snapshot {
    Snapshot { focused: Some(pane("w5:p2", None, None)), ..snap() }
}
fn in_interact() -> Core {
    let mut c = core();
    let out = c.handle(Input::Press(Btn::A), &non_agent());
    assert_eq!(format!("{:?}", c.mode()), "Interact");
    assert!(matches!(out.as_slice(), [Action::Toast { title, .. }] if title == "Interact mode"));
    c
}
#[test]
fn interact_enters_without_sending_and_maps_all_keys() {
    let mut c = in_interact();
    for (button, key) in [(Btn::Up,"Up"),(Btn::Down,"Down"),(Btn::Left,"Left"),(Btn::Right,"Right"),(Btn::A,"Enter"),(Btn::View,"Esc"),(Btn::X,"Space")] {
        assert_eq!(format!("{:?}", c.handle(Input::Press(button), &non_agent())), format!("[SendPaneKeys {{ target: \"w5:p2\", keys: [{key}] }}]"));
        assert_eq!(format!("{:?}", c.mode()), "Interact");
    }
    assert_eq!(c.handle(Input::Press(Btn::LStick), &non_agent()), vec![]);
}
#[test]
fn interact_rejects_missing_or_empty_target() {
    let _ = in_interact(); // Positive assertion fails before the feature exists.
    for focused in [None, Some(Pane::default())] {
        let mut c=core(); let s=Snapshot{focused,..non_agent()};
        let out=c.handle(Input::Press(Btn::A), &s);
        assert_eq!(c.mode(),Mode::Navigate);
        assert!(matches!(out.as_slice(),[Action::Toast{title,..}] if title=="No pane here"));
    }
}
#[test]
fn interact_focus_or_agent_change_consumes_each_press() {
    let mut missing=non_agent(); missing.focused=None;
    let mut moved=non_agent(); moved.focused.as_mut().unwrap().pane_id="w5:p3".into();
    let mut agent=non_agent(); agent.focused.as_mut().unwrap().agent=Some("codex".into());
    for s in [missing,moved,agent] {
        for b in [Btn::A,Btn::B,Btn::X,Btn::Y,Btn::Up,Btn::Down,Btn::Left,Btn::Right,Btn::Lb,Btn::Rb,Btn::Lt,Btn::Rt] {
            let mut c=in_interact();
            let out=c.handle(Input::Press(b), &s);
            assert_eq!(c.mode(),Mode::Navigate);
            assert!(matches!(out.as_slice(),[Action::Toast{title,..}] if title=="Navigate mode"));
            assert_eq!(c.handle(Input::Press(Btn::X), &non_agent()), vec![Action::ToggleZoom("w5:p2".into())]);
        }
    }
}
#[test]
fn interact_circle_disconnect_and_global_navigation_exit() {
    for b in [Btn::B,Btn::Lb,Btn::Rb,Btn::Lt,Btn::Rt,Btn::Y] {
        let mut c=in_interact();
        let expected=core().handle(Input::Press(b),&non_agent());
        let out=c.handle(Input::Press(b),&non_agent());
        assert_eq!(c.mode(),Mode::Navigate);
        assert_eq!(&out[1..],expected.as_slice());
    }
    let mut c=in_interact(); c.handle(Input::Disconnected,&non_agent());
    assert_eq!(c.mode(),Mode::Navigate);
    assert_eq!(c.handle(Input::Press(Btn::Up),&non_agent()),vec![Action::FocusDirection{pane_id:"w5:p2".into(),dir:Dir::Up}]);
}


#[test]
fn circle_exits_both_modes_without_sending_and_preserves_safe_reentry() {
    for s in [snap(), blocked(), non_agent()] {
        let mut c=core(); c.handle(Input::Press(Btn::A), &s);
        assert_ne!(c.mode(),Mode::Navigate);
        assert_eq!(c.handle(Input::Press(Btn::B),&s),vec![toast("Navigate mode","",Sound::None)]);
        assert_eq!(c.mode(),Mode::Navigate);
        assert_eq!(c.handle(Input::Press(Btn::B),&s),vec![]);
        let entry=c.handle(Input::Press(Btn::A),&s);
        if s.focused.as_ref().is_some_and(|p| p.status == Some(Status::Blocked)) {
            assert_eq!(entry,blocked_entry("w5:p1"));
        } else { assert!(matches!(entry.as_slice(),[Action::Toast{..}])); }
        assert_ne!(c.mode(),Mode::Navigate);
    }
}
#[test]
fn circle_focus_mismatch_never_sends_a_key() {
    let mut positive=in_respond();
    assert_eq!(positive.handle(Input::Press(Btn::B),&snap()),vec![toast("Navigate mode","",Sound::None)]);
    for mut c in [in_respond(),in_interact()] {
        let changed=Snapshot{focused:None,..snap()};
        let out=c.handle(Input::Press(Btn::B),&changed);
        assert_eq!(c.mode(),Mode::Navigate);
        assert!(matches!(out.as_slice(),[Action::Toast{title,..}] if title=="Navigate mode"));
    }
}
#[test]
fn share_escape_keeps_the_mode_and_uses_its_own_route() {
    let mut c=in_respond();
    assert_eq!(c.handle(Input::Press(Btn::View),&blocked()),keys("w5:p1",Key::Esc));
    assert_eq!(c.mode(),Mode::Respond);
    assert_eq!(c.handle(Input::Press(Btn::A),&blocked()),keys("w5:p1",Key::Enter));
    assert_eq!(c.handle(Input::Press(Btn::A),&blocked()),keys("w5:p1",Key::Enter));
    let mut c=in_interact();
    assert_eq!(c.handle(Input::Press(Btn::View),&non_agent()),vec![Action::SendPaneKeys{target:"w5:p2".into(),keys:vec![Key::Esc]}]);
    assert_eq!(c.mode(),Mode::Interact);
    assert_eq!(core().handle(Input::Press(Btn::View),&snap()),vec![]);
}

#[test]
fn navigate_dpad_does_not_get_trapped_by_a_blocked_agent() {
    let mut c=core();
    assert_eq!(c.handle(Input::Press(Btn::Down),&blocked()),vec![Action::FocusDirection{pane_id:"w5:p1".into(),dir:Dir::Down}]);
    assert_eq!(c.mode(),Mode::Navigate);
    c.handle(Input::Press(Btn::A),&blocked());
    assert_eq!(c.mode(),Mode::Respond);
    c.handle(Input::Press(Btn::B),&blocked());
    assert_eq!(c.handle(Input::Press(Btn::Down),&blocked()),vec![Action::FocusDirection{pane_id:"w5:p1".into(),dir:Dir::Down}]);
    assert_eq!(c.mode(),Mode::Navigate);
}

fn blocked_entry(target: &str) -> Vec<Action> {
    vec![toast("Respond mode", "D-pad moves; Cross confirms; Square clears input line (Codex/Claude); Circle returns to Navigate; Create / Share escapes", Sound::None),
        Action::SendKeys{target:target.into(),keys:vec![Key::Enter]}]
}
#[test]
fn single_press_is_only_a_shortcut_for_blocked_agent_entry() {
    assert_eq!(core().handle(Input::Press(Btn::A),&blocked()),blocked_entry("w5:p1"));
    for status in [Status::Idle,Status::Working,Status::Done,Status::Unknown] {
        let mut s=snap();s.focused.as_mut().unwrap().status=Some(status);
        let mut c=core();let out=c.handle(Input::Press(Btn::A),&s);
        assert_eq!(c.mode(),Mode::Respond);
        assert!(matches!(out.as_slice(),[Action::Toast{..}]));
    }
    let mut s=snap();s.focused.as_mut().unwrap().status=None;
    assert!(matches!(core().handle(Input::Press(Btn::A),&s).as_slice(),[Action::Toast{..}]));
    assert!(matches!(core().handle(Input::Press(Btn::A),&non_agent()).as_slice(),[Action::Toast{..}]));
    assert_eq!(said("yes",&blocked()),vec![toast("Not sent","agent is waiting at a dialog: yes",Sound::Request)]);
}
