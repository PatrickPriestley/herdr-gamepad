//! The pure state machine: a controller input plus a snapshot of Herdr give the actions to run.
//! Nothing here touches hardware or Herdr.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn { A, B, X, Y, Lb, Rb, Lt, Rt, View, Menu, Up, Down, Left, Right, LStick, RStick }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input { Press(Btn), Release(Btn), Connected, Disconnected, Transcript(String), TranscriptFailed(String) }

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
#[cfg_attr(not(test), allow(dead_code))] // Done is not yet used by a toast; the herdr tests map it.
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
    CancelRecording,
}

pub fn toast(title: &str, body: &str, sound: Sound) -> Action {
    Action::Toast { title: title.into(), body: body.into(), sound }
}

pub struct Core {
    mode: Mode,
    voice_button: Btn,
    /// The pane Respond was entered on; a press with another pane focused leaves Respond instead of answering.
    respond_pane: Option<String>,
    /// The user has pressed a key while this pane's agent was Blocked: the next A may answer the dialog.
    acknowledged_block: bool,
    recording: bool,
    /// A transcription is running; its result decides `voice_target`'s fate.
    transcribing: bool,
    /// The pane focused when the voice button was released: the only pane the transcript may go to.
    voice_target: Option<String>,
}

impl Core {
    pub fn new(voice_button: Btn) -> Self {
        Core { mode: Mode::Navigate, voice_button, respond_pane: None, acknowledged_block: false, recording: false, transcribing: false, voice_target: None }
    }
    pub fn mode(&self) -> Mode { self.mode }
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn recording(&self) -> bool { self.recording }

    pub fn handle(&mut self, input: Input, snap: &Snapshot) -> Vec<Action> {
        match input {
            Input::Press(b) if b == self.voice_button => {
                if self.recording { return vec![]; }
                if self.transcribing { return vec![toast("Still transcribing", "wait for the previous one", Sound::None)]; }
                self.recording = true;
                vec![Action::StartRecording]
            }
            Input::Release(b) if b == self.voice_button => {
                if !self.recording { return vec![]; }
                self.recording = false;
                self.transcribing = true;
                self.voice_target = snap.focused.as_ref().map(|p| p.pane_id.clone());
                vec![Action::StopRecording]
            }
            Input::Press(b) => self.press(b, snap),
            Input::Release(_) => vec![],
            Input::Connected => vec![toast("Controller connected", "", Sound::None)],
            Input::Disconnected => {
                if !self.recording { return vec![toast("Controller disconnected", "", Sound::None)]; }
                self.recording = false;
                vec![Action::CancelRecording, toast("Controller disconnected", "recording discarded", Sound::Request)]
            }
            Input::Transcript(text) => {
                self.transcribing = false;
                let target = self.voice_target.take();
                transcript(text, target, snap)
            }
            Input::TranscriptFailed(e) => {
                self.transcribing = false;
                self.voice_target = None;
                vec![toast("Transcription failed", &e, Sound::Request)]
            }
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
        let blocked = agent_pane.is_some_and(|p| p.status == Some(Status::Blocked));
        if self.mode == Mode::Navigate && blocked && matches!(b, Btn::Up | Btn::Down | Btn::Left | Btn::Right) {
            // The D-pad press is the acknowledgement of this dialog; A may answer it next.
            self.enter_respond(agent_pane, true);
            let mut out = vec![toast("Respond mode", "the agent is waiting for you", Sound::Request)];
            out.extend(self.respond(b, snap));
            return out;
        }
        match self.mode {
            Mode::Navigate => match b {
                Btn::Up | Btn::Down | Btn::Left | Btn::Right => focused
                    .map(|p| vec![Action::FocusDirection { pane_id: p.pane_id.clone(), dir: dir_of(b) }])
                    .unwrap_or_default(),
                Btn::A => match agent_pane {
                    Some(_) => {
                        // Entering on a blocked agent is the acknowledgement; the next A confirms.
                        self.enter_respond(agent_pane, blocked);
                        vec![toast("Respond mode", "D-pad moves, A enters, B escapes, View leaves", Sound::None)]
                    }
                    None => vec![toast("No agent here", "focus a pane with an agent first", Sound::None)],
                },
                _ => vec![],
            },
            Mode::Respond => self.respond(b, snap),
        }
    }

    fn enter_respond(&mut self, agent_pane: Option<&Pane>, acknowledged: bool) {
        self.mode = Mode::Respond;
        self.respond_pane = agent_pane.map(|p| p.pane_id.clone());
        self.acknowledged_block = acknowledged;
    }

    fn enter_navigate(&mut self) {
        self.mode = Mode::Navigate;
        self.respond_pane = None;
        self.acknowledged_block = false;
    }

    fn leave_respond(&mut self, actions: Vec<Action>) -> Vec<Action> {
        if self.mode != Mode::Respond { return actions; }
        self.enter_navigate();
        let mut out = vec![toast("Navigate mode", "", Sound::None)];
        out.extend(actions);
        out
    }

    fn respond(&mut self, b: Btn, snap: &Snapshot) -> Vec<Action> {
        let focused = snap.focused.as_ref();
        // Respond belongs to the pane it was entered on: a press after the focus moved must not answer
        // whatever dialog the newly focused agent shows.
        if focused.map(|p| &p.pane_id) != self.respond_pane.as_ref() {
            self.enter_navigate();
            return vec![toast("Navigate mode", "focus moved to another pane", Sound::None)];
        }
        if b == Btn::View {
            self.enter_navigate();
            return vec![toast("Navigate mode", "", Sound::None)];
        }
        let Some(p) = focused.filter(|p| p.agent.is_some()) else {
            self.enter_navigate();
            return vec![toast("Navigate mode", "no agent in the focused pane", Sound::None)];
        };
        let key = match b {
            Btn::Up => Key::Up, Btn::Down => Key::Down, Btn::Left => Key::Left, Btn::Right => Key::Right,
            Btn::A => Key::Enter, Btn::B => Key::Esc,
            _ => return vec![],
        };
        // Every dialog needs two presses: the first key pressed while the agent is Blocked acknowledges it, and
        // only an acknowledged dialog can be answered with Enter. A status change (the dialog was answered, the
        // agent worked on) resets the acknowledgement so the next dialog needs its own. Accepted limit: a
        // multi-step dialog that stays Blocked throughout counts as one, so each later A sends Enter.
        if p.status != Some(Status::Blocked) {
            self.acknowledged_block = false;
        } else if !self.acknowledged_block {
            self.acknowledged_block = true;
            if key == Key::Enter { return vec![toast("The agent is waiting", "press A again to confirm", Sound::Request)]; }
        }
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

/// The first blocked agent after the focused pane in sidebar order (space number, then Herdr's
/// listing order), wrapping; failing that the first done one; failing that a toast.
fn next_attention(snap: &Snapshot) -> Vec<Action> {
    let number = |ws: &str| snap.workspaces.iter().find(|w| w.id == ws).map_or(u32::MAX, |w| w.number);
    let mut agents: Vec<&Agent> = snap.agents.iter().collect();
    agents.sort_by_key(|a| number(&a.workspace_id));
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

/// A transcript goes to the pane that was focused when the user spoke (`target`), and only if it is still
/// focused; where it goes in that pane depends on what its agent is doing.
fn transcript(text: String, target: Option<String>, snap: &Snapshot) -> Vec<Action> {
    let text = text.trim().to_string();
    if text.is_empty() { return vec![toast("Heard nothing", "", Sound::None)]; }
    let short = preview(&text);
    let Some(target) = target else {
        return vec![toast("Not sent", &format!("nothing was focused when you spoke: {short}"), Sound::Request)];
    };
    let Some(p) = snap.focused.as_ref().filter(|p| p.pane_id == target) else {
        return vec![toast("Not sent", &format!("focus moved since you spoke: {short}"), Sound::Request)];
    };
    if p.agent.is_none() {
        return vec![toast("Not sent", &format!("no agent in the focused pane: {short}"), Sound::Request)];
    }
    match p.status {
        Some(Status::Idle) | Some(Status::Done) => vec![
            Action::Prompt { target: p.pane_id.clone(), text },
            toast("Sent", &short, Sound::None),
        ],
        Some(Status::Working) => vec![
            Action::SendText { pane_id: p.pane_id.clone(), text },
            toast("Typed, not sent", &format!("agent is working: {short}"), Sound::None),
        ],
        Some(Status::Blocked) => vec![toast("Not sent", &format!("agent is waiting at a dialog: {short}"), Sound::Request)],
        _ => vec![toast("Not sent", &format!("agent state unknown: {short}"), Sound::Request)],
    }
}

fn preview(text: &str) -> String {
    let mut s: String = text.chars().take(60).collect();
    if text.chars().count() > 60 { s.push('…'); }
    s
}
