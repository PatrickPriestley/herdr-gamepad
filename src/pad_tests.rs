use crate::engine::{Btn, Input};
use crate::pad::*;
use gilrs::Button;

#[test]
fn physical_buttons_map_to_core_inputs() {
    let mut p = Pad::new();
    for (g, b) in [
        (Button::South, Btn::A), (Button::East, Btn::B), (Button::West, Btn::X), (Button::North, Btn::Y),
        (Button::LeftTrigger, Btn::Lb), (Button::RightTrigger, Btn::Rb),
        (Button::Select, Btn::View), (Button::Start, Btn::Menu),
        (Button::LeftThumb, Btn::LStick), (Button::RightThumb, Btn::RStick),
        (Button::DPadUp, Btn::Up), (Button::DPadDown, Btn::Down), (Button::DPadLeft, Btn::Left), (Button::DPadRight, Btn::Right),
    ] {
        assert_eq!(p.input(Raw::Pressed(g)), Some(Input::Press(b)), "{g:?}");
        assert_eq!(p.input(Raw::Released(g)), Some(Input::Release(b)), "{g:?}");
    }
}

#[test]
fn unknown_buttons_are_ignored() {
    let mut p = Pad::new();
    assert_eq!(p.input(Raw::Pressed(Button::Mode)), None, "the home button is the system's");
    assert_eq!(p.input(Raw::Pressed(Button::Unknown)), None);
    assert_eq!(p.input(Raw::Changed(Button::South, 1.0)), None, "digital buttons use Pressed/Released, not Changed");
}

#[test]
fn analog_triggers_press_past_0_6_and_release_under_0_4() {
    let mut p = Pad::new();
    assert_eq!(p.input(Raw::Changed(Button::LeftTrigger2, 0.3)), None);
    assert_eq!(p.input(Raw::Changed(Button::LeftTrigger2, 0.59)), None, "just under the press threshold");
    assert_eq!(p.input(Raw::Changed(Button::LeftTrigger2, 0.6)), Some(Input::Press(Btn::Lt)));
    assert_eq!(p.input(Raw::Changed(Button::LeftTrigger2, 0.9)), None, "no repeat while held");
    assert_eq!(p.input(Raw::Changed(Button::LeftTrigger2, 0.5)), None, "hysteresis: not released yet");
    assert_eq!(p.input(Raw::Changed(Button::LeftTrigger2, 0.41)), None, "just over the release threshold");
    assert_eq!(p.input(Raw::Changed(Button::LeftTrigger2, 0.4)), Some(Input::Release(Btn::Lt)));
    assert_eq!(p.input(Raw::Changed(Button::RightTrigger2, 1.0)), Some(Input::Press(Btn::Rt)));
    assert_eq!(p.input(Raw::Changed(Button::RightTrigger2, 0.0)), Some(Input::Release(Btn::Rt)));
}

#[test]
fn trigger_pressed_events_are_ignored_so_they_are_not_counted_twice() {
    let mut p = Pad::new();
    assert_eq!(p.input(Raw::Pressed(Button::LeftTrigger2)), None);
    assert_eq!(p.input(Raw::Released(Button::RightTrigger2)), None);
}

#[test]
fn connection_events_pass_through() {
    let mut p = Pad::new();
    assert_eq!(p.input(Raw::Connected), Some(Input::Connected));
    assert_eq!(p.input(Raw::Disconnected), Some(Input::Disconnected));
}

#[test]
fn disconnecting_resets_the_triggers() {
    let mut p = Pad::new();
    assert_eq!(p.input(Raw::Changed(Button::LeftTrigger2, 1.0)), Some(Input::Press(Btn::Lt)));
    assert_eq!(p.input(Raw::Changed(Button::RightTrigger2, 1.0)), Some(Input::Press(Btn::Rt)));
    assert_eq!(p.input(Raw::Disconnected), Some(Input::Disconnected));
    assert_eq!(p.input(Raw::Changed(Button::LeftTrigger2, 1.0)), Some(Input::Press(Btn::Lt)), "a reconnected pad starts with its triggers up");
    assert_eq!(p.input(Raw::Changed(Button::RightTrigger2, 1.0)), Some(Input::Press(Btn::Rt)));
}
