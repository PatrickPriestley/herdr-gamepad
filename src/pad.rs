//! The controller: gilrs events become core Inputs. `Raw` exists because gilrs's `Code` cannot be
//! built in tests; `raw()` is the only untested line and it only forgets the code.

use crate::engine::{Btn, Input};
use gilrs::{Button, EventType};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Raw { Pressed(Button), Released(Button), Changed(Button, f32), Connected, Disconnected }

pub fn raw(ev: &EventType) -> Option<Raw> {
    Some(match ev {
        EventType::ButtonPressed(b, _) => Raw::Pressed(*b),
        EventType::ButtonReleased(b, _) => Raw::Released(*b),
        EventType::ButtonChanged(b, v, _) => Raw::Changed(*b, *v),
        EventType::Connected => Raw::Connected,
        EventType::Disconnected => Raw::Disconnected,
        _ => return None,
    })
}

/// Tracks the analog triggers so each crosses into a press once and out of it once.
#[derive(Default)]
pub struct Pad { lt: bool, rt: bool }

impl Pad {
    pub fn new() -> Self { Self::default() }

    pub fn input(&mut self, r: Raw) -> Option<Input> {
        match r {
            Raw::Changed(Button::LeftTrigger2, v) => edge(&mut self.lt, v, Btn::Lt),
            Raw::Changed(Button::RightTrigger2, v) => edge(&mut self.rt, v, Btn::Rt),
            Raw::Changed(..) => None,
            Raw::Pressed(b) if !is_analog(b) => btn(b).map(Input::Press),
            Raw::Released(b) if !is_analog(b) => btn(b).map(Input::Release),
            Raw::Pressed(_) | Raw::Released(_) => None,
            Raw::Connected => Some(Input::Connected),
            Raw::Disconnected => { self.lt = false; self.rt = false; Some(Input::Disconnected) }
        }
    }
}

fn is_analog(b: Button) -> bool { matches!(b, Button::LeftTrigger2 | Button::RightTrigger2) }

fn edge(down: &mut bool, v: f32, b: Btn) -> Option<Input> {
    if !*down && v >= 0.6 { *down = true; Some(Input::Press(b)) }
    else if *down && v <= 0.4 { *down = false; Some(Input::Release(b)) }
    else { None }
}

fn btn(b: Button) -> Option<Btn> {
    Some(match b {
        Button::South => Btn::A,
        Button::East => Btn::B,
        Button::West => Btn::X,
        Button::North => Btn::Y,
        Button::LeftTrigger => Btn::Lb,
        Button::RightTrigger => Btn::Rb,
        Button::Select => Btn::View,
        Button::Start => Btn::Menu,
        Button::LeftThumb => Btn::LStick,
        Button::RightThumb => Btn::RStick,
        Button::DPadUp => Btn::Up,
        Button::DPadDown => Btn::Down,
        Button::DPadLeft => Btn::Left,
        Button::DPadRight => Btn::Right,
        _ => return None,
    })
}

/// `herd-gamepad probe`: print every controller event until Ctrl-C. For checking a controller's mapping.
pub fn probe() -> Result<(), String> {
    let mut g = gilrs::Gilrs::new().map_err(|e| format!("gilrs: {e}"))?;
    for (_, gp) in g.gamepads() { println!("connected: {} ({:?})", gp.name(), gp.mapping_source()); }
    println!("press buttons; Ctrl-C to stop");
    let mut pad = Pad::new();
    loop {
        if let Some(ev) = g.next_event_blocking(Some(std::time::Duration::from_millis(500))) {
            let r = raw(&ev.event);
            println!("{:?} -> {:?}", ev.event, r.and_then(|r| pad.input(r)));
        }
    }
}
