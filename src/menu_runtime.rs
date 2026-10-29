//! Controller-owned sessions, with one bounded read worker and one mutation worker.
use crate::{
    config::Config,
    engine::{self, Btn, Core},
    feedback::Publisher,
    herdr::{self, Runner},
    lifecycle::Cancellation,
    menu::*,
    menu_worker::{self, Completion},
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

enum ReadJob {
    Context(u64),
    Read(ReadRequest),
    Launch(u64, PathBuf),
    Focus(u64, String, Option<Context>, Response),
    Reconcile(u64, Operation),
}
enum ReadDone {
    Context(u64, Result<Option<Context>, String>),
    Read(Response),
    Launch(u64, Result<(), String>),
    Focus(Response, Result<(), String>),
    Reconcile(RequestId, Outcome),
}
struct SessionRunner {
    stop: Cancellation,
    interest: Arc<AtomicU64>,
    session: u64,
}
impl Runner for SessionRunner {
    fn run(&self, argv: &[&str]) -> Result<String, String> {
        herdr::run_with_cancellation(argv, Duration::from_secs(10), || {
            self.stop.cancelled() || self.interest.load(Ordering::SeqCst) != self.session
        })
    }
}
struct Workers {
    reads: Option<SyncSender<ReadJob>>,
    mutations: Option<SyncSender<Operation>>,
    results: Receiver<ReadDone>,
    completed: Receiver<Completion>,
    threads: Vec<JoinHandle<()>>,
    interest: Arc<AtomicU64>,
    stop: Cancellation,
}
impl Workers {
    fn new(stop: Cancellation, config: PathBuf, state: PathBuf) -> Self {
        let interest = Arc::new(AtomicU64::new(0));
        let (rt, rr) = mpsc::sync_channel::<ReadJob>(1);
        let (dt, dr) = mpsc::channel();
        let (mt, mr) = mpsc::sync_channel::<Operation>(1);
        let (ct, cr) = mpsc::channel();
        let ri = interest.clone();
        let rs = stop.clone();
        let log_state = state.clone();
        let read_thread = std::thread::spawn(move || {
            while let Ok(job) = rr.recv() {
                let session = match &job {
                    ReadJob::Context(s)
                    | ReadJob::Launch(s, _)
                    | ReadJob::Focus(s, ..)
                    | ReadJob::Reconcile(s, _) => *s,
                    ReadJob::Read(r) => r.id.session,
                };
                let runner = SessionRunner {
                    stop: rs.clone(),
                    interest: ri.clone(),
                    session,
                };
                let done = match job {
                    ReadJob::Reconcile(_, op) => {
                        ReadDone::Reconcile(op.id, menu_worker::reconcile(&runner, &op))
                    }
                    ReadJob::Context(s) => ReadDone::Context(s, menu_worker::context(&runner)),
                    ReadJob::Read(r) => {
                        let result = match r.kind {
                            ReadKind::Repository { context } => {
                                menu_worker::repository_for_context(&runner, &context)
                                    .map(ReadData::Repository)
                            }
                            ReadKind::Logs => crate::logging::tail(&log_state)
                                .map(|s| ReadData::Logs(s.lines().map(String::from).collect())),
                        };
                        ReadDone::Read(Response::Read { id: r.id, result })
                    }
                    ReadJob::Launch(s, dir) => ReadDone::Launch(
                        s,
                        runner
                            .run(&[
                                "herdr",
                                "plugin",
                                "pane",
                                "open",
                                "--plugin",
                                crate::config::PLUGIN_ID,
                                "--entrypoint",
                                "actions-menu",
                                "--no-focus",
                                "--env",
                                &format!("GAMEPAD_MENU_DIR={}", dir.display()),
                            ])
                            .map(|_| ()),
                    ),
                    ReadJob::Focus(_, workspace, expected, response) => {
                        let result = menu_worker::context(&runner).and_then(|fresh| {
                            let same = match (&expected, &fresh) {
                                (Some(a), Some(b)) => a.same_identity(b),
                                (None, None) => true,
                                _ => false,
                            };
                            if !same {
                                return Err(
                                    "Focus changed; created workspace remains available".into()
                                );
                            }
                            runner
                                .run(&["herdr", "workspace", "focus", &workspace])
                                .map(|_| ())
                        });
                        ReadDone::Focus(response, result)
                    }
                };
                if dt.send(done).is_err() {
                    break;
                }
            }
        });
        let mi = interest.clone();
        let ms = stop.clone();
        let mutation_thread = std::thread::spawn(move || {
            while let Ok(op) = mr.recv() {
                let runner = SessionRunner {
                    stop: ms.clone(),
                    interest: mi.clone(),
                    session: op.id.session,
                };
                let done = menu_worker::execute_using(
                    &runner,
                    &crate::herdr::CancellableCli {
                        timeout: Duration::from_secs(10),
                        cancellation: ms.clone(),
                    },
                    &op,
                    || !ms.cancelled() && mi.load(Ordering::SeqCst) == op.id.session,
                    &config,
                    &state,
                );
                // Record completion even if Stop prevents another controller tick.
                crate::logging::event(format_args!(
                    "Menu operation {:?}: {:?}; workspace {:?}, pane {:?}, worktree {:?}",
                    done.id, done.outcome, done.workspace, done.pane, done.worktree
                ));
                if ct.send(done).is_err() {
                    break;
                }
            }
        });
        Self {
            reads: Some(rt),
            mutations: Some(mt),
            results: dr,
            completed: cr,
            threads: vec![read_thread, mutation_thread],
            interest,
            stop,
        }
    }
    fn read(&self, job: ReadJob) -> Result<(), String> {
        self.reads
            .as_ref()
            .unwrap()
            .try_send(job)
            .map_err(|_| "Read worker busy".into())
    }
}
impl Drop for Workers {
    fn drop(&mut self) {
        self.interest.store(0, Ordering::SeqCst);
        self.stop.cancel();
        self.reads.take();
        self.mutations.take();
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}
pub struct MenuRuntime {
    workers: Workers,
    model: Option<Menu>,
    publisher: Option<Publisher>,
    opening: bool,
    notice: Option<String>,
    review_voice_requested: bool,
    session: u64,
    sequence: u64,
    slot: OperationSlot,
    active: SettingsSnapshot,
    committed: Option<(SettingsSnapshot, Config)>,
    state: PathBuf,
    config: PathBuf,
    held: Vec<Btn>,
    swallowed: Vec<Btn>,
    repeat: Option<(Btn, Instant)>,
    read_busy: bool,
    launch: Option<PathBuf>,
    queued_read: Option<ReadRequest>,
    follow: Option<(String, Option<Context>, Response)>,
    last_context: Instant,
    last_reconcile: Instant,
}
impl MenuRuntime {
    pub fn new(stop: Cancellation, state: PathBuf, config: PathBuf) -> Result<Self, String> {
        let (active, _) = crate::settings::load(&config, &state)?;
        Ok(Self {
            workers: Workers::new(stop, config.clone(), state.clone()),
            model: None,
            publisher: None,
            opening: false,
            notice: None,
            review_voice_requested: false,
            session: 0,
            sequence: 0,
            slot: OperationSlot::default(),
            active,
            committed: None,
            state,
            config,
            held: vec![],
            swallowed: vec![],
            repeat: None,
            read_busy: false,
            launch: None,
            queued_read: None,
            follow: None,
            last_reconcile: Instant::now(),
            last_context: Instant::now(),
        })
    }
    pub fn take_voice_review_request(&mut self) -> bool { std::mem::take(&mut self.review_voice_requested) }
    pub fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }
    pub fn is_open(&self) -> bool {
        self.opening || self.model.is_some()
    }
    fn close(&mut self, core: &mut Core, cfg: &mut Config) {
        self.workers.interest.store(0, Ordering::SeqCst);
        self.model = None;
        self.publisher = None;
        self.opening = false;
        self.queued_read = None;
        self.launch = None;
        self.follow = None;
        self.repeat = None;
        for b in &self.held {
            if !self.swallowed.contains(b) {
                self.swallowed.push(*b)
            }
        }
        core.menu_exit();
        self.apply_config(core, cfg);
    }
    fn apply_config(&mut self, core: &mut Core, cfg: &mut Config) {
        // A save can complete after dismissal and after another recording starts.
        // Keep both the release binding and transcription configuration until it ends.
        if core.voice_busy() {
            return;
        }
        if let Some((snapshot, new)) = self.committed.take() {
            self.active = snapshot;
            *cfg = new;
            core.rebind(cfg.voice_button);
            core.set_preview_enabled(cfg.preview_before_send);
        }
    }
    /// true means consumed; call before normal-mode snapshot acquisition.
    pub fn handle(&mut self, input: &engine::Input, core: &mut Core, cfg: &mut Config) -> bool {
        if let engine::Input::Release(b) = input {
            self.held.retain(|held| held != b);
            if self.repeat.as_ref().is_some_and(|(held, _)| held == b) {
                self.repeat = None;
            }
            if let Some(i) = self.swallowed.iter().position(|held| held == b) {
                self.swallowed.remove(i);
                return true;
            }
        }
        if matches!(input, engine::Input::Disconnected) {
            let was = self.is_open();
            self.close(core, cfg);
            self.held.clear();
            self.swallowed.clear();
            return was;
        }
        if let engine::Input::Press(b) = input {
            if self.swallowed.contains(b) {
                return true;
            }
            if self.held.contains(b) {
                return self.is_open();
            }
            self.held.push(*b);
            if !self.is_open() && *b == engine::menu_button(cfg.voice_button) {
                self.swallowed.push(*b);
                if core.voice_busy() {
                    self.notice = Some("Finish dictation before opening actions".into());
                    return true;
                }
                core.menu_exit();
                self.follow = None;
                self.sequence += 1;
                self.session = self.sequence;
                self.opening = true;
                self.workers.interest.store(self.session, Ordering::SeqCst);
                self.last_context = Instant::now() - Duration::from_secs(1);
                self.schedule();
                return true;
            }
        }
        if !self.is_open() {
            return false;
        }
        match input {
            engine::Input::Press(b)
                if *b == engine::menu_button(cfg.voice_button) || *b == Btn::View =>
            {
                self.close(core, cfg);
                return true;
            }
            engine::Input::Press(Btn::B) if self.opening => {
                self.close(core, cfg);
                return true;
            }
            engine::Input::Press(b) => {
                if let Some(mapped) = map_button(*b) {
                    self.input(mapped, core, cfg);
                    let editing = self
                        .model
                        .as_ref()
                        .is_some_and(|m| m.view().editor.is_some());
                    if matches!(b, Btn::Up | Btn::Down)
                        || (editing && matches!(b, Btn::Left | Btn::Right))
                    {
                        self.repeat = Some((*b, Instant::now() + Duration::from_millis(400)));
                    }
                }
            }
            engine::Input::Release(b) => {
                if let Some(mapped) = map_button(*b).and_then(Input::action_key) {
                    self.input(Input::Release(mapped), core, cfg)
                }
            }
            // Voice completions must still reach Core, although opening is refused while busy.
            engine::Input::Transcript(_) | engine::Input::TranscriptFailed(_) => return false,
            _ => {}
        }
        true
    }
    fn input(&mut self, input: Input, core: &mut Core, cfg: &mut Config) {
        let ack = match self.publisher.as_mut().map(Publisher::poll) {
            Some(Ok((ack, width))) => {
                if let (Some(model), Some(width)) = (&mut self.model, width) {
                    model.set_detail_width(width);
                }
                ack
            }
            Some(Err(e)) => {
                crate::logging::event(format_args!("{e}"));
                self.close(core, cfg);
                return;
            }
            None => 0,
        };
        let effects = self
            .model
            .as_mut()
            .map(|m| m.input(input, ack))
            .unwrap_or_default();
        for effect in effects {
            match effect {
                Effect::Closed => self.close(core, cfg),
                Effect::ReviewVoiceDraft => { self.close(core,cfg); self.review_voice_requested=true; },
                Effect::Read(read) => self.queued_read = Some(read),
                Effect::Mutate(op) => {
                    if self.slot.reserve(op.clone()) {
                        if self
                            .workers
                            .mutations
                            .as_ref()
                            .unwrap()
                            .try_send(op.clone())
                            .is_err()
                        {
                            let outcome = Outcome::Failed("Mutation worker unavailable".into());
                            self.slot.complete(op.id, outcome.clone());
                            if let Some(m) = &mut self.model {
                                m.receive(Response::Mutation { id: op.id, outcome });
                            }
                        }
                    }
                }
            }
        }
        self.publish(core, cfg);
        self.schedule();
    }
    fn publish(&mut self, core: &mut Core, cfg: &mut Config) {
        if self.model.as_ref().is_some_and(|m| !m.is_open()) {
            self.close(core, cfg);
            return;
        }
        if let Some(m) = &mut self.model {
            m.sync_operation_slot(&self.slot);
            if let Some(p) = &mut self.publisher {
                if let Err(e) = p.publish(&m.view()) {
                    crate::logging::event(format_args!("Menu: {e}"));
                    self.close(core, cfg);
                }
            }
        }
    }
    pub fn tick(&mut self, core: &mut Core, cfg: &mut Config) {
        while let Ok(done) = self.workers.results.try_recv() {
            self.read_busy = false;
            match done {
                ReadDone::Reconcile(id, outcome) => {
                    self.last_reconcile = Instant::now();
                    if self.slot.reconcile(id, outcome.clone()) {
                        if let Some(m) = &mut self.model {
                            m.receive(Response::Mutation { id, outcome });
                        }
                    }
                }
                ReadDone::Context(s, result) if s == self.session && self.is_open() => {
                    self.last_context = Instant::now();
                    if self.opening {
                        match result.and_then(|context| {
                            let (active, _) = crate::settings::load(&self.config, &self.state)?;
                            let model = Menu::new(
                                s,
                                context,
                                active,
                                "New space".into(),
                                format!(
                                    "gamepad-{}-{s}",
                                    std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_secs()
                                ),
                            )
                            .with_active_settings(self.active.clone());
                            let publisher = Publisher::new(&self.state, &model.view())?;
                            Ok((model, publisher))
                        }) {
                            Ok((mut model, publisher)) => {
                                model.sync_operation_slot(&self.slot);
                                self.launch = Some(publisher.directory().into());
                                self.model = Some(model);
                                self.publisher = Some(publisher);
                                self.opening = false;
                            }
                            Err(e) => {
                                self.notice = Some(format!("Actions unavailable: {e}"));
                                self.close(core, cfg);
                            }
                        }
                    } else if let Some(model) = &mut self.model {
                        if model.observe_context(result) {
                            self.close(core, cfg);
                        }
                    }
                }
                ReadDone::Read(response) => {
                    if let Some(m) = &mut self.model {
                        m.receive(response);
                    }
                }
                ReadDone::Launch(s, Err(e)) if s == self.session && self.is_open() => {
                    self.notice = Some(format!("Actions popup unavailable: {e}"));
                    self.close(core, cfg);
                }
                ReadDone::Focus(response, result) => {
                    if let Err(e) = result {
                        crate::logging::event(format_args!("Workspace focus: {e}"));
                    }
                    if let Some(m) = &mut self.model {
                        m.receive(response);
                    }
                }
                _ => {}
            }
        }
        while let Ok(done) = self.workers.completed.try_recv() {
            self.slot.complete(done.id, done.outcome.clone());
            if let Some(saved) = done.committed {
                self.committed = Some(saved.clone());
                if let Some(m) = &mut self.model {
                    m.settings_saved(done.id, saved.0);
                }
            }
            if let Some(current) = done.conflict {
                if let Some(m) = &mut self.model {
                    m.settings_conflict(done.id, current);
                }
            }
            let interested =
                self.model.as_ref().is_some_and(|m| m.is_open()) && done.id.session == self.session;
            if interested {
                let response = Response::Mutation {
                    id: done.id,
                    outcome: done.outcome,
                };
                if let Some(workspace) = done.workspace {
                    self.follow = Some((
                        workspace,
                        self.model.as_ref().and_then(|m| m.context().cloned()),
                        response,
                    ));
                } else if let Some(m) = &mut self.model {
                    m.receive(response);
                }
            }
        }
        // Retry deferred activation even when there is no new worker completion.
        if !self.is_open() {
            self.apply_config(core, cfg);
        }
        if let Some(p) = &mut self.publisher {
            if p.poll().is_err() {
                self.close(core, cfg);
            }
        }
        if let Some((button, when)) = self.repeat {
            let editing = self
                .model
                .as_ref()
                .is_some_and(|m| m.view().editor.is_some());
            if matches!(button, Btn::Left | Btn::Right) && !editing {
                self.repeat = None;
            } else if Instant::now() >= when && self.model.is_some() {
                if let Some(input) = map_button(button) {
                    if let Some(key) = input.action_key() {
                        self.input(Input::Release(key), core, cfg);
                    }
                    self.input(input, core, cfg);
                }
                self.repeat = Some((button, Instant::now() + Duration::from_millis(100)));
            }
        }
        if let Some(model)=&mut self.model { model.set_voice_draft_available(core.draft().is_some()); }
        self.publish(core, cfg);
        self.schedule();
    }
    fn schedule(&mut self) {
        if self.read_busy {
            return;
        }
        let job = if let Some((workspace, context, response)) = self.follow.take() {
            Some(ReadJob::Focus(self.session, workspace, context, response))
        } else if let Some(dir) = self.launch.take() {
            Some(ReadJob::Launch(self.session, dir))
        } else if let Some(read) = self.queued_read.take() {
            Some(ReadJob::Read(read))
        } else if self.is_open()
            && matches!(self.slot.state(), Some(OperationState::Unknown(_)))
            && self.last_reconcile.elapsed() >= Duration::from_secs(2)
        {
            self.slot
                .operation()
                .cloned()
                .map(|op| ReadJob::Reconcile(self.session, op))
        } else if self.is_open() && self.last_context.elapsed() >= Duration::from_millis(200) {
            Some(ReadJob::Context(self.session))
        } else {
            None
        };
        if let Some(job) = job {
            self.read_busy = self.workers.read(job).is_ok();
        }
    }
}
fn map_button(button: Btn) -> Option<Input> {
    Some(match button {
        Btn::Up => Input::Up,
        Btn::Down => Input::Down,
        Btn::Left => Input::Left,
        Btn::Right => Input::Right,
        Btn::A => Input::Activate,
        Btn::B => Input::Back,
        Btn::X => Input::Delete,
        Btn::Y => Input::Page,
        _ => return None,
    })
}
#[cfg(test)]
#[path = "menu_runtime_tests.rs"]
mod tests;
