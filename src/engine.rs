//! The pure state machine: a controller input plus a snapshot of Herdr give the actions to run.
//! Nothing here touches hardware or Herdr.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Btn { A, B, X, Y, Lb, Rb, Lt, Rt, View, Menu, Up, Down, Left, Right, LStick, RStick }

impl Btn {
    /// Physical labels remain meaningful regardless of the lettering on a controller.
    pub fn label(self) -> &'static str {
        match self {
            Btn::A => "bottom face button", Btn::B => "right face button",
            Btn::X => "left face button", Btn::Y => "top face button",
            Btn::Lb => "left shoulder", Btn::Rb => "right shoulder",
            Btn::Lt => "left trigger", Btn::Rt => "right trigger",
            Btn::View => "Back / Select", Btn::Menu => "Menu / Options",
            Btn::Up => "D-pad up", Btn::Down => "D-pad down",
            Btn::Left => "D-pad left", Btn::Right => "D-pad right",
            Btn::LStick => "left stick click", Btn::RStick => "right stick click",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Input { ScrollAxis(i8), Press(Btn), Release(Btn), Connected, Disconnected, Transcript(String), TranscriptFailed(String) }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode { Navigate, Respond, Interact }

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
pub enum Key { Up, Down, Left, Right, Enter, Esc, Space }

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
    ClosePane(String),
    CloseWorkspace(String),
    SendKeys { target: String, keys: Vec<Key> },
    SendPaneKeys { target: String, keys: Vec<Key> },
    Prompt { target: String, text: String },
    InsertText { target: Pane, text: String },
    SubmitText { target: Pane },
    ClearInput { target: Pane },
    Toast { title: String, body: String, sound: Sound },
    StartRecording,
    StopRecording,
    CancelRecording,
}

pub fn toast(title: &str, body: &str, sound: Sound) -> Action {
    Action::Toast { title: title.into(), body: body.into(), sound }
}

/// The action menu never takes the configured push-to-talk button.
pub fn menu_button(voice_button: Btn) -> Btn {
    if voice_button == Btn::Menu { Btn::LStick } else { Btn::Menu }
}

pub struct Core {
    mode: Mode,
    voice_button: Btn,
    /// The pane Respond was entered on; a press with another pane focused leaves Respond instead of answering.
    respond_pane: Option<String>,
    respond_target: Option<Pane>,
    /// Explicitly selected non-agent pane; an agent appearing invalidates this mode.
    interact_pane: Option<String>,
    recording: bool,
    /// A transcription is running; its result decides `voice_target`'s fate.
    transcribing: bool,
    /// The pane focused when the voice button was released: the only pane the transcript may go to.
    voice_target: Option<String>,
    voice_pane: Option<Pane>,
    preview_enabled: bool,
    recording_preview: bool,
    pending_insert: Option<Pane>,
    staged: Option<Pane>,
    submit_held: bool,
    clear_held: bool,
    pending_submit: Option<Pane>,
    fresh_insert: bool,
}

impl Core {
    pub fn new(voice_button: Btn) -> Self {
        Core { mode: Mode::Navigate, voice_button, respond_pane: None, respond_target: None, interact_pane: None, recording: false, transcribing: false, voice_target: None, voice_pane: None, preview_enabled: false, recording_preview: false, pending_insert: None, staged: None, submit_held: false, clear_held: false, pending_submit: None, fresh_insert: false }
    }
    pub fn begin_tick(&mut self) {self.fresh_insert=false;}
    pub fn submission_result(&mut self, target: Pane, refusal: Option<String>) -> Vec<Action> {
        if self.pending_submit.as_ref() != Some(&target) {return vec![];}
        self.pending_submit=None;
        if let Some(error)=refusal {
            self.staged=Some(target);
            return vec![toast("Voice text not sent", &error, Sound::Request)];
        }
        vec![]
    }
    fn disarm_native_submit(&mut self) {self.staged=None;self.pending_submit=None;self.fresh_insert=false;}
    pub fn insertion_result(&mut self, target: Pane, result: Result<(), String>) -> Vec<Action> {
        if self.pending_insert.as_ref() != Some(&target) { return vec![]; }
        self.pending_insert=None;
        match result {
            Ok(()) => {
                self.staged=Some(target);
                self.fresh_insert=true;
                self.enter_navigate();
                vec![toast("Voice text inserted", "Review in the agent input; Cross sends; Square clears line", Sound::None)]
            }
            Err(error) => vec![toast("Voice insertion unconfirmed", &error, Sound::Request)],
        }
    }
    pub fn set_preview_enabled(&mut self, enabled: bool) {
        self.preview_enabled=enabled;
        if !enabled { self.disarm_native_submit(); self.pending_insert=None; }
    }
    pub fn preview_in_progress(&self) -> bool { self.recording_preview }
    pub fn mode(&self) -> Mode { self.mode }
    pub fn voice_busy(&self)->bool {self.recording||self.transcribing}
    pub fn snapshot_failed(&mut self){self.enter_navigate();self.submit_held=false;self.clear_held=false;}
    // Menus own button releases while open, so they must also release Core's submit latch.
    pub fn menu_exit(&mut self){self.enter_navigate();self.submit_held=false;self.clear_held=false;self.disarm_native_submit();}
    pub fn rebind(&mut self,button:Btn){self.voice_button=button;}
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn recording(&self) -> bool { self.recording }

    pub fn handle(&mut self, input: Input, snap: &Snapshot) -> Vec<Action> {
        match input {
            Input::ScrollAxis(_) => vec![],
            Input::Press(b) if b == self.voice_button => {
                if self.recording { return vec![]; }
                if self.transcribing { return vec![toast("Still transcribing", "wait for the previous one", Sound::None)]; }
                self.recording_preview=self.preview_enabled;
                self.recording = true;
                vec![Action::StartRecording]
            }
            Input::Release(b) if b == self.voice_button => {
                if !self.recording { return vec![]; }
                self.recording = false;
                self.transcribing = true;
                self.voice_target = snap.focused.as_ref().map(|p| p.pane_id.clone());
                self.voice_pane = snap.focused.clone();
                vec![Action::StopRecording]
            }
            Input::Press(b) => self.press(b, snap),
            Input::Release(Btn::A) => { self.submit_held=false; vec![] },
            Input::Release(Btn::X) => { self.clear_held=false; vec![] },
            Input::Release(_) => vec![],
            Input::Connected => vec![toast("Controller connected", "", Sound::None)],
            Input::Disconnected => {
                self.submit_held=false;
                self.clear_held=false;
                self.disarm_native_submit();
                // A disconnected controller leaves every interaction mode.
                self.enter_navigate();
                if !self.recording { return vec![toast("Controller disconnected", "", Sound::None)]; }
                self.recording = false;
                vec![Action::CancelRecording, toast("Controller disconnected", "recording discarded", Sound::Request)]
            }
            Input::Transcript(text) => {
                self.transcribing = false;
                let target = self.voice_target.take();
                let pane = self.voice_pane.take();
                if std::mem::take(&mut self.recording_preview) {
                    let text=crate::dictation::native_text(&text);
                    if text.is_empty() { return vec![toast("Heard nothing", "", Sound::None)]; }
                    let Some(pane)=pane else { return vec![toast("Voice text not inserted", "No agent was selected when recorded", Sound::Request)]; };
                    if let Err(error)=crate::dictation::validate_target(&pane,snap) {
                        return vec![toast("Voice text not inserted", &error, Sound::Request)];
                    }
                    self.pending_insert=Some(pane.clone());
                    vec![Action::InsertText {target:pane,text}]
                } else { transcript(text, target, snap) }
            }
            Input::TranscriptFailed(e) => {
                self.recording = false;
                self.transcribing = false;
                self.voice_target = None;
                self.voice_pane = None;
                self.recording_preview=false;
                vec![toast("Transcription failed", &e, Sound::Request)]
            }
        }
    }

    fn clear_input(&mut self, target: Pane, snap: &Snapshot) -> Vec<Action> {
        self.clear_held=true;
        if self.fresh_insert {return vec![];}
        if self.voice_busy() {return vec![toast("Still recording or transcribing", "Wait before clearing", Sound::None)];}
        if let Err(error)=crate::dictation::validate_clear_target(&target,snap) {
            return vec![toast("Input not cleared", &error, Sound::Request)];
        }
        self.disarm_native_submit();
        self.enter_navigate();
        vec![Action::ClearInput {target}]
    }

    fn press(&mut self, b: Btn, snap: &Snapshot) -> Vec<Action> {
        if b == Btn::X {
            if self.clear_held {return vec![];}
            if let Some(target)=self.staged.clone() {return self.clear_input(target,snap);}
        }
        if b == Btn::A {
            if self.submit_held { return vec![]; }
            if let Some(target)=self.staged.as_ref() {
                self.submit_held=true;
                if self.fresh_insert {return vec![];}
                if self.voice_busy() { return vec![toast("Still recording or transcribing", "Wait before sending", Sound::None)]; }
                if let Err(error)=crate::dictation::validate_target(target,snap) {
                    return vec![toast("Voice text not sent", &error, Sound::Request)];
                }
                let target=self.staged.take().expect("staged target checked above");
                self.pending_submit=Some(target.clone());
                self.enter_navigate();
                return vec![Action::SubmitText {target}];
            }
        }
        if b == Btn::B && self.staged.take().is_some() {
            self.enter_navigate();
            return vec![toast("Navigate mode", "Text remains in the agent input", Sound::None)];
        }
        if matches!(b, Btn::Lb | Btn::Rb | Btn::Lt | Btn::Rt | Btn::Y)
            || matches!(b, Btn::Up | Btn::Down | Btn::Left | Btn::Right) {
            self.disarm_native_submit();
        }
        let focused = snap.focused.as_ref();
        if self.mode == Mode::Interact && !focused.is_some_and(|p| {
            Some(&p.pane_id) == self.interact_pane.as_ref() && p.agent.is_none()
        }) {
            self.enter_navigate();
            return vec![toast("Navigate mode", "pane changed", Sound::None)];
        }
        let agent_pane = focused.filter(|p| p.agent.is_some());
        // Global navigation leaves either pane interaction mode.
        let nav = match b {
            Btn::Lb => Some(step_workspace(snap, -1)),
            Btn::Rb => Some(step_workspace(snap, 1)),
            Btn::Lt => Some(step_tab(snap, -1)),
            Btn::Rt => Some(step_tab(snap, 1)),
            Btn::Y => Some(next_attention(snap)),
            Btn::X if self.mode == Mode::Navigate => Some(focused.map(|p| vec![Action::ToggleZoom(p.pane_id.clone())]).unwrap_or_default()),
            _ => None,
        };
        if let Some(actions) = nav { return self.leave_respond(actions); }
        let blocked = agent_pane.is_some_and(|p| p.status == Some(Status::Blocked));
        match self.mode {
            Mode::Navigate => match b {
                Btn::Up | Btn::Down | Btn::Left | Btn::Right => focused
                    .map(|p| vec![Action::FocusDirection { pane_id: p.pane_id.clone(), dir: dir_of(b) }])
                    .unwrap_or_default(),
                Btn::A => match agent_pane {
                    Some(p) => {
                        self.enter_respond(p);
                        let mut out = vec![toast("Respond mode", "D-pad moves; Cross confirms; Square clears input line (Codex/Claude); Circle returns to Navigate; Create / Share escapes", Sound::None)];
                        if blocked {
                            out.push(Action::SendKeys { target: p.pane_id.clone(), keys: vec![Key::Enter] });
                        }
                        out
                    }
                    None => match focused.filter(|p| !p.pane_id.is_empty()) {
                        Some(p) => {
                            self.mode = Mode::Interact;
                            self.interact_pane = Some(p.pane_id.clone());
                            vec![toast("Interact mode", "D-pad moves; Cross enters; Square selects; Circle returns to Navigate; Create / Share escapes", Sound::None)]
                        }
                        None => vec![toast("No pane here", "focus a pane first", Sound::None)],
                    },
                },
                _ => vec![],
            },
            Mode::Respond => self.respond(b, snap),
            Mode::Interact => {
                if b == Btn::B {
                    self.enter_navigate();
                    return vec![toast("Navigate mode", "", Sound::None)];
                }
                let key = if b == Btn::X { Some(Key::Space) } else { key_for(b) };
                key.map(|key| vec![Action::SendPaneKeys {
                    target: self.interact_pane.clone().expect("Interact has a validated target"),
                    keys: vec![key],
                }]).unwrap_or_default()
            }
        }
    }

    fn enter_respond(&mut self, pane: &Pane) {
        self.mode = Mode::Respond;
        self.respond_pane = Some(pane.pane_id.clone());
        self.respond_target = Some(pane.clone());
    }

    fn enter_navigate(&mut self) {
        self.mode = Mode::Navigate;
        self.respond_pane = None;
        self.respond_target = None;
        self.interact_pane = None;
    }

    fn leave_respond(&mut self, actions: Vec<Action>) -> Vec<Action> {
        if self.mode == Mode::Navigate { return actions; }
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
        if b == Btn::B {
            self.enter_navigate();
            return vec![toast("Navigate mode", "", Sound::None)];
        }
        let Some(p) = focused.filter(|p| p.agent.is_some()) else {
            self.enter_navigate();
            return vec![toast("Navigate mode", "no agent in the focused pane", Sound::None)];
        };
        if b == Btn::X {
            return self.clear_input(self.respond_target.clone().expect("Respond captures its agent target"),snap);
        }
        let Some(key) = key_for(b) else { return vec![] };
        vec![Action::SendKeys { target: p.pane_id.clone(), keys: vec![key] }]
    }
}

/// The key a button sends in Respond; None for the buttons that send nothing.
fn key_for(b: Btn) -> Option<Key> {
    Some(match b {
        Btn::Up => Key::Up, Btn::Down => Key::Down, Btn::Left => Key::Left, Btn::Right => Key::Right,
        Btn::A => Key::Enter, Btn::View => Key::Esc,
        _ => return None,
    })
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
        Some(Status::Idle) | Some(Status::Done) | Some(Status::Working) => vec![
            Action::Prompt { target: p.pane_id.clone(), text },
            toast("Sent", &short, Sound::None),
        ],
        Some(Status::Blocked) => vec![toast("Not sent", &format!("agent is waiting at a dialog: {short}"), Sound::Request)],
        _ => vec![toast("Not sent", &format!("agent state unknown: {short}"), Sound::Request)],
    }
}

pub fn preview(text: &str) -> String {
    let mut s: String = text.chars().take(60).collect();
    if text.chars().count() > 60 { s.push('…'); }
    s
}
