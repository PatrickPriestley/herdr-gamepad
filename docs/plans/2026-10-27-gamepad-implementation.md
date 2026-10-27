# Xbox Controller for Herdr Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** A Herdr plugin, `herd.gamepad`, whose daemon turns Xbox controller presses into Herdr focus changes, agent key presses, and push-to-talk prompts.

**Architecture:** One Rust binary `herd-gamepad`. `core` is a pure state machine: a controller `Input` plus a `Snapshot` of Herdr yields `Action`s. Thin adapters surround it: `pad` (gilrs events to `Input`), `herdr` (a `Snapshot` from, and each `Action` to, the `herdr` CLI), `voice` (cpal recording and whisper-cli transcription), `serve` (the loop). Design: `docs/plans/2026-10-27-gamepad-design.md`.

**Tech Stack:** Rust 1.99, gilrs 0.11, cpal 0.18, hound 3.5, serde_json, toml 0.8, libc; `herdr` 0.9.1 CLI; Homebrew `whisper-cpp` 1.9.5 (`whisper-cli`); cargo-mutants 27.

**Facts probed on 2026-10-27 (do not re-derive):**
- `herdr pane current` returns `result.pane.{pane_id,workspace_id,tab_id,agent,agent_status,focused}`; `workspace list` returns `result.workspaces[].{workspace_id,number,focused}`; `tab list --workspace W` returns `result.tabs[].{tab_id,number,focused}`; `agent list` returns `result.agents[].{pane_id,workspace_id,agent_status}`. Errors are JSON on stderr with exit 1 and codes `pane_not_found`, `agent_not_found`, `workspace_not_found`, `invalid_key`.
- `herdr agent send-keys <pane_id> up` accepts `up down left right enter esc`; `esc` is canonical.
- `herdr notification show TITLE --body B --sound none|done|request`.
- Plugin pane placements: `overlay|popup|split|tab|zoomed`. Plugin dirs: config `~/.config/herdr/plugins/config/<id>` (env `HERDR_PLUGIN_CONFIG_DIR`), state `~/.local/state/herdr/plugins/<id>` (env `HERDR_PLUGIN_STATE_DIR`).
- cpal 0.18: `StreamConfig { channels: 1, sample_rate: 48000, buffer_size: BufferSize::Default }` with `sample_rate` a plain `u32`; `build_input_stream(cfg, …)` takes the config by value; the MacBook microphone offers 44100/48000/88200/96000 Hz only; `DeviceTrait::description()` replaces the old `name()`.
- `whisper-cli -m M -f W -nt -np -l en` prints only the transcript on stdout (leading newline), `[BLANK_AUDIO]` for silence, exits 2 for a missing file, accepts 48 kHz WAV.
- gilrs 0.11: `Gilrs::new()`, `next_event_blocking(Some(Duration))`, `Event { id, event: EventType, time }`; `EventType::{ButtonPressed(Button, Code), ButtonReleased, ButtonChanged(Button, f32, Code), Connected, Disconnected}`; `Code` is not constructible outside the crate (so tests use `pad::Raw`). Xbox mapping: `South`=A, `East`=B, `West`=X, `North`=Y, `LeftTrigger`=LB, `RightTrigger`=RB, `LeftTrigger2`=LT, `RightTrigger2`=RT, `Select`=View, `Start`=Menu, `LeftThumb`/`RightThumb`=stick clicks, `DPad*`.

---

### Task 1: Scaffold the crate

**Files:**
- Create: `Cargo.toml`, `src/main.rs`, `.gitignore`

**Step 1: Create the crate files**

`Cargo.toml`:

```toml
[package]
name = "herd-gamepad"
version = "0.1.0"
edition = "2021"
rust-version = "1.89"
description = "Herdr plugin: drive Herdr with an Xbox controller"
license = "MIT"

[[bin]]
name = "herd-gamepad"
path = "src/main.rs"

[dependencies]
gilrs = "0.11"
cpal = "0.18"
hound = "3.5"
serde_json = "1"
toml = "0.8"
libc = "0.2"

[profile.release]
strip = true
lto = "thin"
```

`src/main.rs`:

```rust
//! herd-gamepad: drive Herdr with an Xbox controller.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("version") | Some("--version") => { println!("herd-gamepad {}", env!("CARGO_PKG_VERSION")); 0 }
        _ => { eprintln!("usage: herd-gamepad version"); 2 }
    };
    std::process::exit(code);
}
```

`.gitignore`:

```
target/
bin/
.DS_Store
mutants.out/
```

**Step 2: Build and run**

Run: `cargo run --quiet -- version`
Expected: `herd-gamepad 0.1.0`

**Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock src/main.rs .gitignore docs/
git commit -m "scaffold: herd-gamepad crate and design"
```

---

### Task 2: Core types and D-pad pane focus

**Files:**
- Create: `src/core.rs`, `src/core_tests.rs`
- Modify: `src/main.rs`

**Step 1: Write the failing tests**

`src/core_tests.rs`:

```rust
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
```

Add to `src/main.rs` top:

```rust
mod core;
#[cfg(test)]
mod core_tests;
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --quiet 2>&1 | head -5`
Expected: compile error, `core` module not found

**Step 3: Write the implementation**

`src/core.rs`:

```rust
//! The pure state machine: a controller input plus a snapshot of Herdr give the actions to run.
//! Nothing here touches hardware or Herdr.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn { A, B, X, Y, Lb, Rb, Lt, Rt, View, Menu, Up, Down, Left, Right, LStick, RStick }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input { Press(Btn), Release(Btn), Connected, Disconnected, Transcript(String) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode { Navigate, Respond }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status { Idle, Working, Blocked, Done, Unknown }

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Pane { pub pane_id: String, pub workspace_id: String, pub tab_id: String, pub agent: Option<String>, pub status: Option<Status> }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Workspace { pub id: String, pub number: u32, pub focused: bool }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tab { pub id: String, pub number: u32, pub focused: bool }

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agent { pub pane_id: String, pub workspace_id: String, pub status: Status }

/// What Herdr looks like right now: the focused pane, the spaces, the focused space's tabs, every agent.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Snapshot { pub focused: Option<Pane>, pub workspaces: Vec<Workspace>, pub tabs: Vec<Tab>, pub agents: Vec<Agent> }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dir { Left, Right, Up, Down }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key { Up, Down, Left, Right, Enter, Esc }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))] // Done is used by tests only until a sound is chosen for it
pub enum Sound { None, Done, Request }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    FocusWorkspace(String),
    FocusTab(String),
    FocusDirection { pane_id: String, dir: Dir },
    FocusAgent(String),
    ToggleZoom(String),
    SendKeys { target: String, keys: Vec<Key> },
    Prompt { target: String, text: String },
    SendText { pane_id: String, text: String },
    Toast { title: String, body: String, sound: Sound },
    StartRecording,
    StopRecording,
}

pub fn toast(title: &str, body: &str, sound: Sound) -> Action {
    Action::Toast { title: title.into(), body: body.into(), sound }
}

pub struct Core { mode: Mode, recording: bool, voice: Btn }

impl Core {
    pub fn new(voice: Btn) -> Self { Core { mode: Mode::Navigate, recording: false, voice } }
    pub fn mode(&self) -> Mode { self.mode }
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn recording(&self) -> bool { self.recording }

    pub fn handle(&mut self, input: Input, snap: &Snapshot) -> Vec<Action> {
        match input {
            Input::Press(b) => self.press(b, snap),
            Input::Release(_) => vec![],
            Input::Connected | Input::Disconnected | Input::Transcript(_) => vec![],
        }
    }

    fn press(&mut self, b: Btn, snap: &Snapshot) -> Vec<Action> {
        let focused = snap.focused.as_ref();
        match b {
            Btn::Up | Btn::Down | Btn::Left | Btn::Right => focused
                .map(|p| vec![Action::FocusDirection { pane_id: p.pane_id.clone(), dir: dir_of(b) }])
                .unwrap_or_default(),
            _ => vec![],
        }
    }
}

fn dir_of(b: Btn) -> Dir {
    match b { Btn::Up => Dir::Up, Btn::Down => Dir::Down, Btn::Left => Dir::Left, _ => Dir::Right }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --quiet 2>&1 | tail -3`
Expected: `test result: ok. 4 passed`

**Step 5: Commit**

```bash
git add src/core.rs src/core_tests.rs src/main.rs
git commit -m "core: input, snapshot and action types; D-pad focuses panes"
```

---

### Task 3: Bumpers step spaces, triggers step tabs

**Files:**
- Modify: `src/core.rs`, `src/core_tests.rs`

**Step 1: Write the failing tests**

Append to `src/core_tests.rs`:

```rust
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
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --quiet 2>&1 | grep -E "^test .*FAILED|panicked" | head`
Expected: the four new tests fail (`assertion failed`, left is `[]`)

**Step 3: Write the implementation**

In `src/core.rs`, replace the `press` method and add the helpers:

```rust
    fn press(&mut self, b: Btn, snap: &Snapshot) -> Vec<Action> {
        let focused = snap.focused.as_ref();
        match b {
            Btn::Up | Btn::Down | Btn::Left | Btn::Right => focused
                .map(|p| vec![Action::FocusDirection { pane_id: p.pane_id.clone(), dir: dir_of(b) }])
                .unwrap_or_default(),
            Btn::Lb => step_workspace(snap, -1),
            Btn::Rb => step_workspace(snap, 1),
            Btn::Lt => step_tab(snap, -1),
            Btn::Rt => step_tab(snap, 1),
            _ => vec![],
        }
    }
```

```rust
/// The space `delta` places from the focused one in sidebar order, wrapping.
fn step_workspace(snap: &Snapshot, delta: i32) -> Vec<Action> {
    let mut ws: Vec<&Workspace> = snap.workspaces.iter().collect();
    ws.sort_by_key(|w| w.number);
    step(ws.len(), ws.iter().position(|w| w.focused), delta)
        .map(|j| vec![Action::FocusWorkspace(ws[j].id.clone())])
        .unwrap_or_default()
}

fn step_tab(snap: &Snapshot, delta: i32) -> Vec<Action> {
    let mut tabs: Vec<&Tab> = snap.tabs.iter().collect();
    tabs.sort_by_key(|t| t.number);
    step(tabs.len(), tabs.iter().position(|t| t.focused), delta)
        .map(|j| vec![Action::FocusTab(tabs[j].id.clone())])
        .unwrap_or_default()
}

/// The index `delta` away from `current` in a ring of `len`, or None when there is nowhere to go.
fn step(len: usize, current: Option<usize>, delta: i32) -> Option<usize> {
    if len < 2 { return None; }
    let i = current? as i32;
    Some((i + delta).rem_euclid(len as i32) as usize)
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --quiet 2>&1 | tail -3`
Expected: `test result: ok. 8 passed`

**Step 5: Commit**

```bash
git add src/core.rs src/core_tests.rs
git commit -m "core: bumpers step spaces, triggers step tabs, both wrap"
```

---

### Task 4: Y finds the agent that needs you; X zooms

**Files:**
- Modify: `src/core.rs`, `src/core_tests.rs`

**Step 1: Write the failing tests**

Append to `src/core_tests.rs`:

```rust
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
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --quiet 2>&1 | grep -c FAILED`
Expected: `1` (the summary line) and five failing tests listed above it

**Step 3: Write the implementation**

In `press`, add the arms before `_ => vec![]`:

```rust
            Btn::Y => next_attention(snap),
            Btn::X => focused.map(|p| vec![Action::ToggleZoom(p.pane_id.clone())]).unwrap_or_default(),
```

Add the helper:

```rust
/// The first blocked agent after the focused pane in sidebar order (space number, then pane id),
/// wrapping; failing that the first done one; failing that a toast.
fn next_attention(snap: &Snapshot) -> Vec<Action> {
    let number = |ws: &str| snap.workspaces.iter().find(|w| w.id == ws).map_or(u32::MAX, |w| w.number);
    let mut agents: Vec<&Agent> = snap.agents.iter().collect();
    agents.sort_by(|a, b| (number(&a.workspace_id), &a.pane_id).cmp(&(number(&b.workspace_id), &b.pane_id)));
    let n = agents.len();
    if n == 0 { return vec![toast("No agent needs you", "", Sound::None)]; }
    let start = snap.focused.as_ref()
        .and_then(|p| agents.iter().position(|a| a.pane_id == p.pane_id))
        .map_or(0, |i| i + 1);
    let pick = |want: Status| (0..n).map(|k| agents[(start + k) % n]).find(|a| a.status == want);
    match pick(Status::Blocked).or_else(|| pick(Status::Done)) {
        Some(a) => vec![Action::FocusAgent(a.pane_id.clone())],
        None => vec![toast("No agent needs you", "", Sound::None)],
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --quiet 2>&1 | tail -3`
Expected: `test result: ok. 13 passed`

**Step 5: Commit**

```bash
git add src/core.rs src/core_tests.rs
git commit -m "core: Y focuses the next agent that needs you, X toggles zoom"
```

---

### Task 5: Respond mode

**Files:**
- Modify: `src/core.rs`, `src/core_tests.rs`

**Step 1: Write the failing tests**

Append to `src/core_tests.rs`:

```rust
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
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --quiet 2>&1 | grep -E "^test .*FAILED" | wc -l`
Expected: `6` (menu_and_stick_clicks passes already; it guards against later regressions)

**Step 3: Write the implementation**

Replace `press` in `src/core.rs` and add `leave_respond` and `respond`:

```rust
    fn press(&mut self, b: Btn, snap: &Snapshot) -> Vec<Action> {
        let focused = snap.focused.as_ref();
        let agent_pane = focused.filter(|p| p.agent.is_some());
        // Navigation works in both modes and leaves Respond.
        let nav = match b {
            Btn::Lb => Some(step_workspace(snap, -1)),
            Btn::Rb => Some(step_workspace(snap, 1)),
            Btn::Lt => Some(step_tab(snap, -1)),
            Btn::Rt => Some(step_tab(snap, 1)),
            Btn::Y => Some(next_attention(snap)),
            Btn::X => Some(focused.map(|p| vec![Action::ToggleZoom(p.pane_id.clone())]).unwrap_or_default()),
            _ => None,
        };
        if let Some(actions) = nav { return self.leave_respond(actions); }
        match self.mode {
            Mode::Navigate => match b {
                Btn::Up | Btn::Down | Btn::Left | Btn::Right => focused
                    .map(|p| vec![Action::FocusDirection { pane_id: p.pane_id.clone(), dir: dir_of(b) }])
                    .unwrap_or_default(),
                Btn::A => match agent_pane {
                    Some(_) => { self.mode = Mode::Respond; vec![toast("Respond mode", "D-pad moves, A enters, B escapes, View leaves", Sound::None)] }
                    None => vec![toast("No agent here", "focus a pane with an agent first", Sound::None)],
                },
                _ => vec![],
            },
            Mode::Respond => self.respond(b, agent_pane),
        }
    }

    fn leave_respond(&mut self, actions: Vec<Action>) -> Vec<Action> {
        if self.mode != Mode::Respond { return actions; }
        self.mode = Mode::Navigate;
        let mut out = vec![toast("Navigate mode", "", Sound::None)];
        out.extend(actions);
        out
    }

    fn respond(&mut self, b: Btn, agent_pane: Option<&Pane>) -> Vec<Action> {
        if b == Btn::View {
            self.mode = Mode::Navigate;
            return vec![toast("Navigate mode", "", Sound::None)];
        }
        let Some(p) = agent_pane else {
            self.mode = Mode::Navigate;
            return vec![toast("Navigate mode", "no agent in the focused pane", Sound::None)];
        };
        let key = match b {
            Btn::Up => Key::Up, Btn::Down => Key::Down, Btn::Left => Key::Left, Btn::Right => Key::Right,
            Btn::A => Key::Enter, Btn::B => Key::Esc,
            _ => return vec![],
        };
        vec![Action::SendKeys { target: p.pane_id.clone(), keys: vec![key] }]
    }
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --quiet 2>&1 | tail -3`
Expected: `test result: ok. 20 passed`

**Step 5: Commit**

```bash
git add src/core.rs src/core_tests.rs
git commit -m "core: Respond mode sends D-pad, A and B to the focused agent"
```

---

### Task 6: A blocked agent switches the D-pad to Respond

**Files:**
- Modify: `src/core.rs`, `src/core_tests.rs`

**Step 1: Write the failing tests**

Append to `src/core_tests.rs`:

```rust
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
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --quiet 2>&1 | grep -E "^test .*FAILED"`
Expected: `dpad_on_a_blocked_agent_switches_to_respond_and_sends_the_key` fails

**Step 3: Write the implementation**

In `press`, insert after `if let Some(actions) = nav { … }`:

```rust
        let blocked = agent_pane.map_or(false, |p| p.status == Some(Status::Blocked));
        if self.mode == Mode::Navigate && blocked && matches!(b, Btn::Up | Btn::Down | Btn::Left | Btn::Right) {
            self.mode = Mode::Respond;
            let mut out = vec![toast("Respond mode", "the agent is waiting for you", Sound::None)];
            out.extend(self.respond(b, agent_pane));
            return out;
        }
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --quiet 2>&1 | tail -3`
Expected: `test result: ok. 23 passed`

**Step 5: Commit**

```bash
git add src/core.rs src/core_tests.rs
git commit -m "core: the D-pad answers a blocked agent directly; A never confirms on the switching press"
```

---

### Task 7: Push-to-talk and transcript routing

**Files:**
- Modify: `src/core.rs`, `src/core_tests.rs`

**Step 1: Write the failing tests**

Append to `src/core_tests.rs`:

```rust
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
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --quiet 2>&1 | grep -E "^test .*FAILED" | wc -l`
Expected: `9`

**Step 3: Write the implementation**

Replace `handle` in `src/core.rs` and add `transcript` and `preview`:

```rust
    pub fn handle(&mut self, input: Input, snap: &Snapshot) -> Vec<Action> {
        match input {
            Input::Press(b) if b == self.voice => {
                if self.recording { return vec![]; }
                self.recording = true;
                vec![Action::StartRecording]
            }
            Input::Release(b) if b == self.voice => {
                if !self.recording { return vec![]; }
                self.recording = false;
                vec![Action::StopRecording]
            }
            Input::Press(b) => self.press(b, snap),
            Input::Release(_) => vec![],
            Input::Connected => vec![toast("Controller connected", "", Sound::None)],
            Input::Disconnected => vec![toast("Controller disconnected", "", Sound::None)],
            Input::Transcript(text) => transcript(text, snap),
        }
    }
```

```rust
/// Where a transcript goes depends on what the focused agent is doing.
fn transcript(text: String, snap: &Snapshot) -> Vec<Action> {
    let text = text.trim().to_string();
    if text.is_empty() { return vec![toast("Heard nothing", "", Sound::None)]; }
    let short = preview(&text);
    let Some(p) = snap.focused.as_ref().filter(|p| p.agent.is_some()) else {
        return vec![toast("Not sent", &format!("no agent in the focused pane: {short}"), Sound::Request)];
    };
    match p.status {
        Some(Status::Idle) | Some(Status::Done) => vec![
            Action::Prompt { target: p.pane_id.clone(), text },
            toast("Sent", &short, Sound::None),
        ],
        Some(Status::Working) => vec![
            Action::SendText { pane_id: p.pane_id.clone(), text },
            toast("Typed, not sent", &format!("agent is working: {short}"), Sound::None),
        ],
        _ => vec![toast("Not sent", &format!("agent is waiting at a dialog: {short}"), Sound::Request)],
    }
}

fn preview(text: &str) -> String {
    let mut s: String = text.chars().take(60).collect();
    if text.chars().count() > 60 { s.push('…'); }
    s
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --quiet 2>&1 | tail -3`
Expected: `test result: ok. 32 passed`

**Step 5: Commit**

```bash
git add src/core.rs src/core_tests.rs
git commit -m "core: push-to-talk recording and transcript routing by agent state"
```

---

### Task 8: Mutation-test the core

**Files:**
- Modify: `src/core_tests.rs`

**Step 1: Run cargo-mutants and see what survives**

Run: `cargo mutants --file src/core.rs --timeout 60 2>&1 | tail -3`
Expected (probed 2026-10-27 against this exact code): `78 mutants tested … 6 missed, 61 caught, 11 unviable`. The six, from `mutants.out/missed.txt`:

```
replace match guard b == self.voice with true in Core::handle   (any release stops a recording)
delete - in Core::press                                         (LT steps forward; two tabs hide it)
delete match arm Btn::Up in dir_of                              (Up becomes Right)
replace == with != in next_attention (number lookup)            (sidebar order ignored)
replace == with != in next_attention (focused position)         (the ring starts one too late)
replace > with >= in preview                                    (a 60-character text gets an ellipsis)
```

**Step 2: Add the tests that kill them**

Append to `src/core_tests.rs`:

```rust
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
    // Focused w5:p1 (#1); blocked agents in #2 (w7:p1) and #3 (w0:p3): the one in #2 is next.
    let mut s = snap();
    s.agents[1].status = Status::Blocked;
    assert_eq!(core().handle(Input::Press(Btn::Y), &s), vec![Action::FocusAgent("w7:p1".into())]);
}

#[test]
fn a_60_character_transcript_is_not_truncated() {
    let text = "b".repeat(60);
    let out = core().handle(Input::Transcript(text.clone()), &snap());
    assert_eq!(out[1], toast("Sent", &text, Sound::None));
}
```

**Step 3: Run the tests, then the mutants again**

Run: `cargo test --quiet 2>&1 | tail -3`
Expected: `test result: ok. 37 passed`

Run: `cargo mutants --file src/core.rs --timeout 60 2>&1 | tail -2`
Expected (probed): `78 mutants tested … 67 caught, 11 unviable`, nothing missed. A new survivor means a guard without a test: add the test; do not delete the guard without reasoning about it.

**Step 4: Commit**

```bash
git add src/core_tests.rs
git commit -m "core: tests for the guards cargo-mutants found uncovered"
```

---

### Task 9: Herdr snapshot parsing

**Files:**
- Create: `src/herdr.rs`, `src/herdr_tests.rs`
- Modify: `src/main.rs`

**Step 1: Write the failing tests**

`src/herdr_tests.rs` (the JSON is trimmed from live `herdr` output on 2026-10-27):

```rust
use crate::core::*;
use crate::herdr::*;
use std::cell::RefCell;

/// A Runner that answers each argv from a table and records what was asked.
pub struct Fake { pub calls: RefCell<Vec<Vec<String>>>, pub answers: Vec<(&'static str, Result<&'static str, &'static str>)> }

impl Fake {
    pub fn new(answers: Vec<(&'static str, Result<&'static str, &'static str>)>) -> Self { Fake { calls: RefCell::new(vec![]), answers } }
    pub fn calls(&self) -> Vec<String> { self.calls.borrow().iter().map(|c| c.join(" ")).collect() }
}

impl Runner for Fake {
    fn run(&self, argv: &[&str]) -> Result<String, String> {
        let joined = argv.join(" ");
        self.calls.borrow_mut().push(argv.iter().map(|s| s.to_string()).collect());
        for (prefix, answer) in &self.answers {
            if joined.starts_with(prefix) { return answer.map(String::from).map_err(String::from); }
        }
        Err(format!("fake: no answer for {joined}"))
    }
}

const PANE_CURRENT: &str = r#"{"id":"cli:pane:current","result":{"pane":{"agent":"claude","agent_status":"working","cwd":"/Users/p/Code","focused":true,"pane_id":"w1M:p1","revision":6,"tab_id":"w1M:t1","terminal_id":"term_65ed","workspace_id":"w1M"},"type":"pane_current"}}"#;
const PANE_CURRENT_SHELL: &str = r#"{"id":"cli:pane:current","result":{"pane":{"cwd":"/Users/p","focused":true,"pane_id":"w5:p2","revision":1,"tab_id":"w5:t1","terminal_id":"term_1","workspace_id":"w5"},"type":"pane_current"}}"#;
const WORKSPACES: &str = r#"{"id":"cli:workspace:list","result":{"type":"workspace_list","workspaces":[{"active_tab_id":"w5:t1","agent_status":"idle","focused":false,"label":"meal-plan","number":1,"pane_count":4,"tab_count":1,"workspace_id":"w5"},{"active_tab_id":"w1M:t1","agent_status":"working","focused":true,"label":"Code","number":9,"pane_count":1,"tab_count":1,"workspace_id":"w1M"}]}}"#;
const TABS: &str = r#"{"id":"cli:tab:list","result":{"tabs":[{"agent_status":"idle","focused":false,"label":"1","number":1,"pane_count":4,"tab_id":"w7:t1","workspace_id":"w7"},{"agent_status":"unknown","focused":true,"label":"cursor-probe","number":7,"pane_count":2,"tab_id":"w7:t7","workspace_id":"w7"}],"type":"tab_list"}}"#;
const AGENTS: &str = r#"{"id":"cli:agent:list","result":{"agents":[{"agent":"claude","agent_status":"idle","focused":false,"pane_id":"w5:p1","revision":3,"tab_id":"w5:t1","workspace_id":"w5"},{"agent":"codex","agent_status":"done","focused":false,"name":"reviewer2","pane_id":"w0:p6","tab_id":"w0:t1","workspace_id":"w0"},{"agent":"claude","agent_status":"blocked","pane_id":"w7:p21","tab_id":"w7:t1","workspace_id":"w7"},{"agent":"grok","agent_status":"mystery","pane_id":"w9:p1","tab_id":"w9:t1","workspace_id":"w9"}],"type":"agent_list"}}"#;

fn fake() -> Fake {
    Fake::new(vec![
        ("herdr pane current", Ok(PANE_CURRENT)),
        ("herdr workspace list", Ok(WORKSPACES)),
        ("herdr tab list --workspace w1M", Ok(TABS)),
        ("herdr agent list", Ok(AGENTS)),
    ])
}

#[test]
fn snapshot_reads_the_focused_pane_spaces_tabs_and_agents() {
    let f = fake();
    let s = Herdr { runner: &f }.snapshot().unwrap();
    assert_eq!(s.focused, Some(Pane { pane_id: "w1M:p1".into(), workspace_id: "w1M".into(), tab_id: "w1M:t1".into(), agent: Some("claude".into()), status: Some(Status::Working) }));
    assert_eq!(s.workspaces, vec![
        Workspace { id: "w5".into(), number: 1, focused: false },
        Workspace { id: "w1M".into(), number: 9, focused: true },
    ]);
    assert_eq!(s.tabs, vec![Tab { id: "w7:t1".into(), number: 1, focused: false }, Tab { id: "w7:t7".into(), number: 7, focused: true }]);
    assert_eq!(s.agents[0], Agent { pane_id: "w5:p1".into(), workspace_id: "w5".into(), status: Status::Idle });
    assert_eq!(s.agents[1].status, Status::Done);
    assert_eq!(s.agents[2].status, Status::Blocked);
    assert_eq!(s.agents[3].status, Status::Unknown, "a status this build does not know is Unknown, not an error");
    assert_eq!(f.calls(), ["herdr pane current", "herdr workspace list", "herdr tab list --workspace w1M", "herdr agent list"], "tabs are listed for the focused pane's space");
}

#[test]
fn a_shell_pane_has_no_agent_and_no_status() {
    let mut f = fake();
    f.answers[0].1 = Ok(PANE_CURRENT_SHELL);
    f.answers.push(("herdr tab list --workspace w5", Ok(TABS)));
    let s = Herdr { runner: &f }.snapshot().unwrap();
    assert_eq!(s.focused, Some(Pane { pane_id: "w5:p2".into(), workspace_id: "w5".into(), tab_id: "w5:t1".into(), agent: None, status: None }));
}

#[test]
fn a_failed_command_fails_the_snapshot_with_its_stderr() {
    let mut f = fake();
    f.answers[1].1 = Err(r#"{"error":{"code":"server_unavailable","message":"no server"}}"#);
    let err = Herdr { runner: &f }.snapshot().unwrap_err();
    assert!(err.contains("server_unavailable"), "{err}");
}

#[test]
fn bad_json_is_an_error_naming_the_command() {
    let mut f = fake();
    f.answers[3].1 = Ok("not json");
    let err = Herdr { runner: &f }.snapshot().unwrap_err();
    assert!(err.starts_with("agent list: bad json"), "{err}");
}
```

Add to `src/main.rs`:

```rust
mod herdr;
#[cfg(test)]
mod herdr_tests;
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --quiet 2>&1 | head -5`
Expected: compile error, `herdr` module not found

**Step 3: Write the implementation**

`src/herdr.rs`:

```rust
//! Herdr through the `herdr` CLI: a Snapshot of what is focused, and each Action as one command.

use crate::core::*;
use serde_json::Value;

/// Runs a command; Ok(stdout) on exit 0, Err(stderr) otherwise. Faked in tests.
pub trait Runner {
    fn run(&self, argv: &[&str]) -> Result<String, String>;
}

impl<R: Runner> Runner for &R {
    fn run(&self, argv: &[&str]) -> Result<String, String> { (*self).run(argv) }
}

pub struct Cli;

impl Runner for Cli {
    fn run(&self, argv: &[&str]) -> Result<String, String> {
        let out = std::process::Command::new(argv[0]).args(&argv[1..]).output()
            .map_err(|e| format!("{}: {e}", argv[0]))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }
}

pub struct Herdr<R: Runner> { pub runner: R }

impl<R: Runner> Herdr<R> {
    pub fn snapshot(&self) -> Result<Snapshot, String> {
        let focused = parse_pane(&self.json(&["herdr", "pane", "current"])?);
        let workspaces = parse_workspaces(&self.json(&["herdr", "workspace", "list"])?);
        let tabs = match &focused {
            Some(p) => parse_tabs(&self.json(&["herdr", "tab", "list", "--workspace", &p.workspace_id])?),
            None => vec![],
        };
        let agents = parse_agents(&self.json(&["herdr", "agent", "list"])?);
        Ok(Snapshot { focused, workspaces, tabs, agents })
    }

    fn json(&self, argv: &[&str]) -> Result<Value, String> {
        let out = self.runner.run(argv)?;
        serde_json::from_str(&out).map_err(|e| format!("{}: bad json: {e}", argv[1..].join(" ")))
    }
}

fn status(v: &Value) -> Option<Status> {
    Some(match v.as_str()? {
        "idle" => Status::Idle,
        "working" => Status::Working,
        "blocked" => Status::Blocked,
        "done" => Status::Done,
        _ => Status::Unknown,
    })
}

fn text(v: &Value, key: &str) -> String { v[key].as_str().unwrap_or_default().to_string() }

pub fn parse_pane(v: &Value) -> Option<Pane> {
    let p = &v["result"]["pane"];
    if !p.is_object() { return None; }
    Some(Pane {
        pane_id: text(p, "pane_id"),
        workspace_id: text(p, "workspace_id"),
        tab_id: text(p, "tab_id"),
        agent: p["agent"].as_str().map(String::from),
        status: status(&p["agent_status"]),
    })
}

pub fn parse_workspaces(v: &Value) -> Vec<Workspace> {
    v["result"]["workspaces"].as_array().map(|a| a.iter().map(|w| Workspace {
        id: text(w, "workspace_id"),
        number: w["number"].as_u64().unwrap_or(0) as u32,
        focused: w["focused"].as_bool().unwrap_or(false),
    }).collect()).unwrap_or_default()
}

pub fn parse_tabs(v: &Value) -> Vec<Tab> {
    v["result"]["tabs"].as_array().map(|a| a.iter().map(|t| Tab {
        id: text(t, "tab_id"),
        number: t["number"].as_u64().unwrap_or(0) as u32,
        focused: t["focused"].as_bool().unwrap_or(false),
    }).collect()).unwrap_or_default()
}

pub fn parse_agents(v: &Value) -> Vec<Agent> {
    v["result"]["agents"].as_array().map(|a| a.iter().map(|g| Agent {
        pane_id: text(g, "pane_id"),
        workspace_id: text(g, "workspace_id"),
        status: status(&g["agent_status"]).unwrap_or(Status::Unknown),
    }).collect()).unwrap_or_default()
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --quiet 2>&1 | tail -3`
Expected: `test result: ok. 41 passed`

**Step 5: Commit**

```bash
git add src/herdr.rs src/herdr_tests.rs src/main.rs
git commit -m "herdr: a Snapshot from pane current, workspace list, tab list and agent list"
```

---

### Task 10: Actions become herdr commands

**Files:**
- Modify: `src/herdr.rs`, `src/herdr_tests.rs`

**Step 1: Write the failing tests**

Append to `src/herdr_tests.rs`:

```rust
#[test]
fn each_action_is_one_herdr_command() {
    let cases: Vec<(Action, &str)> = vec![
        (Action::FocusWorkspace("w7".into()), "herdr workspace focus w7"),
        (Action::FocusTab("w7:t7".into()), "herdr tab focus w7:t7"),
        (Action::FocusDirection { pane_id: "w5:p1".into(), dir: Dir::Left }, "herdr pane focus --direction left --pane w5:p1"),
        (Action::FocusDirection { pane_id: "w5:p1".into(), dir: Dir::Down }, "herdr pane focus --direction down --pane w5:p1"),
        (Action::FocusAgent("w0:p3".into()), "herdr agent focus w0:p3"),
        (Action::ToggleZoom("w5:p1".into()), "herdr pane zoom --pane w5:p1 --toggle"),
        (Action::SendKeys { target: "w5:p1".into(), keys: vec![Key::Up, Key::Enter] }, "herdr agent send-keys w5:p1 up enter"),
        (Action::SendKeys { target: "w5:p1".into(), keys: vec![Key::Esc] }, "herdr agent send-keys w5:p1 esc"),
        (Action::SendKeys { target: "w5:p1".into(), keys: vec![Key::Left, Key::Right, Key::Down] }, "herdr agent send-keys w5:p1 left right down"),
        (Action::Prompt { target: "w5:p1".into(), text: "run the tests".into() }, "herdr agent prompt w5:p1 run the tests"),
        (Action::SendText { pane_id: "w5:p1".into(), text: "also lint".into() }, "herdr pane send-text w5:p1 also lint"),
        (toast("Sent", "run the tests", Sound::None), "herdr notification show Sent --body run the tests --sound none"),
        (toast("Not sent", "x", Sound::Request), "herdr notification show Not sent --body x --sound request"),
        (toast("Navigate mode", "", Sound::Done), "herdr notification show Navigate mode --sound done"),
    ];
    for (action, expect) in cases {
        assert_eq!(argv_for(&action).join(" "), expect, "{action:?}");
    }
}

#[test]
fn prompt_text_is_one_argument_even_with_spaces() {
    let argv = argv_for(&Action::Prompt { target: "w5:p1".into(), text: "run the tests".into() });
    assert_eq!(argv.len(), 5, "{argv:?}");
}

#[test]
fn recording_actions_are_not_herdr_commands() {
    assert!(argv_for(&Action::StartRecording).is_empty());
    assert!(argv_for(&Action::StopRecording).is_empty());
    let f = Fake::new(vec![]);
    Herdr { runner: &f }.apply(&Action::StartRecording).unwrap();
    assert!(f.calls().is_empty(), "apply must not run a command for them");
}

#[test]
fn apply_runs_the_command_and_surfaces_the_error() {
    let f = Fake::new(vec![("herdr agent send-keys", Err(r#"{"error":{"code":"agent_not_found","message":"agent target w5:p2 not found"}}"#))]);
    let err = Herdr { runner: &f }.apply(&Action::SendKeys { target: "w5:p2".into(), keys: vec![Key::Enter] }).unwrap_err();
    assert_eq!(err, "agent_not_found: agent target w5:p2 not found");
    let f = Fake::new(vec![("herdr agent send-keys", Err("boom"))]);
    let err = Herdr { runner: &f }.apply(&Action::SendKeys { target: "w5:p2".into(), keys: vec![Key::Enter] }).unwrap_err();
    assert_eq!(err, "boom", "stderr that is not Herdr's JSON is passed through");
}
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --quiet 2>&1 | head -5`
Expected: compile error, `argv_for` and `apply` not found

**Step 3: Write the implementation**

Append to `src/herdr.rs`:

```rust
impl<R: Runner> Herdr<R> {
    pub fn apply(&self, action: &Action) -> Result<(), String> {
        let argv = argv_for(action);
        if argv.is_empty() { return Ok(()); }
        let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        self.runner.run(&refs).map(|_| ()).map_err(|e| error_message(&e))
    }
}

/// Herdr's JSON error as "code: message"; anything else as it came.
fn error_message(stderr: &str) -> String {
    match serde_json::from_str::<Value>(stderr) {
        Ok(v) if v["error"]["code"].is_string() => format!("{}: {}", text(&v["error"], "code"), text(&v["error"], "message")),
        _ => stderr.to_string(),
    }
}

/// The herdr command for an action; empty for the recording actions, which serve handles itself.
pub fn argv_for(a: &Action) -> Vec<String> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    match a {
        Action::FocusWorkspace(id) => s(&["herdr", "workspace", "focus", id]),
        Action::FocusTab(id) => s(&["herdr", "tab", "focus", id]),
        Action::FocusDirection { pane_id, dir } => s(&["herdr", "pane", "focus", "--direction", dir_name(*dir), "--pane", pane_id]),
        Action::FocusAgent(target) => s(&["herdr", "agent", "focus", target]),
        Action::ToggleZoom(pane_id) => s(&["herdr", "pane", "zoom", "--pane", pane_id, "--toggle"]),
        Action::SendKeys { target, keys } => {
            let mut v = s(&["herdr", "agent", "send-keys", target]);
            v.extend(keys.iter().map(|k| key_name(*k).to_string()));
            v
        }
        Action::Prompt { target, text } => s(&["herdr", "agent", "prompt", target, text]),
        Action::SendText { pane_id, text } => s(&["herdr", "pane", "send-text", pane_id, text]),
        Action::Toast { title, body, sound } => {
            let mut v = s(&["herdr", "notification", "show", title]);
            if !body.is_empty() { v.extend(s(&["--body", body])); }
            v.extend(s(&["--sound", sound_name(*sound)]));
            v
        }
        Action::StartRecording | Action::StopRecording => vec![],
    }
}

fn dir_name(d: Dir) -> &'static str {
    match d { Dir::Left => "left", Dir::Right => "right", Dir::Up => "up", Dir::Down => "down" }
}

fn key_name(k: Key) -> &'static str {
    match k { Key::Up => "up", Key::Down => "down", Key::Left => "left", Key::Right => "right", Key::Enter => "enter", Key::Esc => "esc" }
}

fn sound_name(s: Sound) -> &'static str {
    match s { Sound::None => "none", Sound::Done => "done", Sound::Request => "request" }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --quiet 2>&1 | tail -3`
Expected: `test result: ok. 45 passed`

**Step 5: Commit**

```bash
git add src/herdr.rs src/herdr_tests.rs
git commit -m "herdr: each Action is one herdr command; errors carry Herdr's code"
```

---

### Task 11: Config and plugin directories

**Files:**
- Create: `src/config.rs`, `src/config_tests.rs`
- Modify: `src/main.rs`

**Step 1: Write the failing tests**

`src/config_tests.rs`:

```rust
use crate::config::*;
use crate::core::Btn;
use std::path::{Path, PathBuf};

fn state() -> PathBuf { PathBuf::from("/tmp/state") }

#[test]
fn an_empty_config_gives_the_defaults() {
    let c = parse("", &state()).unwrap();
    assert_eq!(c, Config { model: Path::new("/tmp/state/models/ggml-base.en.bin").into(), language: "en".into(), voice_button: Btn::RStick, max_seconds: 60 });
}

#[test]
fn every_field_can_be_set() {
    let text = "[whisper]\nmodel = \"/m/small.bin\"\nlanguage = \"de\"\n[voice]\nbutton = \"menu\"\nmax_seconds = 20\n";
    let c = parse(text, &state()).unwrap();
    assert_eq!(c, Config { model: "/m/small.bin".into(), language: "de".into(), voice_button: Btn::Menu, max_seconds: 20 });
}

#[test]
fn an_empty_model_means_the_default_and_tilde_expands() {
    assert_eq!(parse("[whisper]\nmodel = \"\"\n", &state()).unwrap().model, Path::new("/tmp/state/models/ggml-base.en.bin"));
    let home = std::env::var("HOME").unwrap();
    assert_eq!(parse("[whisper]\nmodel = \"~/m.bin\"\n", &state()).unwrap().model, Path::new(&home).join("m.bin"));
}

#[test]
fn the_voice_button_must_be_one_that_can_be_held() {
    assert_eq!(parse("[voice]\nbutton = \"lstick\"\n", &state()).unwrap().voice_button, Btn::LStick);
    let err = parse("[voice]\nbutton = \"a\"\n", &state()).unwrap_err();
    assert!(err.contains("voice.button") && err.contains("rstick"), "{err}");
}

#[test]
fn max_seconds_is_at_least_one() {
    assert_eq!(parse("[voice]\nmax_seconds = 0\n", &state()).unwrap().max_seconds, 1);
    assert_eq!(parse("[voice]\nmax_seconds = -5\n", &state()).unwrap().max_seconds, 1);
}

#[test]
fn bad_toml_is_an_error_naming_the_file() {
    let err = parse("[whisper\n", &state()).unwrap_err();
    assert!(err.starts_with("config.toml:"), "{err}");
}

#[test]
fn dirs_follow_the_herdr_plugin_env_overrides() {
    let (state, config) = dirs_from(Some("/s".into()), Some("/c".into()), "/home/x");
    assert_eq!((state, config), (PathBuf::from("/s"), PathBuf::from("/c")));
    let (state, config) = dirs_from(None, None, "/home/x");
    assert_eq!(state, Path::new("/home/x/.local/state/herdr/plugins/herd.gamepad"));
    assert_eq!(config, Path::new("/home/x/.config/herdr/plugins/config/herd.gamepad"));
}
```

Add to `src/main.rs`:

```rust
mod config;
#[cfg(test)]
mod config_tests;
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --quiet 2>&1 | head -5`
Expected: compile error, `config` module not found

**Step 3: Write the implementation**

`src/config.rs`:

```rust
//! The plugin's settings file and its Herdr-assigned directories.

use crate::core::Btn;
use std::path::{Path, PathBuf};

pub const PLUGIN_ID: &str = "herd.gamepad";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config { pub model: PathBuf, pub language: String, pub voice_button: Btn, pub max_seconds: u64 }

/// (state dir, config dir), as Herdr sets them for a plugin process, with the documented fallbacks.
pub fn dirs() -> (PathBuf, PathBuf) {
    let (state, config) = dirs_from(
        std::env::var_os("HERDR_PLUGIN_STATE_DIR").map(PathBuf::from),
        std::env::var_os("HERDR_PLUGIN_CONFIG_DIR").map(PathBuf::from),
        &std::env::var("HOME").unwrap_or_default(),
    );
    let _ = std::fs::create_dir_all(&state);
    (state, config)
}

pub fn dirs_from(state: Option<PathBuf>, config: Option<PathBuf>, home: &str) -> (PathBuf, PathBuf) {
    (
        state.unwrap_or_else(|| Path::new(home).join(".local/state/herdr/plugins").join(PLUGIN_ID)),
        config.unwrap_or_else(|| Path::new(home).join(".config/herdr/plugins/config").join(PLUGIN_ID)),
    )
}

pub fn load() -> Result<(Config, PathBuf), String> {
    let (state, config) = dirs();
    let text = std::fs::read_to_string(config.join("config.toml")).unwrap_or_default();
    Ok((parse(&text, &state)?, state))
}

pub fn parse(text: &str, state: &Path) -> Result<Config, String> {
    let t: toml::Table = text.parse().map_err(|e| format!("config.toml: {e}"))?;
    let get = |a: &str, b: &str| t.get(a).and_then(|v| v.get(b));
    let model = match get("whisper", "model").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        Some(s) => expand(s),
        None => state.join("models/ggml-base.en.bin"),
    };
    let language = get("whisper", "language").and_then(|v| v.as_str()).unwrap_or("en").to_string();
    let voice_button = match get("voice", "button").and_then(|v| v.as_str()).unwrap_or("rstick") {
        "rstick" => Btn::RStick,
        "lstick" => Btn::LStick,
        "menu" => Btn::Menu,
        other => return Err(format!("config.toml: voice.button {other:?} is not one of rstick, lstick, menu")),
    };
    let max_seconds = get("voice", "max_seconds").and_then(|v| v.as_integer()).unwrap_or(60).max(1) as u64;
    Ok(Config { model, language, voice_button, max_seconds })
}

fn expand(s: &str) -> PathBuf {
    match s.strip_prefix("~/") {
        Some(rest) => Path::new(&std::env::var("HOME").unwrap_or_default()).join(rest),
        None => PathBuf::from(s),
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --quiet 2>&1 | tail -3`
Expected: `test result: ok. 52 passed`

**Step 5: Commit**

```bash
git add src/config.rs src/config_tests.rs src/main.rs
git commit -m "config: whisper model, language, voice button and plugin directories"
```

---

### Task 12: Voice: WAV, whisper-cli and transcript cleaning

**Files:**
- Create: `src/voice.rs`, `src/voice_tests.rs`
- Modify: `src/main.rs`

**Step 1: Write the failing tests**

`src/voice_tests.rs`:

```rust
use crate::herdr_tests::Fake;
use crate::voice::*;
use std::path::Path;

#[test]
fn clean_keeps_the_words_and_drops_whisper_markers() {
    assert_eq!(clean("\n Review the diff and report only actionable findings."), "Review the diff and report only actionable findings.");
    assert_eq!(clean("\n [BLANK_AUDIO]"), "");
    assert_eq!(clean(" [BLANK_AUDIO] run the tests (typing) please\n"), "run the tests please");
    assert_eq!(clean("one\n two   three\n"), "one two three");
    assert_eq!(clean("say (hello) to [them] now"), "say to now", "markers are dropped even mid-sentence");
}

#[test]
fn transcribe_runs_whisper_cli_with_the_probed_flags() {
    let f = Fake::new(vec![("whisper-cli", Ok("\n Hello there."))]);
    let text = transcribe(&f, Path::new("/m/base.bin"), Path::new("/s/voice.wav"), "en").unwrap();
    assert_eq!(text, "Hello there.");
    assert_eq!(f.calls(), ["whisper-cli -m /m/base.bin -f /s/voice.wav -nt -np -l en"]);
}

#[test]
fn transcribe_surfaces_whisper_failure() {
    let f = Fake::new(vec![("whisper-cli", Err("error: failed to read audio"))]);
    assert_eq!(transcribe(&f, Path::new("/m"), Path::new("/w"), "en").unwrap_err(), "error: failed to read audio");
}

#[test]
fn recordings_under_300_ms_are_too_short() {
    assert!(!long_enough(0));
    assert!(!long_enough(RATE as usize * 3 / 10 - 1));
    assert!(long_enough(RATE as usize * 3 / 10));
}

#[test]
fn write_wav_writes_16_bit_mono_at_the_capture_rate() {
    let dir = std::env::temp_dir().join(format!("herd-gamepad-wav-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("t.wav");
    let samples: Vec<f32> = (0..4800).map(|i| ((i as f32) * 0.01).sin()).collect();
    write_wav(&path, &samples).unwrap();
    let r = hound::WavReader::open(&path).unwrap();
    assert_eq!(r.spec(), hound::WavSpec { channels: 1, sample_rate: RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int });
    assert_eq!(r.len(), 4800);
    std::fs::remove_dir_all(&dir).unwrap();
}
```

Add to `src/main.rs`:

```rust
mod voice;
#[cfg(test)]
mod voice_tests;
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --quiet 2>&1 | head -5`
Expected: compile error, `voice` module not found

**Step 3: Write the implementation**

`src/voice.rs`:

```rust
//! Push-to-talk: record the microphone with cpal, write a WAV, transcribe it with whisper-cli.

use crate::herdr::Runner;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::path::Path;
use std::sync::{Arc, Mutex};

/// The capture rate. The MacBook microphone does not offer 16 kHz; whisper-cli resamples itself.
pub const RATE: u32 = 48_000;

pub struct Recorder { stream: cpal::Stream, buf: Arc<Mutex<Vec<f32>>> }

impl Recorder {
    /// Starts capturing mono f32 from the default input, keeping at most `max_seconds`.
    pub fn start(max_seconds: u64) -> Result<Self, String> {
        let dev = cpal::default_host().default_input_device().ok_or("no input device")?;
        let cfg = cpal::StreamConfig { channels: 1, sample_rate: RATE, buffer_size: cpal::BufferSize::Default };
        let buf: Arc<Mutex<Vec<f32>>> = Arc::new(Mutex::new(Vec::new()));
        let max = max_seconds as usize * RATE as usize;
        let sink = buf.clone();
        let stream = dev.build_input_stream(
            cfg,
            move |data: &[f32], _| {
                let mut v = sink.lock().unwrap();
                let room = max.saturating_sub(v.len());
                v.extend_from_slice(&data[..data.len().min(room)]);
            },
            |e| eprintln!("herd-gamepad: recorder: {e}"),
            None,
        ).map_err(|e| format!("input stream: {e}"))?;
        stream.play().map_err(|e| format!("play: {e}"))?;
        Ok(Recorder { stream, buf })
    }

    pub fn stop(self) -> Vec<f32> {
        let Recorder { stream, buf } = self;
        drop(stream);
        let v = buf.lock().unwrap().clone();
        v
    }
}

pub fn long_enough(samples: usize) -> bool { samples >= RATE as usize * 3 / 10 }

pub fn write_wav(path: &Path, samples: &[f32]) -> Result<(), String> {
    let spec = hound::WavSpec { channels: 1, sample_rate: RATE, bits_per_sample: 16, sample_format: hound::SampleFormat::Int };
    let mut w = hound::WavWriter::create(path, spec).map_err(|e| e.to_string())?;
    for s in samples {
        w.write_sample((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).map_err(|e| e.to_string())?;
    }
    w.finalize().map_err(|e| e.to_string())
}

pub fn transcribe(r: &impl Runner, model: &Path, wav: &Path, language: &str) -> Result<String, String> {
    let out = r.run(&["whisper-cli", "-m", &model.to_string_lossy(), "-f", &wav.to_string_lossy(), "-nt", "-np", "-l", language])?;
    Ok(clean(&out))
}

/// whisper-cli's stdout as one line of words, without bracketed markers such as [BLANK_AUDIO] or (typing).
pub fn clean(raw: &str) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for c in raw.chars() {
        match c {
            '[' | '(' => depth += 1,
            ']' | ')' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}
```

`Fake` in `herdr_tests.rs` is `pub`; `voice_tests` reaches it through `crate::herdr_tests::Fake`, which exists only under `cfg(test)`, as does `voice_tests`.

**Step 4: Run tests to verify they pass**

Run: `cargo test --quiet 2>&1 | tail -3`
Expected: `test result: ok. 57 passed`

**Step 5: Probe the recorder for real**

Add a temporary `probe-mic` subcommand is not needed: the serve loop in Task 14 exercises it. Skip.

**Step 6: Commit**

```bash
git add src/voice.rs src/voice_tests.rs src/main.rs
git commit -m "voice: cpal recorder, WAV writer, whisper-cli transcription and marker cleaning"
```

---

### Task 13: The controller adapter

**Files:**
- Create: `src/pad.rs`, `src/pad_tests.rs`
- Modify: `src/main.rs`

**Step 1: Write the failing tests**

`src/pad_tests.rs`:

```rust
use crate::core::{Btn, Input};
use crate::pad::*;
use gilrs::Button;

#[test]
fn xbox_buttons_map_to_their_labels() {
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
    assert_eq!(p.input(Raw::Pressed(Button::Mode)), None, "the Xbox button is the system's");
    assert_eq!(p.input(Raw::Pressed(Button::Unknown)), None);
    assert_eq!(p.input(Raw::Changed(Button::South, 1.0)), None, "digital buttons use Pressed/Released, not Changed");
}

#[test]
fn analog_triggers_press_past_0_6_and_release_under_0_4() {
    let mut p = Pad::new();
    assert_eq!(p.input(Raw::Changed(Button::LeftTrigger2, 0.3)), None);
    assert_eq!(p.input(Raw::Changed(Button::LeftTrigger2, 0.6)), Some(Input::Press(Btn::Lt)));
    assert_eq!(p.input(Raw::Changed(Button::LeftTrigger2, 0.9)), None, "no repeat while held");
    assert_eq!(p.input(Raw::Changed(Button::LeftTrigger2, 0.5)), None, "hysteresis: not released yet");
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
```

Add to `src/main.rs`:

```rust
mod pad;
#[cfg(test)]
mod pad_tests;
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --quiet 2>&1 | head -5`
Expected: compile error, `pad` module not found

**Step 3: Write the implementation**

`src/pad.rs`:

```rust
//! The controller: gilrs events become core Inputs. `Raw` exists because gilrs's `Code` cannot be
//! built in tests; `raw()` is the only untested line and it only forgets the code.

use crate::core::{Btn, Input};
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
pub struct Pad { lt: bool, rt: bool }

impl Pad {
    pub fn new() -> Self { Pad { lt: false, rt: false } }

    pub fn input(&mut self, r: Raw) -> Option<Input> {
        match r {
            Raw::Changed(Button::LeftTrigger2, v) => edge(&mut self.lt, v, Btn::Lt),
            Raw::Changed(Button::RightTrigger2, v) => edge(&mut self.rt, v, Btn::Rt),
            Raw::Changed(..) => None,
            Raw::Pressed(b) if !is_analog(b) => btn(b).map(Input::Press),
            Raw::Released(b) if !is_analog(b) => btn(b).map(Input::Release),
            Raw::Pressed(_) | Raw::Released(_) => None,
            Raw::Connected => Some(Input::Connected),
            Raw::Disconnected => Some(Input::Disconnected),
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
    loop {
        if let Some(ev) = g.next_event_blocking(Some(std::time::Duration::from_millis(500))) {
            let r = raw(&ev.event);
            println!("{:?} -> {:?}", ev.event, r.and_then(|r| Pad::new().input(r)));
        }
    }
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test --quiet 2>&1 | tail -3`
Expected: `test result: ok. 62 passed`

**Step 5: Wire `probe` into main and commit**

In `src/main.rs`, add the arm `Some("probe") => run(pad::probe),` and the helper:

```rust
fn run(f: impl FnOnce() -> Result<(), String>) -> i32 {
    match f() { Ok(()) => 0, Err(e) => { eprintln!("herd-gamepad: {e}"); 1 } }
}
```

Run: `cargo run --quiet -- probe` with no controller.
Expected: `press buttons; Ctrl-C to stop` (then Ctrl-C). With a paired controller, each press prints its mapping. **Check here that LT and RT arrive as `ButtonChanged(LeftTrigger2, …)`** on the real controller; if they arrive only as `ButtonPressed`, drop `is_analog` and its test and handle them like the other buttons.

```bash
git add src/pad.rs src/pad_tests.rs src/main.rs
git commit -m "pad: gilrs buttons to core inputs, with hysteresis on the analog triggers"
```

---

### Task 14: The serve loop and the remaining subcommands

**Files:**
- Create: `src/serve.rs`
- Modify: `src/main.rs`

No unit tests: `serve` only wires tested parts together. It is exercised by Task 16.

**Step 1: Write the loop**

`src/serve.rs`:

```rust
//! The daemon: controller events in, herdr commands out, with push-to-talk transcribed off-thread.

use crate::config::Config;
use crate::core::*;
use crate::herdr::{Cli, Herdr, Runner};
use crate::pad::{self, Pad};
use crate::voice::{self, Recorder};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

pub fn serve() -> Result<(), String> {
    let (cfg, state) = crate::config::load()?;
    if !cfg.model.exists() {
        return Err(format!("whisper model missing at {}; run: herd-gamepad setup", cfg.model.display()));
    }
    let herdr = Herdr { runner: Cli };
    let mut gilrs = gilrs::Gilrs::new().map_err(|e| format!("gilrs: {e}"))?;
    let mut pad = Pad::new();
    let mut core = Core::new(cfg.voice_button);
    let mut recorder: Option<Recorder> = None;
    let (tx, rx) = mpsc::channel::<Input>();
    std::fs::write(state.join("daemon.pid"), std::process::id().to_string()).map_err(|e| e.to_string())?;
    eprintln!("herd-gamepad: listening; {} controller(s) connected; hold {:?} to talk", gilrs.gamepads().count(), cfg.voice_button);
    loop {
        let mut inputs = vec![];
        if let Some(ev) = gilrs.next_event_blocking(Some(Duration::from_millis(100))) {
            if let Some(i) = pad::raw(&ev.event).and_then(|r| pad.input(r)) { inputs.push(i); }
        }
        inputs.extend(rx.try_iter());
        for input in inputs {
            // A release only matters to the voice button; it needs no snapshot.
            let snap = if matches!(input, Input::Release(_)) { Snapshot::default() } else {
                herdr.snapshot().unwrap_or_else(|e| { eprintln!("herd-gamepad: snapshot: {e}"); Snapshot::default() })
            };
            eprintln!("herd-gamepad: {input:?} in {:?}", core.mode());
            for action in core.handle(input, &snap) {
                match action {
                    Action::StartRecording => match Recorder::start(cfg.max_seconds) {
                        Ok(r) => recorder = Some(r),
                        Err(e) => eprintln!("herd-gamepad: record: {e}"),
                    },
                    Action::StopRecording => if let Some(r) = recorder.take() {
                        spawn_transcribe(r.stop(), &cfg, &state, tx.clone());
                    },
                    other => if let Err(e) = herdr.apply(&other) {
                        eprintln!("herd-gamepad: {other:?}: {e}");
                        let _ = herdr.apply(&toast("Herdr refused", &e, Sound::Request));
                    },
                }
            }
        }
    }
}

fn spawn_transcribe(samples: Vec<f32>, cfg: &Config, state: &Path, tx: mpsc::Sender<Input>) {
    if !voice::long_enough(samples.len()) {
        let _ = tx.send(Input::Transcript(String::new()));
        return;
    }
    let (model, lang, wav): (PathBuf, String, PathBuf) = (cfg.model.clone(), cfg.language.clone(), state.join("voice.wav"));
    std::thread::spawn(move || {
        let text = voice::write_wav(&wav, &samples).and_then(|_| voice::transcribe(&Cli, &model, &wav, &lang));
        match text {
            Ok(t) => { let _ = tx.send(Input::Transcript(t)); }
            Err(e) => eprintln!("herd-gamepad: transcribe: {e}"),
        }
    });
}

/// `ensure`: open the daemon's tab unless a daemon is already running.
pub fn ensure() -> Result<(), String> {
    let (state, _) = crate::config::dirs();
    if let Ok(pid) = std::fs::read_to_string(state.join("daemon.pid")) {
        if let Ok(pid) = pid.trim().parse::<i32>() {
            if unsafe { libc::kill(pid, 0) } == 0 { println!("herd-gamepad already running (pid {pid})"); return Ok(()); }
        }
    }
    Cli.run(&["herdr", "plugin", "pane", "open", "--plugin", crate::config::PLUGIN_ID, "--entrypoint", "daemon", "--no-focus"]).map(|_| ())
}

const MODEL_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin";

/// `setup`: check whisper-cli is installed and download the model the config points at when missing.
pub fn setup() -> Result<(), String> {
    let (cfg, _) = crate::config::load()?;
    Cli.run(&["whisper-cli", "--help"]).map_err(|_| "whisper-cli not found; run: brew install whisper-cpp".to_string())?;
    if cfg.model.exists() { println!("model present: {}", cfg.model.display()); return Ok(()); }
    let dir = cfg.model.parent().ok_or("model path has no parent")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = cfg.model.with_extension("part");
    println!("downloading {MODEL_URL} (148 MB)…");
    Cli.run(&["curl", "-fsSL", "-o", &tmp.to_string_lossy(), MODEL_URL])?;
    std::fs::rename(&tmp, &cfg.model).map_err(|e| e.to_string())?;
    println!("model saved: {}", cfg.model.display());
    Ok(())
}

pub const MAPPING: &str = "\
herd-gamepad                                   View = back to Navigate

Navigate (default)                 Respond (after A, or a D-pad press on a blocked agent)
  D-pad   focus pane                 D-pad   up / down / left / right
  LB RB   previous / next space      A       enter
  LT RT   previous / next tab        B       esc
  Y       next agent that needs you  LB RB LT RT Y X  as in Navigate, then back to Navigate
  X       zoom the pane
  A       Respond mode

Hold the right stick click (or voice.button in config.toml) to talk; release to send.
  idle or done agent: sent    working: typed, not sent    blocked or none: dropped

Press Enter to close.
";
```

**Step 2: Finish main.rs**

`src/main.rs`:

```rust
//! herd-gamepad: drive Herdr with an Xbox controller.

mod config;
mod core;
mod herdr;
mod pad;
mod serve;
mod voice;
#[cfg(test)] mod config_tests;
#[cfg(test)] mod core_tests;
#[cfg(test)] mod herdr_tests;
#[cfg(test)] mod pad_tests;
#[cfg(test)] mod voice_tests;

use herdr::Runner;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        Some("serve") => run(serve::serve),
        Some("ensure") => run(serve::ensure),
        Some("setup") => run(serve::setup),
        Some("probe") => run(pad::probe),
        Some("mapping") => { print!("{}", serve::MAPPING); let _ = std::io::stdin().read_line(&mut String::new()); 0 }
        Some("open-mapping") => run(|| herdr::Cli.run(&["herdr", "plugin", "pane", "open", "--plugin", config::PLUGIN_ID, "--entrypoint", "mapping"]).map(|_| ())),
        Some("version") | Some("--version") => { println!("herd-gamepad {}", env!("CARGO_PKG_VERSION")); 0 }
        _ => { eprintln!("usage: herd-gamepad serve|ensure|setup|probe|mapping|open-mapping|version"); 2 }
    };
    std::process::exit(code);
}

fn run(f: impl FnOnce() -> Result<(), String>) -> i32 {
    match f() { Ok(()) => 0, Err(e) => { eprintln!("herd-gamepad: {e}"); 1 } }
}
```

**Step 3: Build, run the tests, try the subcommands**

Run: `cargo build --quiet 2>&1 | head; cargo test --quiet 2>&1 | tail -2`
Expected: no warnings; `test result: ok. 62 passed`

Run: `cargo run --quiet -- setup`
Expected: downloads the model into `~/.local/state/herdr/plugins/herd.gamepad/models/` (or says it is present), exit 0.

Run: `cargo run --quiet -- serve` in a shell pane inside Herdr, with no controller; press Ctrl-C after the banner.
Expected: `herd-gamepad: listening; 0 controller(s) connected; hold RStick to talk`. The first `Recorder::start` on this machine will trigger the macOS microphone prompt for the terminal application if it has not been granted yet.

**Step 4: Commit**

```bash
git add src/serve.rs src/main.rs
git commit -m "serve: the daemon loop, ensure, setup and the mapping card"
```

---

### Task 15: Plugin manifest, build and install scripts

**Files:**
- Create: `herdr-plugin.toml`, `scripts/build.sh`, `install.sh`, `README.md`, `tests/manifest.rs`
- Modify: `Cargo.toml` (dev-dependency `toml` is already a dependency; nothing to add)

**Step 1: Write the failing test**

`tests/manifest.rs`:

```rust
//! The manifest calls real subcommands of the binary it builds.
#[test]
fn entries_call_real_subcommands() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/herdr-plugin.toml")).unwrap();
    let m: toml::Table = text.parse().unwrap();
    assert_eq!(m["id"].as_str(), Some("herd.gamepad"));
    let real = ["serve", "ensure", "setup", "mapping", "open-mapping"];
    let mut entries = vec![];
    for key in ["panes", "actions"] { entries.extend(m[key].as_array().unwrap().iter().cloned()); }
    assert!(!entries.is_empty());
    for e in &entries {
        let cmd: Vec<&str> = e["command"].as_array().unwrap().iter().map(|c| c.as_str().unwrap()).collect();
        assert_eq!(cmd[0], "./bin/herd-gamepad", "{e:?}");
        assert!(real.contains(&cmd[1]), "{e:?}");
    }
    let panes = m["panes"].as_array().unwrap();
    let daemon = panes.iter().find(|p| p["id"].as_str() == Some("daemon")).unwrap();
    assert_eq!(daemon["command"][1].as_str(), Some("serve"));
    assert_eq!(daemon["placement"].as_str(), Some("tab"), "the daemon lives in its own tab, out of the layout");
    let mapping = panes.iter().find(|p| p["id"].as_str() == Some("mapping")).unwrap();
    assert_eq!(mapping["placement"].as_str(), Some("overlay"));
    let actions = m["actions"].as_array().unwrap();
    for sub in ["ensure", "setup", "open-mapping"] {
        assert!(actions.iter().any(|a| a["command"][1].as_str() == Some(sub)), "an action runs {sub}");
    }
    let build = m["build"].as_array().unwrap();
    assert!(build.iter().any(|b| b["command"].as_array().unwrap().iter().any(|c| c.as_str() == Some("scripts/build.sh"))));
    assert!(m["platforms"].as_array().unwrap().iter().any(|p| p.as_str() == Some("macos")));
}
```

**Step 2: Run the test to verify it fails**

Run: `cargo test --quiet --test manifest 2>&1 | grep -E "panicked|No such file"`
Expected: panics reading `herdr-plugin.toml`

**Step 3: Write the manifest and scripts**

`herdr-plugin.toml`:

```toml
# herd.gamepad: drive Herdr with an Xbox controller.
# Design: docs/plans/2026-10-27-gamepad-design.md
# Settings: $(herdr plugin config-dir herd.gamepad)/config.toml — whisper.model, whisper.language,
# voice.button (rstick, lstick, menu), voice.max_seconds.

id = "herd.gamepad"
name = "Gamepad"
version = "0.1.0"
description = "Drive Herdr with an Xbox controller: focus panes, tabs and spaces, answer agents, hold a button to talk."
min_herdr_version = "0.9.1"
platforms = ["macos"]

[[build]]
platforms = ["macos"]
command = ["/bin/sh", "scripts/build.sh"]

[[panes]]
id = "daemon"
title = "Gamepad"
placement = "tab"
command = ["./bin/herd-gamepad", "serve"]

[[panes]]
id = "mapping"
title = "Gamepad mapping"
placement = "overlay"
command = ["./bin/herd-gamepad", "mapping"]

[[actions]]
id = "start"
title = "Gamepad: start"
description = "Start listening to the controller in its own tab, unless already running."
command = ["./bin/herd-gamepad", "ensure"]

[[actions]]
id = "setup"
title = "Gamepad: set up voice"
description = "Check whisper-cli and download the speech model."
command = ["./bin/herd-gamepad", "setup"]

[[actions]]
id = "show-mapping"
title = "Gamepad: show the mapping"
description = "What each button does."
command = ["./bin/herd-gamepad", "open-mapping"]
```

`scripts/build.sh`:

```sh
#!/bin/sh
# Put the herd-gamepad binary at bin/herd-gamepad (or $HERD_GAMEPAD_OUT). An installed copy
# that already has its binary and ships no sources does nothing. Needs Rust (rustup.rs).
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
out=${HERD_GAMEPAD_OUT:-"$root/bin/herd-gamepad"}
if [ -z "${HERD_GAMEPAD_OUT:-}" ] && [ -x "$out" ] && [ ! -f "$root/Cargo.toml" ]; then
  exit 0
fi
command -v cargo >/dev/null 2>&1 || { echo "herd-gamepad: cargo not found; install Rust from https://rustup.rs" >&2; exit 1; }
( cd "$root" && cargo build --release --quiet )
mkdir -p "$(dirname -- "$out")"
cp "$root/target/release/herd-gamepad" "$out"
```

`install.sh`:

```bash
#!/usr/bin/env bash
# install.sh — build herd-gamepad, copy it to a stable folder and link it into Herdr.
#   install.sh [--dest DIR] [--dry-run]
# Re-run to update. Settings and state are untouched (Herdr's per-plugin folders).
set -euo pipefail
src="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
dest="${HOME}/.local/share/herdr-plugins/gamepad"
dry=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --dest) dest="$2"; shift 2 ;;
    --dry-run) dry=1; shift ;;
    *) echo "usage: install.sh [--dest DIR] [--dry-run]" >&2; exit 2 ;;
  esac
done
case "$dest" in /*) ;; *) dest="$PWD/$dest" ;; esac
marker=".herd-gamepad-install"
if [ -e "$dest" ] && [ ! -f "$dest/$marker" ]; then
  echo "install.sh: $dest exists and is not a herd-gamepad install; choose --dest" >&2
  exit 1
fi
if [ "$dry" = 1 ]; then
  echo "would: build bin/herd-gamepad, copy herdr-plugin.toml README.md scripts/build.sh to $dest, then: herdr plugin link $dest"
  exit 0
fi
mkdir -p "$(dirname "$dest")"
tmp="$(mktemp -d "$dest.new.XXXXXX")"
cp "$src/herdr-plugin.toml" "$src/README.md" "$tmp/"
mkdir -p "$tmp/scripts" && cp "$src/scripts/build.sh" "$tmp/scripts/build.sh"
HERD_GAMEPAD_OUT="$tmp/bin/herd-gamepad" /bin/sh "$src/scripts/build.sh"
( cd "$src" && git rev-parse --short HEAD 2>/dev/null || echo unknown ) > "$tmp/$marker"
rm -rf "$dest.prev"
[ -e "$dest" ] && mv "$dest" "$dest.prev"
mv "$tmp" "$dest"
rm -rf "$dest.prev"
herdr plugin unlink herd.gamepad >/dev/null 2>&1 || true
herdr plugin link "$dest"
echo "installed to $dest and linked; run the action 'Gamepad: set up voice', then 'Gamepad: start'"
```

`README.md`:

```markdown
# herd.gamepad

A Herdr plugin that lets an Xbox controller drive Herdr: the D-pad moves between panes,
the bumpers and triggers between spaces and tabs, Y jumps to the agent that needs you,
A and B answer its prompts, and holding the right stick click dictates a prompt through
whisper.cpp on your machine.

## Install

    brew install whisper-cpp        # whisper-cli
    ./install.sh                    # builds, copies to ~/.local/share/herdr-plugins/gamepad, links it

Pair the controller in System Settings > Bluetooth (hold the pairing button on the
controller until the Xbox button blinks fast). Then, in Herdr's command palette:
"Gamepad: set up voice" (downloads the 148 MB base.en model once), then "Gamepad: start".

## Buttons

See "Gamepad: show the mapping" in the palette, or `herd-gamepad mapping`.

## Settings

`$(herdr plugin config-dir herd.gamepad)/config.toml`:

    [whisper]
    model = ""          # empty: <state>/models/ggml-base.en.bin
    language = "en"
    [voice]
    button = "rstick"   # rstick, lstick or menu
    max_seconds = 60

## Design

`docs/plans/2026-10-27-gamepad-design.md`.
```

Run: `chmod +x install.sh scripts/build.sh`

**Step 4: Run the tests to verify they pass**

Run: `cargo test --quiet 2>&1 | grep "test result"`
Expected: two lines, both `ok` (62 unit tests, 1 manifest test)

**Step 5: Commit**

```bash
git add herdr-plugin.toml scripts/build.sh install.sh README.md tests/manifest.rs
git commit -m "plugin: manifest with daemon tab, mapping overlay and actions; build and install scripts"
```

---

### Task 16: Install and run end to end

Needs the Xbox controller paired and `brew install whisper-cpp` done (it was, on 2026-10-27).

**Step 1: Install**

Run: `./install.sh`
Expected: ends with `installed to … and linked`. `herdr plugin list` shows `herd.gamepad (Gamepad) enabled`.

**Step 2: Set up voice, start the daemon**

Run: `herdr plugin action invoke setup --plugin herd.gamepad` then `herdr plugin action invoke start --plugin herd.gamepad`
Expected: a tab titled "Gamepad" appears in the current space with the listening banner. Running `start` again prints `already running`.

**Step 3: Walk the mapping with a shell pane and two agents**

Open a second space with one Claude pane and one shell pane split beside it. Then, on the controller:

| press | expect |
|---|---|
| D-pad right / left | focus moves between the two panes |
| RB, LB | the space changes and comes back; the sidebar highlight follows |
| RT with one tab | nothing (no toast) |
| X twice | the pane zooms and unzooms |
| A on the shell pane | toast "No agent here" |
| A on the agent pane | toast "Respond mode" |
| hold right stick, say "reply with the word pong", release | toast "Sent: reply with the word pong"; the agent answers |
| ask the agent to run a command needing approval, then D-pad down, A | the option moves, then confirms; the agent proceeds |
| Y from the shell pane while another agent is blocked | focus jumps to that agent |
| Menu | nothing |

Each failure here is a `herdr` error in the Gamepad tab: read it before changing code.

**Step 4: Record what the real controller reports**

Run `herd-gamepad probe` once and keep the trigger lines in `docs/plans/2026-10-27-gamepad-design.md` under a new "Probed on hardware" heading (which events LT and RT produce, and whether `Select` is the View button on this controller firmware).

**Step 5: Commit the notes**

```bash
git add docs/plans/2026-10-27-gamepad-design.md
git commit -m "docs: what the controller reports on hardware"
```

---

### Task 17: Independent review

Per `~/AGENT-WORKFLOW.md`: have a different model family read `src/core.rs`, `src/serve.rs` and `src/voice.rs` and hunt for holes, in particular any path by which a transcript or a key press could reach an agent the user did not intend, or a blocked dialog could be confirmed with one press. Require a concrete failure case for each finding, verify it, and fix only the verified ones with a failing test first.
