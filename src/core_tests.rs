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
