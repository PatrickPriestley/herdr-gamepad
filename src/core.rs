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
        let blocked = agent_pane.map_or(false, |p| p.status == Some(Status::Blocked));
        if self.mode == Mode::Navigate && blocked && matches!(b, Btn::Up | Btn::Down | Btn::Left | Btn::Right) {
            self.mode = Mode::Respond;
            let mut out = vec![toast("Respond mode", "the agent is waiting for you", Sound::None)];
            out.extend(self.respond(b, agent_pane));
            return out;
        }
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
}

fn dir_of(b: Btn) -> Dir {
    match b { Btn::Up => Dir::Up, Btn::Down => Dir::Down, Btn::Left => Dir::Left, _ => Dir::Right }
}

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
