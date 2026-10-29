//! Pure controller menu. Effects contain frozen targets; this module performs no I/O.
use crate::engine::Btn;
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LabelStyle {
    #[default]
    Both,
    PlayStation,
    Xbox,
}

impl LabelStyle {
    pub fn button(self, button: Btn) -> &'static str {
        match (self, button) {
            (Self::Both, Btn::A) => "✕ Cross/A",
            (Self::Both, Btn::B) => "○ Circle/B",
            (Self::Both, Btn::X) => "□ Square/X",
            (Self::Both, Btn::Y) => "△ Triangle/Y",
            (Self::Both, Btn::Menu) => "Options/Menu",
            (Self::Both, Btn::View) => "Create/Share/View",
            (Self::Both, Btn::LStick) => "L3/Left stick",
            (Self::Both, Btn::RStick) => "R3/Right stick",
            (Self::PlayStation, Btn::A) => "✕ Cross",
            (Self::PlayStation, Btn::B) => "○ Circle",
            (Self::PlayStation, Btn::X) => "□ Square",
            (Self::PlayStation, Btn::Y) => "△ Triangle",
            (Self::PlayStation, Btn::Menu) => "Options",
            (Self::PlayStation, Btn::View) => "Create/Share",
            (Self::PlayStation, Btn::LStick) => "L3",
            (Self::PlayStation, Btn::RStick) => "R3",
            (Self::Xbox, Btn::A) => "A",
            (Self::Xbox, Btn::B) => "B",
            (Self::Xbox, Btn::X) => "X",
            (Self::Xbox, Btn::Y) => "Y",
            (Self::Xbox, Btn::Menu) => "Menu",
            (Self::Xbox, Btn::View) => "View",
            (Self::Xbox, Btn::LStick) => "Left stick",
            (Self::Xbox, Btn::RStick) => "Right stick",
            _ => button.label(),
        }
    }
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
    Repository { context: Context },
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
    pub expected_repository_root: Option<String>,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Row {
    pub label: String,
    pub disabled: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
/// A row carries its behavior with its presentation, so reordering rows cannot
/// change which command a visible label invokes. Only the view crosses IPC.
#[derive(Clone, Debug, PartialEq, Eq)]
struct MenuRow {
    view: Row,
    action: RowAction,
}
#[derive(Clone, Debug, PartialEq, Eq)]
enum RowAction {
    Open(Screen),
    Edit(Field),
    Run(Command),
    ClosePane,
    CloseWorkspace,
    RemoveWorktree(Worktree),
    CycleLabels,
    CycleVoice,
    CycleDuration,
    SaveSettings,
    Back,
    RefreshLogs,
    None,
}
impl RowAction {
    fn opens_submenu(&self) -> bool {
        matches!(
            self,
            Self::Open(_) | Self::Edit(Field::Label) | Self::ClosePane | Self::CloseWorkspace
        )
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
    /// Keep persisted form values independent from the controls active this session.
    pub fn with_active_settings(mut self, active: SettingsSnapshot) -> Self {
        self.active_settings = active;
        self
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
        let external = slot
            .operation()
            .zip(slot.state())
            .map(|(op, state)| (op.clone(), state.clone()));
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
            expected_repository_root: if matches!(
                command,
                Command::CreateWorktree { .. }
                    | Command::OpenWorktree { .. }
                    | Command::RemoveWorktree { .. }
            ) {
                self.repository.as_ref().map(|r| r.root.clone())
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
        let rows = self.rows();
        let Some(row) = rows.get(self.frame().selected) else {
            return vec![];
        };
        self.activate_row(row.clone(), input)
    }
    fn activate_row(&mut self, row: MenuRow, input: Input) -> Vec<Effect> {
        if let Some(reason) = row.view.disabled {
            self.status = reason;
            return vec![];
        }
        if input == Input::Right && !row.action.opens_submenu() {
            return vec![];
        }
        self.activate(row.action)
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
    fn activate(&mut self, action: RowAction) -> Vec<Effect> {
        match action {
            RowAction::Open(screen) => {
                if screen == Screen::Settings {
                    self.draft = self.settings.clone();
                }
                self.push(screen.clone());
                if matches!(screen, Screen::Worktrees | Screen::CreateWorktree) {
                    return self.start_read(ReadKind::Repository {
                        context: self.context.as_ref().unwrap().clone(),
                    });
                }
                if screen == Screen::Logs {
                    return self.start_read(ReadKind::Logs);
                }
            }
            RowAction::Edit(field) => self.edit(field),
            RowAction::Run(command) => return self.mutate(command),
            RowAction::ClosePane => self.confirm(Command::ClosePane, self.pane_target()),
            RowAction::CloseWorkspace => {
                let c = self.context.as_ref().unwrap();
                self.confirm(
                    Command::CloseWorkspace,
                    format!(
                        "{} [{}] — running processes stop; worktree remains",
                        c.workspace_label, c.workspace
                    ),
                );
            }
            RowAction::RemoveWorktree(e) => {
                self.confirm(Command::RemoveWorktree{path:e.path.clone(),workspace:e.workspace.clone().unwrap()},format!("Remove checkout {} — branch {} — workspace {}. Stops processes and deletes this checkout.",e.path,e.branch.as_deref().unwrap_or("detached"),e.workspace.as_deref().unwrap()));
            }
            RowAction::CycleLabels => {
                self.draft.labels = match self.draft.labels {
                    LabelStyle::Both => LabelStyle::PlayStation,
                    LabelStyle::PlayStation => LabelStyle::Xbox,
                    LabelStyle::Xbox => LabelStyle::Both,
                }
            }
            RowAction::CycleVoice => {
                self.draft.voice_button = match self.draft.voice_button {
                    Btn::RStick => Btn::LStick,
                    Btn::LStick => Btn::Menu,
                    _ => Btn::RStick,
                }
            }
            RowAction::CycleDuration => {
                let presets = [15, 30, 60, 120, 300];
                self.draft.max_seconds = presets
                    .iter()
                    .position(|v| *v == self.draft.max_seconds)
                    .map(|i| presets[(i + 1) % presets.len()])
                    .unwrap_or(15);
            }
            RowAction::SaveSettings => {
                return self.mutate(Command::SaveSettings {
                    original: self.settings.clone(),
                    updated: self.draft.clone(),
                })
            }
            RowAction::Back => return self.back(),
            RowAction::RefreshLogs => return self.start_read(ReadKind::Logs),
            RowAction::None => {}
        }
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
    fn rows(&self) -> Vec<MenuRow> {
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
        let row = |label: String, disabled: Option<String>, action| MenuRow {
            view: Row { label, disabled },
            action,
        };
        let directory = self
            .context
            .as_ref()
            .and_then(Context::directory)
            .unwrap_or_default();
        match &self.frame().screen {
            Screen::Main => vec![
                row(
                    "New space".into(),
                    no_cwd.clone(),
                    RowAction::Open(Screen::NewSpace),
                ),
                row(
                    "Worktrees".into(),
                    no_context.clone(),
                    RowAction::Open(Screen::Worktrees),
                ),
                row(
                    "Pane actions".into(),
                    no_context.clone(),
                    RowAction::Open(Screen::Pane),
                ),
                row(
                    "Close…".into(),
                    no_context.clone(),
                    RowAction::Open(Screen::Close),
                ),
                row("Settings".into(), None, RowAction::Open(Screen::Settings)),
                row("Logs".into(), None, RowAction::Open(Screen::Logs)),
                row("Controls".into(), None, RowAction::Open(Screen::Controls)),
            ],
            Screen::NewSpace => vec![
                row(
                    format!("Name: {}", self.space_name),
                    None,
                    RowAction::Edit(Field::Name),
                ),
                row(
                    "Create and switch".into(),
                    validate(Field::Name, &self.space_name)
                        .err()
                        .or(no_cwd.clone()),
                    RowAction::Run(Command::CreateSpace {
                        name: self.space_name.clone(),
                        cwd: directory.into(),
                    }),
                ),
            ],
            Screen::Pane => vec![
                row(
                    "Split right".into(),
                    no_cwd.clone(),
                    RowAction::Run(Command::Split {
                        direction: SplitDirection::Right,
                        cwd: directory.into(),
                    }),
                ),
                row(
                    "Split down".into(),
                    no_cwd,
                    RowAction::Run(Command::Split {
                        direction: SplitDirection::Down,
                        cwd: directory.into(),
                    }),
                ),
                row(
                    "Toggle zoom".into(),
                    None,
                    RowAction::Run(Command::ToggleZoom),
                ),
                row(
                    format!("Rename pane: {}", self.pane_label),
                    None,
                    RowAction::Edit(Field::Label),
                ),
                row(
                    "Clear label".into(),
                    None,
                    RowAction::Run(Command::RenamePane { label: None }),
                ),
                row("Close pane".into(), None, RowAction::ClosePane),
                row(
                    "Apply pane label".into(),
                    validate(Field::Label, &self.pane_label).err(),
                    RowAction::Run(Command::RenamePane {
                        label: Some(self.pane_label.clone()),
                    }),
                ),
            ],
            Screen::Close => vec![
                row("Close captured pane".into(), None, RowAction::ClosePane),
                row(
                    "Close captured space".into(),
                    None,
                    RowAction::CloseWorkspace,
                ),
            ],
            Screen::Confirm { command, .. } => vec![
                row("Cancel".into(), None, RowAction::Back),
                row("Confirm".into(), None, RowAction::Run(command.clone())),
            ],
            Screen::Worktrees => vec![
                row(
                    "Create worktree".into(),
                    no_repo.clone(),
                    RowAction::Open(Screen::CreateWorktree),
                ),
                row(
                    "Open existing".into(),
                    no_repo.clone(),
                    RowAction::Open(Screen::OpenWorktree),
                ),
                row(
                    "Remove worktree".into(),
                    no_repo.clone(),
                    RowAction::Open(Screen::RemoveWorktree),
                ),
            ],
            Screen::CreateWorktree => vec![
                row(
                    format!("Branch: {}", self.branch),
                    None,
                    RowAction::Edit(Field::Branch),
                ),
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
                    RowAction::Run(Command::CreateWorktree {
                        branch: self.branch.clone(),
                        base: self
                            .repository
                            .as_ref()
                            .map(|r| r.base_commit.clone())
                            .unwrap_or_default(),
                        repository: self
                            .repository
                            .as_ref()
                            .map(|r| r.root.clone())
                            .unwrap_or_default(),
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
                                    if self.frame().screen == Screen::RemoveWorktree {
                                        RowAction::RemoveWorktree(e.clone())
                                    } else {
                                        RowAction::Run(Command::OpenWorktree {
                                            path: e.path.clone(),
                                            workspace: e.workspace.clone(),
                                        })
                                    },
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if rows.is_empty() {
                    vec![row(
                        "No worktrees".into(),
                        Some("No entries in this repository".into()),
                        RowAction::None,
                    )]
                } else {
                    rows
                }
            }
            Screen::Settings => vec![
                row(
                    format!("Button labels: {:?}", self.draft.labels),
                    None,
                    RowAction::CycleLabels,
                ),
                row(
                    format!(
                        "Voice button: {}",
                        self.draft.labels.button(self.draft.voice_button)
                    ),
                    None,
                    RowAction::CycleVoice,
                ),
                row(
                    format!(
                        "Maximum recording duration: {} seconds",
                        self.draft.max_seconds
                    ),
                    None,
                    RowAction::CycleDuration,
                ),
                row(
                    "View voice configuration".into(),
                    None,
                    RowAction::Open(Screen::VoiceConfig),
                ),
                row("Save".into(), None, RowAction::SaveSettings),
                row("Discard".into(), None, RowAction::Back),
            ],
            Screen::Logs => vec![row("Refresh".into(), None, RowAction::RefreshLogs)],
            Screen::Controls | Screen::VoiceConfig | Screen::Edit(_) => vec![],
        }
    }
    pub fn view(&self) -> View {
        let rows = self.rows().into_iter().map(|r| r.view).collect::<Vec<_>>();
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
            Screen::Controls => {
                let style = self.active_settings.labels;
                let menu = if self.active_settings.voice_button == Btn::Menu {
                    Btn::LStick
                } else {
                    Btn::Menu
                };
                vec![
                    format!(
                        "Voice: {}; Menu: {}",
                        style.button(self.active_settings.voice_button),
                        style.button(menu)
                    ),
                    format!(
                        "D-pad: select; Left/{}: back; Right: open submenu",
                        style.button(Btn::B)
                    ),
                    format!(
                        "{}: activate once; {}/{}: close",
                        style.button(Btn::A),
                        style.button(menu),
                        style.button(Btn::View)
                    ),
                    format!("{}: details; text entry: delete", style.button(Btn::X)),
                    format!(
                        "{}: text page; {}: insert or Done",
                        style.button(Btn::Y),
                        style.button(Btn::A)
                    ),
                    "Opening the menu returns to Navigate mode".into(),
                ]
            }
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
                "Operation {}:{}: {}; target {:?}",
                op.id.session,
                op.id.serial,
                if matches!(op.command, Command::SaveSettings { .. }) {
                    "Save settings".into()
                } else {
                    format!("{:?}", op.command)
                },
                op.context
            ));
        }
        if let Some((_, OperationState::Finished(outcome))) = &self.external_operation {
            let message = match outcome {
                Outcome::Success(message)
                | Outcome::Failed(message)
                | Outcome::Unknown(message) => message,
            };
            details.push(format!("Last operation result: {message}"));
        }
        let status = if self.status.is_empty() {
            match self.external_operation.as_ref().map(|(_, state)| state) {
                Some(OperationState::Unknown(reason)) => {
                    format!("Outcome unknown: {reason}. Reconciliation required; no retry.")
                }
                Some(OperationState::Pending) => {
                    "Pending operation from another menu session".into()
                }
                Some(OperationState::Finished(Outcome::Success(message))) => {
                    format!("Last result: {message}")
                }
                Some(OperationState::Finished(Outcome::Failed(message))) => {
                    format!("Last failure: {message}")
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

#[cfg(test)]
mod row_identity_tests {
    use super::*;

    #[test]
    fn reordered_pane_rows_keep_their_action_and_submenu_identity() {
        for (label, expected) in [
            (
                "Split right",
                Command::Split {
                    direction: SplitDirection::Right,
                    cwd: "/tmp/project".into(),
                },
            ),
            (
                "Split down",
                Command::Split {
                    direction: SplitDirection::Down,
                    cwd: "/tmp/project".into(),
                },
            ),
            ("Toggle zoom", Command::ToggleZoom),
            ("Clear label", Command::RenamePane { label: None }),
        ] {
            let mut m = crate::menu_tests::menu();
            m.push(Screen::Pane);
            let mut rows = m.rows();
            rows.reverse();
            let index = rows.iter().position(|row| row.view.label == label).unwrap();
            m.frame_mut().selected = index;
            let row = rows[index].clone();
            assert!(m.activate_row(row.clone(), Input::Right).is_empty());
            assert_eq!(
                m.activate_row(row, Input::Activate)
                    .iter()
                    .find_map(|effect| match effect {
                        Effect::Mutate(op) => Some(&op.command),
                        _ => None,
                    }),
                Some(&expected),
                "{label}"
            );
        }
        let mut m = crate::menu_tests::menu();
        m.push(Screen::Pane);
        let row = m
            .rows()
            .into_iter()
            .find(|row| row.view.label.starts_with("Rename pane:"))
            .unwrap();
        // Move the displayed row to index zero: eligibility must not come from index 3.
        m.frame_mut().selected = 0;
        m.activate_row(row, Input::Right);
        assert_eq!(m.editor.as_ref().map(|e| e.field), Some(Field::Label));
    }

    #[test]
    fn worktree_row_carries_its_path_when_entries_reorder() {
        let mut m = crate::menu_tests::menu();
        m.repository = Some(crate::menu_tests::repository());
        m.push(Screen::OpenWorktree);
        let row = m.rows().remove(1);
        assert!(row.view.label.contains("/tmp/linked"));
        m.repository.as_mut().unwrap().entries.reverse();
        m.frame_mut().selected = 0;
        let effects = m.activate_row(row, Input::Activate);
        assert_eq!(
            effects.iter().find_map(|effect| match effect {
                Effect::Mutate(op) => Some(&op.command),
                _ => None,
            }),
            Some(&Command::OpenWorktree {
                path: "/tmp/linked".into(),
                workspace: Some("ws-2".into())
            })
        );
    }
}
