//! Pure controller menu. Effects contain frozen targets; this module performs no I/O.
use crate::engine::Btn;
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LabelStyle {
    #[default]
    Both,
    PlayStation,
    Xbox,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Context {
    pub workspace: String,
    pub tab: String,
    pub pane: String,
    pub workspace_label: String,
    pub pane_label: String,
    pub cwd: Option<String>,
}
impl Context {
    pub fn same_identity(&self, other: &Self) -> bool {
        self.workspace == other.workspace && self.tab == other.tab && self.pane == other.pane
    }
    pub fn directory(&self) -> Option<&str> {
        self.cwd
            .as_deref()
            .filter(|s| !s.is_empty() && Path::new(s).is_absolute())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsSnapshot {
    pub labels: LabelStyle,
    pub voice_button: Btn,
    pub max_seconds: u64,
    pub model: String,
    pub language: String,
    pub config_path: String,
    pub version: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RequestId {
    pub session: u64,
    pub serial: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadKind {
    Repository { workspace: String },
    Logs,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadRequest {
    pub id: RequestId,
    pub kind: ReadKind,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worktree {
    pub path: String,
    pub branch: Option<String>,
    pub workspace: Option<String>,
    pub primary: bool,
    pub bare: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repository {
    pub repo_key: String,
    pub name: String,
    pub root: String,
    pub source_checkout: String,
    pub base_commit: String,
    pub entries: Vec<Worktree>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReadData {
    Repository(Repository),
    Logs(Vec<String>),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SplitDirection {
    Right,
    Down,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    CreateSpace {
        name: String,
        cwd: String,
    },
    Split {
        direction: SplitDirection,
        cwd: String,
    },
    ToggleZoom,
    RenamePane {
        label: Option<String>,
    },
    ClosePane,
    CloseWorkspace,
    CreateWorktree {
        branch: String,
        base: String,
        repository: String,
    },
    OpenWorktree {
        path: String,
        workspace: Option<String>,
    },
    RemoveWorktree {
        path: String,
        workspace: String,
    },
    SaveSettings {
        original: SettingsSnapshot,
        updated: SettingsSnapshot,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Operation {
    pub expected_repository: Option<String>,
    pub id: RequestId,
    pub context: Option<Context>,
    pub command: Command,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    Read(ReadRequest),
    Mutate(Operation),
    Closed,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Success(String),
    Failed(String),
    Unknown(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Response {
    Read {
        id: RequestId,
        result: Result<ReadData, String>,
    },
    Mutation {
        id: RequestId,
        outcome: Outcome,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OperationState {
    Pending,
    Unknown(String),
    Finished(Outcome),
}
/// Keep this slot in the daemon, not in the popup. Unknown outcomes continue to
/// block mutation until a read-only reconciliation establishes an outcome.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OperationSlot {
    current: Option<(Operation, OperationState)>,
    seen: Vec<RequestId>,
}
impl OperationSlot {
    pub fn reserve(&mut self, op: Operation) -> bool {
        if self.seen.contains(&op.id)
            || self
                .current
                .as_ref()
                .is_some_and(|(_, s)| !matches!(s, OperationState::Finished(_)))
        {
            return false;
        }
        self.seen.push(op.id);
        self.current = Some((op, OperationState::Pending));
        true
    }
    pub fn complete(&mut self, id: RequestId, outcome: Outcome) -> bool {
        match &mut self.current {
            Some((op, state)) if op.id == id && *state == OperationState::Pending => {
                *state = match outcome {
                    Outcome::Unknown(s) => OperationState::Unknown(s),
                    other => OperationState::Finished(other),
                };
                true
            }
            _ => false,
        }
    }
    pub fn reconcile(&mut self, id: RequestId, outcome: Outcome) -> bool {
        match &mut self.current {
            Some((op, state))
                if op.id == id
                    && matches!(state, OperationState::Unknown(_))
                    && !matches!(outcome, Outcome::Unknown(_)) =>
            {
                *state = OperationState::Finished(outcome);
                true
            }
            _ => false,
        }
    }
    pub fn state(&self) -> Option<&OperationState> {
        self.current.as_ref().map(|(_, s)| s)
    }
    pub fn operation(&self) -> Option<&Operation> {
        self.current.as_ref().map(|(op, _)| op)
    }
    pub fn blocked(&self) -> bool {
        self.state()
            .is_some_and(|s| !matches!(s, OperationState::Finished(_)))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Up,
    Down,
    Left,
    Right,
    Activate,
    Back,
    Menu,
    View,
    Delete,
    Page,
    Details,
    Release(ActionKey),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKey {
    Activate,
    Right,
    Delete,
    Page,
}
impl Input {
    pub fn action_key(self) -> Option<ActionKey> {
        match self {
            Self::Activate => Some(ActionKey::Activate),
            Self::Right => Some(ActionKey::Right),
            Self::Delete => Some(ActionKey::Delete),
            Self::Page => Some(ActionKey::Page),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Name,
    Branch,
    Label,
}
pub fn validate(field: Field, value: &str) -> Result<(), String> {
    let limit = if field == Field::Branch { 200 } else { 80 };
    if value.trim().is_empty() {
        return Err("Enter a nonempty name".into());
    }
    if value.chars().count() > limit {
        return Err(format!("Maximum {limit} characters"));
    }
    if value.chars().any(char::is_control) {
        return Err("Control characters are not allowed".into());
    }
    if field == Field::Branch
        && (value.starts_with('-')
            || value.contains(' ')
            || value.contains("..")
            || value.contains("@{")
            || value.ends_with('/')
            || value.ends_with('.')
            || value.starts_with('/')
            || value.contains("//")
            || value.chars().any(|c| "~^:?*[\\".contains(c)))
    {
        return Err("Invalid branch name; Git also validates before creation".into());
    }
    Ok(())
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditResult {
    Continue,
    Done(String),
    Cancelled,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextEditor {
    pub field: Field,
    pub buffer: String,
    pub selected: usize,
    pub page: usize,
    pub error: String,
}
impl TextEditor {
    pub fn new(field: Field, buffer: String) -> Self {
        Self {
            field,
            buffer,
            selected: 0,
            page: 0,
            error: String::new(),
        }
    }
    pub fn cells(&self) -> Vec<String> {
        let alphabet = match self.page {
            0 => "abcdefghijklmnopqrstuvwxyz0123456789 /-_",
            1 => "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 /-_",
            _ => "0123456789 /-_.@+()[]{}=,!#$%&;",
        };
        let mut cells = alphabet.chars().map(|c| c.to_string()).collect::<Vec<_>>();
        cells.push("Done".into());
        cells
    }
    pub fn input(&mut self, input: Input) -> EditResult {
        let count = self.cells().len();
        match input {
            Input::Up => self.selected = self.selected.saturating_sub(8),
            Input::Down => self.selected = (self.selected + 8).min(count - 1),
            Input::Left => self.selected = self.selected.saturating_sub(1),
            Input::Right => self.selected = (self.selected + 1).min(count - 1),
            Input::Delete => {
                self.buffer.pop();
                self.error.clear();
            }
            Input::Page => {
                self.page = (self.page + 1) % 3;
                self.selected = self.selected.min(self.cells().len() - 1);
            }
            Input::Back => return EditResult::Cancelled,
            Input::Activate => {
                if self.selected == count - 1 {
                    match validate(self.field, &self.buffer) {
                        Ok(()) => return EditResult::Done(self.buffer.clone()),
                        Err(e) => self.error = e,
                    }
                } else {
                    let limit = if self.field == Field::Branch { 200 } else { 80 };
                    if self.buffer.chars().count() < limit {
                        self.buffer.push_str(&self.cells()[self.selected]);
                        self.error.clear();
                    } else {
                        self.error = format!("Maximum {limit} characters");
                    }
                }
            }
            _ => {}
        };
        EditResult::Continue
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub label: String,
    pub disabled: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct View {
    pub revision: u64,
    pub scope: String,
    pub breadcrumb: String,
    pub rows: Vec<Row>,
    pub selected: usize,
    pub details: Vec<String>,
    pub status: String,
    pub scroll: usize,
    pub expanded: bool,
    pub labels: LabelStyle,
    pub editor: Option<TextEditor>,
    pub menu_button: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum Screen {
    Main,
    NewSpace,
    Worktrees,
    CreateWorktree,
    OpenWorktree,
    RemoveWorktree,
    Pane,
    Close,
    Settings,
    VoiceConfig,
    Logs,
    Controls,
    Confirm { command: Command, target: String },
    Edit(Field),
}
impl Screen {
    fn title(&self) -> &str {
        match self {
            Self::Main => "Main",
            Self::NewSpace => "New space",
            Self::Worktrees => "Worktrees",
            Self::CreateWorktree => "Create worktree",
            Self::OpenWorktree => "Open existing",
            Self::RemoveWorktree => "Remove worktree",
            Self::Pane => "Pane actions",
            Self::Close => "Close…",
            Self::Settings => "Settings",
            Self::VoiceConfig => "Voice configuration",
            Self::Logs => "Logs",
            Self::Controls => "Controls",
            Self::Confirm { .. } => "Confirm",
            Self::Edit(_) => "Text entry",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Frame {
    screen: Screen,
    selected: usize,
    scroll: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    session: u64,
    serial: u64,
    revision: u64,
    context: Option<Context>,
    stack: Vec<Frame>,
    open: bool,
    settings: SettingsSnapshot,
    active_settings: SettingsSnapshot,
    draft: SettingsSnapshot,
    space_name: String,
    branch: String,
    pane_label: String,
    repository: Option<Repository>,
    pinned_repository: Option<String>,
    logs: Vec<String>,
    read: Option<ReadRequest>,
    pending: Option<Operation>,
    status: String,
    editor: Option<TextEditor>,
    expanded: bool,
    detail_scroll: usize,
    detail_width: usize,
    held: Vec<ActionKey>,
    external_busy: bool,
    external_operation: Option<(Operation, OperationState)>,
}
impl Menu {
    pub fn new(
        session: u64,
        context: Option<Context>,
        settings: SettingsSnapshot,
        space_name: String,
        branch: String,
    ) -> Self {
        let pane_label = context
            .as_ref()
            .map(|c| c.pane_label.clone())
            .unwrap_or_default();
        Self {
            session,
            serial: 0,
            revision: 1,
            context,
            stack: vec![Frame {
                screen: Screen::Main,
                selected: 0,
                scroll: 0,
            }],
            open: true,
            draft: settings.clone(),
            active_settings: settings.clone(),
            settings,
            space_name,
            branch,
            pane_label,
            repository: None,
            pinned_repository: None,
            logs: vec![],
            read: None,
            pending: None,
            status: String::new(),
            editor: None,
            expanded: false,
            detail_scroll: 0,
            detail_width: 64,
            held: vec![],
            external_busy: false,
            external_operation: None,
        }
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn is_open(&self) -> bool {
        self.open
    }
    pub fn context(&self) -> Option<&Context> {
        self.context.as_ref()
    }
    pub fn pending(&self) -> Option<&Operation> {
        self.pending.as_ref()
    }
    pub fn settings(&self) -> &SettingsSnapshot {
        &self.settings
    }
    /// Set the popup's actual content width before input, including on resize.
    pub fn set_detail_width(&mut self, width: usize) {
        let width = width.max(1);
        if self.detail_width != width {
            self.detail_width = width;
            let max = crate::menu_render::detail_lines(&self.view(), width)
                .len()
                .saturating_sub(1);
            self.detail_scroll = self.detail_scroll.min(max);
            self.revision += 1;
        }
    }
    /// Synchronize the daemon-owned slot even when this popup did not submit it.
    pub fn sync_operation_slot(&mut self, slot: &OperationSlot) {
        let external = if slot.blocked() {
            slot.operation()
                .zip(slot.state())
                .map(|(op, state)| (op.clone(), state.clone()))
        } else {
            None
        };
        if self.external_operation != external || self.external_busy != slot.blocked() {
            self.external_operation = external;
            self.external_busy = slot.blocked();
            self.revision += 1;
        }
    }
    pub fn set_external_busy(&mut self, busy: bool) {
        if self.external_busy != busy {
            self.external_busy = busy;
            self.revision += 1;
        }
    }
    pub fn observe_context(&mut self, result: Result<Option<Context>, String>) -> bool {
        let valid = match (&self.context, result) {
            (None, Ok(None)) => true,
            (Some(a), Ok(Some(b))) => a.same_identity(&b),
            _ => false,
        };
        if !valid && self.open {
            self.dismiss();
            self.revision += 1;
            return true;
        }
        false
    }
    fn frame(&self) -> &Frame {
        self.stack.last().expect("root frame")
    }
    fn frame_mut(&mut self) -> &mut Frame {
        self.stack.last_mut().expect("root frame")
    }
    fn next_id(&mut self) -> RequestId {
        self.serial += 1;
        RequestId {
            session: self.session,
            serial: self.serial,
        }
    }
    fn dismiss(&mut self) {
        self.open = false;
        self.read = None;
        self.draft = self.settings.clone();
        self.editor = None;
    }
    fn push(&mut self, screen: Screen) {
        self.stack.push(Frame {
            screen,
            selected: 0,
            scroll: 0,
        });
        self.status.clear();
        self.expanded = false;
        self.detail_scroll = 0;
    }
    fn back(&mut self) -> Vec<Effect> {
        if self.expanded {
            self.expanded = false;
            self.detail_scroll = 0;
            return vec![];
        }
        self.read = None;
        self.editor = None;
        self.status.clear();
        if self.stack.len() == 1 {
            self.dismiss();
            vec![Effect::Closed]
        } else {
            self.stack.pop();
            vec![]
        }
    }
    fn start_read(&mut self, kind: ReadKind) -> Vec<Effect> {
        if matches!(kind, ReadKind::Repository { .. }) {
            self.repository = None;
        }
        let r = ReadRequest {
            id: self.next_id(),
            kind,
        };
        self.read = Some(r.clone());
        self.status = "Loading…".into();
        vec![Effect::Read(r)]
    }
    fn mutate(&mut self, command: Command) -> Vec<Effect> {
        if self.pending.is_some() || self.external_busy {
            self.status = "Another operation is pending; wait for its outcome".into();
            return vec![];
        }
        let context = if matches!(command, Command::SaveSettings { .. }) {
            None
        } else {
            self.context.clone()
        };
        if context.is_none() && !matches!(command, Command::SaveSettings { .. }) {
            self.status = "No captured context".into();
            return vec![];
        }
        let operation = Operation {
            expected_repository: if matches!(
                command,
                Command::CreateWorktree { .. }
                    | Command::OpenWorktree { .. }
                    | Command::RemoveWorktree { .. }
            ) {
                self.pinned_repository.clone()
            } else {
                None
            },
            id: self.next_id(),
            context,
            command,
        };
        self.pending = Some(operation.clone());
        self.status = "Pending — waiting for operation outcome".into();
        vec![Effect::Mutate(operation)]
    }
    pub fn input(&mut self, input: Input, acknowledged_revision: u64) -> Vec<Effect> {
        if !self.open {
            return vec![];
        }
        if let Input::Release(key) = input {
            self.held.retain(|k| *k != key);
            return vec![];
        }
        let key = input.action_key();
        let gated = key.is_some();
        // Held-button state is not rendered and must not invalidate its own ack.
        if gated
            && (key.is_some_and(|key| self.held.contains(&key))
                || acknowledged_revision != self.revision)
        {
            return vec![];
        }
        if let Some(key) = key {
            self.held.push(key);
        }
        let before = self.clone();
        let effects = self.handle(input);
        if *self != before {
            self.revision += 1;
        }
        effects
    }
    fn handle(&mut self, input: Input) -> Vec<Effect> {
        if matches!(input, Input::Menu | Input::View) {
            self.dismiss();
            return vec![Effect::Closed];
        }
        if self.editor.is_some() {
            let result = self.editor.as_mut().unwrap().input(input);
            match result {
                EditResult::Done(text) => {
                    match self.editor.as_ref().unwrap().field {
                        Field::Name => self.space_name = text,
                        Field::Branch => self.branch = text,
                        Field::Label => self.pane_label = text,
                    };
                    self.editor = None;
                    self.stack.pop();
                }
                EditResult::Cancelled => {
                    self.editor = None;
                    self.stack.pop();
                }
                EditResult::Continue => {}
            };
            return vec![];
        }
        if matches!(input, Input::Back | Input::Left) {
            return self.back();
        }
        if input == Input::Details || input == Input::Delete {
            self.expanded = !self.expanded;
            self.detail_scroll = 0;
            return vec![];
        }
        if matches!(input, Input::Up | Input::Down) {
            if self.expanded {
                let max = crate::menu_render::detail_lines(&self.view(), self.detail_width)
                    .len()
                    .saturating_sub(1);
                self.detail_scroll = if input == Input::Up {
                    self.detail_scroll.saturating_sub(1)
                } else {
                    (self.detail_scroll + 1).min(max)
                };
            } else if matches!(
                self.frame().screen,
                Screen::Logs | Screen::Controls | Screen::VoiceConfig
            ) {
                let max = self.view().details.len().saturating_sub(1);
                let f = self.frame_mut();
                f.scroll = if input == Input::Up {
                    f.scroll.saturating_sub(1)
                } else {
                    (f.scroll + 1).min(max)
                };
            } else {
                let max = self.view().rows.len().saturating_sub(1);
                let f = self.frame_mut();
                f.selected = if input == Input::Up {
                    f.selected.saturating_sub(1)
                } else {
                    (f.selected + 1).min(max)
                };
            }
            return vec![];
        }
        if !matches!(input, Input::Activate | Input::Right) || self.expanded {
            return vec![];
        }
        let view = self.view();
        let Some(row) = view.rows.get(view.selected) else {
            return vec![];
        };
        if let Some(reason) = &row.disabled {
            self.status = reason.clone();
            return vec![];
        }
        let submenu = matches!(
            self.frame().screen,
            Screen::Main | Screen::Worktrees | Screen::Close
        ) || (self.frame().screen == Screen::Pane && matches!(view.selected, 3 | 5))
            || (self.frame().screen == Screen::Settings && view.selected == 3);
        if input == Input::Right && !submenu {
            return vec![];
        }
        self.activate()
    }
    fn edit(&mut self, field: Field) {
        let text = match field {
            Field::Name => self.space_name.clone(),
            Field::Branch => self.branch.clone(),
            Field::Label => self.pane_label.clone(),
        };
        self.editor = Some(TextEditor::new(field, text));
        self.push(Screen::Edit(field));
    }
    fn confirm(&mut self, command: Command, target: String) {
        self.push(Screen::Confirm { command, target });
    }
    fn activate(&mut self) -> Vec<Effect> {
        let selected = self.frame().selected;
        match self.frame().screen.clone() {
            Screen::Main => match selected {
                0 => self.push(Screen::NewSpace),
                1 => {
                    self.push(Screen::Worktrees);
                    return self.start_read(ReadKind::Repository {
                        workspace: self.context.as_ref().unwrap().workspace.clone(),
                    });
                }
                2 => self.push(Screen::Pane),
                3 => self.push(Screen::Close),
                4 => {
                    self.draft = self.settings.clone();
                    self.push(Screen::Settings);
                }
                5 => {
                    self.push(Screen::Logs);
                    return self.start_read(ReadKind::Logs);
                }
                6 => self.push(Screen::Controls),
                _ => {}
            },
            Screen::NewSpace => {
                if selected == 0 {
                    self.edit(Field::Name)
                } else {
                    return self.mutate(Command::CreateSpace {
                        name: self.space_name.clone(),
                        cwd: self.context.as_ref().unwrap().directory().unwrap().into(),
                    });
                }
            }
            Screen::Pane => match selected {
                0 | 1 => {
                    return self.mutate(Command::Split {
                        direction: if selected == 0 {
                            SplitDirection::Right
                        } else {
                            SplitDirection::Down
                        },
                        cwd: self.context.as_ref().unwrap().directory().unwrap().into(),
                    })
                }
                2 => return self.mutate(Command::ToggleZoom),
                3 => self.edit(Field::Label),
                4 => return self.mutate(Command::RenamePane { label: None }),
                5 => self.confirm(Command::ClosePane, self.pane_target()),
                6 => {
                    return self.mutate(Command::RenamePane {
                        label: Some(self.pane_label.clone()),
                    })
                }
                _ => {}
            },
            Screen::Close => {
                if selected == 0 {
                    self.confirm(Command::ClosePane, self.pane_target())
                } else {
                    let c = self.context.as_ref().unwrap();
                    self.confirm(
                        Command::CloseWorkspace,
                        format!(
                            "{} [{}] — running processes stop; worktree remains",
                            c.workspace_label, c.workspace
                        ),
                    );
                }
            }
            Screen::Confirm { command, .. } => {
                if selected == 0 {
                    return self.back();
                } else {
                    return self.mutate(command);
                }
            }
            Screen::Worktrees => {
                self.push(match selected {
                    0 => Screen::CreateWorktree,
                    1 => Screen::OpenWorktree,
                    _ => Screen::RemoveWorktree,
                });
                if selected == 0 {
                    return self.start_read(ReadKind::Repository {
                        workspace: self.context.as_ref().unwrap().workspace.clone(),
                    });
                }
            }
            Screen::CreateWorktree => {
                if selected == 0 {
                    self.edit(Field::Branch)
                } else {
                    let r = self.repository.as_ref().unwrap();
                    return self.mutate(Command::CreateWorktree {
                        branch: self.branch.clone(),
                        base: r.base_commit.clone(),
                        repository: r.root.clone(),
                    });
                }
            }
            Screen::OpenWorktree => {
                let e = &self.repository.as_ref().unwrap().entries[selected];
                return self.mutate(Command::OpenWorktree {
                    path: e.path.clone(),
                    workspace: e.workspace.clone(),
                });
            }
            Screen::RemoveWorktree => {
                let e = &self.repository.as_ref().unwrap().entries[selected];
                self.confirm(Command::RemoveWorktree{path:e.path.clone(),workspace:e.workspace.clone().unwrap()},format!("Remove checkout {} — branch {} — workspace {}. Stops processes and deletes this checkout.",e.path,e.branch.as_deref().unwrap_or("detached"),e.workspace.as_deref().unwrap()));
            }
            Screen::Settings => match selected {
                0 => {
                    self.draft.labels = match self.draft.labels {
                        LabelStyle::Both => LabelStyle::PlayStation,
                        LabelStyle::PlayStation => LabelStyle::Xbox,
                        LabelStyle::Xbox => LabelStyle::Both,
                    }
                }
                1 => {
                    self.draft.voice_button = match self.draft.voice_button {
                        Btn::RStick => Btn::LStick,
                        Btn::LStick => Btn::Menu,
                        _ => Btn::RStick,
                    }
                }
                2 => {
                    let presets = [15, 30, 60, 120, 300];
                    self.draft.max_seconds = presets
                        .iter()
                        .position(|v| *v == self.draft.max_seconds)
                        .map(|i| presets[(i + 1) % presets.len()])
                        .unwrap_or(15);
                }
                3 => self.push(Screen::VoiceConfig),
                4 => {
                    return self.mutate(Command::SaveSettings {
                        original: self.settings.clone(),
                        updated: self.draft.clone(),
                    })
                }
                5 => return self.back(),
                _ => {}
            },
            Screen::Logs => return self.start_read(ReadKind::Logs),
            Screen::Controls | Screen::VoiceConfig | Screen::Edit(_) => {}
        };
        vec![]
    }
    fn pane_target(&self) -> String {
        let c = self.context.as_ref().unwrap();
        format!(
            "{} [{}] in {} [{}] — running processes stop",
            c.pane_label, c.pane, c.workspace_label, c.workspace
        )
    }
    /// Adapter calls only with the exact snapshot returned by atomic persistence.
    /// Its version is the new version, not the optimistic draft's old version.
    pub fn settings_saved(&mut self, id: RequestId, committed: SettingsSnapshot) -> bool {
        if !self
            .pending
            .as_ref()
            .is_some_and(|op| op.id == id && matches!(op.command, Command::SaveSettings { .. }))
        {
            return false;
        }
        self.settings = committed.clone();
        self.draft = committed;
        self.pending = None;
        self.status = "Settings saved; apply on menu exit".into();
        self.revision += 1;
        true
    }
    /// A detected external edit replaces the stale form; no write was performed.
    pub fn settings_conflict(&mut self, id: RequestId, current: SettingsSnapshot) -> bool {
        if !self
            .pending
            .as_ref()
            .is_some_and(|op| op.id == id && matches!(op.command, Command::SaveSettings { .. }))
        {
            return false;
        }
        self.settings = current.clone();
        self.draft = current;
        self.pending = None;
        self.status = "Configuration changed externally; reloaded. Review and retry.".into();
        self.revision += 1;
        true
    }
    pub fn receive(&mut self, response: Response) -> bool {
        let accepted = match response {
            Response::Read { id, result } => {
                if !self.open || self.read.as_ref().map(|r| r.id) != Some(id) {
                    return false;
                }
                match result {
                    Ok(ReadData::Repository(r))
                        if matches!(
                            self.read.as_ref().unwrap().kind,
                            ReadKind::Repository { .. }
                        ) =>
                    {
                        if self
                            .pinned_repository
                            .as_ref()
                            .is_some_and(|key| *key != r.repo_key)
                        {
                            self.repository = None;
                            self.status =
                                "Repository changed; close and reopen the menu to use it".into();
                        } else {
                            self.pinned_repository = Some(r.repo_key.clone());
                            self.repository = Some(r);
                            self.status.clear();
                        }
                    }
                    Ok(ReadData::Logs(lines))
                        if self.read.as_ref().unwrap().kind == ReadKind::Logs =>
                    {
                        self.logs = lines;
                        self.frame_mut().scroll = 0;
                        self.status.clear();
                    }
                    Err(e) => self.status = e,
                    _ => return false,
                };
                self.read = None;
                true
            }
            Response::Mutation { id, outcome } => {
                if self.pending.as_ref().map(|op| op.id) != Some(id) {
                    return false;
                }
                // A generic success lacks the newly committed configuration version.
                // The adapter must use settings_saved for a successful persistence.
                if matches!(outcome, Outcome::Success(_))
                    && matches!(
                        self.pending.as_ref().unwrap().command,
                        Command::SaveSettings { .. }
                    )
                {
                    return false;
                }
                self.status = match &outcome {
                    Outcome::Success(s) => format!("Success: {s}"),
                    Outcome::Failed(s) => format!("Failed: {s}"),
                    Outcome::Unknown(s) => {
                        format!("Outcome unknown: {s}. Refresh state; do not retry.")
                    }
                };
                let structural = matches!(
                    self.pending.as_ref().unwrap().command,
                    Command::CreateSpace { .. }
                        | Command::CreateWorktree { .. }
                        | Command::OpenWorktree { .. }
                        | Command::RemoveWorktree { .. }
                        | Command::ClosePane
                        | Command::CloseWorkspace
                );
                if matches!(outcome, Outcome::Success(_)) && structural {
                    self.dismiss();
                }
                if !matches!(outcome, Outcome::Unknown(_)) {
                    self.pending = None;
                }
                true
            }
        };
        if accepted {
            self.revision += 1;
        }
        accepted
    }
    pub fn view(&self) -> View {
        let no_context = self
            .context
            .is_none()
            .then(|| "No focused pane or space".to_string());
        let no_cwd = self
            .context
            .as_ref()
            .and_then(Context::directory)
            .is_none()
            .then(|| "No captured absolute working directory".to_string());
        let no_repo = self.repository.is_none().then(|| {
            if self.read.is_some() {
                "Loading repository".into()
            } else {
                "Repository unavailable".into()
            }
        });
        let row = |label: String, disabled: Option<String>| Row { label, disabled };
        let simple = |labels: &[&str]| {
            labels
                .iter()
                .map(|s| row((*s).into(), None))
                .collect::<Vec<_>>()
        };
        let rows = match &self.frame().screen {
            Screen::Main => vec![
                row("New space".into(), no_cwd.clone()),
                row("Worktrees".into(), no_context.clone()),
                row("Pane actions".into(), no_context.clone()),
                row("Close…".into(), no_context.clone()),
                row("Settings".into(), None),
                row("Logs".into(), None),
                row("Controls".into(), None),
            ],
            Screen::NewSpace => vec![
                row(format!("Name: {}", self.space_name), None),
                row(
                    "Create and switch".into(),
                    validate(Field::Name, &self.space_name)
                        .err()
                        .or(no_cwd.clone()),
                ),
            ],
            Screen::Pane => vec![
                row("Split right".into(), no_cwd.clone()),
                row("Split down".into(), no_cwd),
                row("Toggle zoom".into(), None),
                row(format!("Rename pane: {}", self.pane_label), None),
                row("Clear label".into(), None),
                row("Close pane".into(), None),
                row(
                    "Apply pane label".into(),
                    validate(Field::Label, &self.pane_label).err(),
                ),
            ],
            Screen::Close => simple(&["Close captured pane", "Close captured space"]),
            Screen::Confirm { .. } => simple(&["Cancel", "Confirm"]),
            Screen::Worktrees => ["Create worktree", "Open existing", "Remove worktree"]
                .iter()
                .map(|s| row((*s).into(), no_repo.clone()))
                .collect(),
            Screen::CreateWorktree => vec![
                row(format!("Branch: {}", self.branch), None),
                row(
                    "Create and switch".into(),
                    validate(Field::Branch, &self.branch)
                        .err()
                        .or(no_repo)
                        .or_else(|| {
                            self.repository
                                .as_ref()
                                .filter(|r| r.base_commit.is_empty())
                                .map(|_| "Source HEAD is unavailable".into())
                        })
                        .or_else(|| {
                            self.repository
                                .as_ref()
                                .filter(|r| {
                                    r.entries
                                        .iter()
                                        .any(|e| e.branch.as_deref() == Some(&self.branch))
                                })
                                .map(|_| "Branch already exists".into())
                        }),
                ),
            ],
            Screen::OpenWorktree | Screen::RemoveWorktree => {
                let rows = self
                    .repository
                    .as_ref()
                    .map(|r| {
                        r.entries
                            .iter()
                            .map(|e| {
                                let disabled = if self.frame().screen == Screen::RemoveWorktree {
                                    if e.primary {
                                        Some("Primary checkout cannot be removed".into())
                                    } else if e.bare {
                                        Some("Bare repository cannot be removed".into())
                                    } else if e.workspace.is_none() {
                                        Some("Open this worktree before removal".into())
                                    } else {
                                        None
                                    }
                                } else if e.bare {
                                    Some("Bare repository cannot be opened".into())
                                } else {
                                    None
                                };
                                row(
                                    format!(
                                        "{} — {}{}",
                                        e.branch.as_deref().unwrap_or("detached"),
                                        e.path,
                                        e.workspace
                                            .as_ref()
                                            .map(|w| format!(" [open: {w}]"))
                                            .unwrap_or_default()
                                    ),
                                    disabled,
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if rows.is_empty() {
                    vec![row(
                        "No worktrees".into(),
                        Some("No entries in this repository".into()),
                    )]
                } else {
                    rows
                }
            }
            Screen::Settings => vec![
                row(format!("Button labels: {:?}", self.draft.labels), None),
                row(
                    format!("Voice button: {}", voice_name(self.draft.voice_button)),
                    None,
                ),
                row(
                    format!(
                        "Maximum recording duration: {} seconds",
                        self.draft.max_seconds
                    ),
                    None,
                ),
                row("View voice configuration".into(), None),
                row("Save".into(), None),
                row("Discard".into(), None),
            ],
            Screen::Logs => simple(&["Refresh"]),
            Screen::Controls | Screen::VoiceConfig | Screen::Edit(_) => vec![],
        };
        let global = self
            .stack
            .iter()
            .any(|f| matches!(f.screen, Screen::Settings | Screen::Logs | Screen::Controls));
        let scope = if global {
            "Global".into()
        } else {
            self.context
                .as_ref()
                .map(|c| {
                    format!(
                        "{} [{}] / {} [{}] / tab {}",
                        c.workspace_label, c.workspace, c.pane_label, c.pane, c.tab
                    )
                })
                .unwrap_or_else(|| "No focused context".into())
        };
        let mut details = match &self.frame().screen {
            Screen::NewSpace => vec![format!(
                "Directory: {}",
                self.context
                    .as_ref()
                    .and_then(Context::directory)
                    .unwrap_or("unavailable")
            )],
            Screen::Worktrees
            | Screen::CreateWorktree
            | Screen::OpenWorktree
            | Screen::RemoveWorktree => self
                .repository
                .as_ref()
                .map(|r| {
                    vec![
                        format!("Repository: {} — {}", r.name, r.root),
                        format!(
                            "Source: {}; pinned HEAD: {}",
                            r.source_checkout, r.base_commit
                        ),
                    ]
                })
                .unwrap_or_default(),
            Screen::Confirm { target, .. } => vec![target.clone()],
            Screen::VoiceConfig => vec![
                format!("Model: {}", self.settings.model),
                format!("Language: {}", self.settings.language),
                format!("Configuration: {}", self.settings.config_path),
                "Model and language are read-only".into(),
            ],
            Screen::Logs => self.logs.clone(),
            Screen::Controls => vec![
                format!(
                    "Voice: {}; Menu: {}",
                    voice_name(self.active_settings.voice_button),
                    if self.active_settings.voice_button == Btn::Menu {
                        "L3"
                    } else {
                        "Options/Menu"
                    }
                ),
                "D-pad: select; Left/B: back; Right: open submenu".into(),
                "A/Cross: activate once; Menu/View: close".into(),
                "X/Square: details; text entry: delete".into(),
                "Y/Triangle: text page; A/Cross: insert or Done".into(),
                "Opening the menu returns to Navigate mode".into(),
            ],
            Screen::Edit(field) => vec![format!(
                "Limit: {} characters; X deletes; Y changes page",
                if *field == Field::Branch { 200 } else { 80 }
            )],
            _ => vec![],
        };
        if let Some(row) = rows.get(self.frame().selected) {
            if let Some(reason) = &row.disabled {
                details.push(reason.clone());
            }
            if self.expanded {
                details.insert(0, row.label.clone());
            }
        }
        if let Some(op) = self
            .pending
            .as_ref()
            .or_else(|| self.external_operation.as_ref().map(|(op, _)| op))
        {
            details.push(format!(
                "Operation {}:{}: {:?}; target {:?}",
                op.id.session, op.id.serial, op.command, op.context
            ));
        }
        let status = if self.status.is_empty() {
            match self.external_operation.as_ref().map(|(_, state)| state) {
                Some(OperationState::Unknown(reason)) => {
                    format!("Outcome unknown: {reason}. Reconciliation required; no retry.")
                }
                Some(OperationState::Pending) => {
                    "Pending operation from another menu session".into()
                }
                _ => self.status.clone(),
            }
        } else {
            self.status.clone()
        };
        View {
            revision: self.revision,
            scope,
            breadcrumb: self
                .stack
                .iter()
                .map(|f| f.screen.title())
                .collect::<Vec<_>>()
                .join(" / "),
            rows,
            selected: self.frame().selected,
            details,
            status,
            scroll: if self.expanded {
                self.detail_scroll
            } else {
                self.frame().scroll
            },
            expanded: self.expanded,
            labels: self.active_settings.labels,
            editor: self.editor.clone(),
            menu_button: if self.active_settings.voice_button == Btn::Menu {
                "L3"
            } else {
                "Options/Menu"
            }
            .into(),
        }
    }
}
fn voice_name(button: Btn) -> &'static str {
    match button {
        Btn::LStick => "L3",
        Btn::Menu => "Options/Menu",
        _ => "R3",
    }
}
