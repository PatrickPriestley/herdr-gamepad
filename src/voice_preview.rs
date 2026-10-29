//! Controller-owned, transient dictation preview.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PreviewView {
    pub text: String,
    pub destination: String,
    pub status: String,
    pub voice_button: String,
}

use crate::{
    config::Config,
    dictation::DeliveryState,
    engine::{Btn, Core, Input, Snapshot},
    feedback::Publisher,
    herdr::{CancellableCli, Herdr},
    lifecycle::Cancellation,
    menu::View,
};
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc,
    },
    thread::JoinHandle,
    time::Duration,
};

#[derive(Debug, PartialEq, Eq)]
enum Choice {
    Pass,
    Eat,
    Back,
    Send,
    Delete,
    Scroll(bool),
}
#[derive(Default)]
struct InputGate {
    open: bool,
    lost: bool,
    held: Vec<Btn>,
    consumed: Vec<Btn>,
}
impl InputGate {
    fn decide(&mut self, input: &Input, voice: Btn, ready: bool, busy: bool) -> Choice {
        match input {
            Input::Disconnected => {
                self.open = false;
                self.lost = false;
                self.held.clear();
                self.consumed.clear();
                Choice::Pass
            }
            Input::Release(button) => {
                self.held.retain(|b| b != button);
                if let Some(i) = self.consumed.iter().position(|b| b == button) {
                    self.consumed.remove(i);
                    Choice::Eat
                } else if self.open && *button != voice {
                    Choice::Eat
                } else {
                    Choice::Pass
                }
            }
            Input::Press(button) => {
                if self.lost && *button != voice {
                    self.lost = false;
                    self.held.retain(|held| held != button);
                    self.held.push(*button);
                    if *button == crate::engine::menu_button(voice) {
                        return Choice::Pass;
                    }
                    self.consumed.retain(|held| held != button);
                    self.consumed.push(*button);
                    return Choice::Eat;
                }
                if self.held.contains(button) {
                    return if self.open || self.consumed.contains(button) {
                        Choice::Eat
                    } else {
                        Choice::Pass
                    };
                }
                self.held.push(*button);
                if !self.open || *button == voice {
                    return Choice::Pass;
                }
                self.consumed.push(*button);
                match button {
                    Btn::B | Btn::View => Choice::Back,
                    b if *b == crate::engine::menu_button(voice) => Choice::Back,
                    Btn::A if ready && !busy => Choice::Send,
                    Btn::X if ready && !busy => Choice::Delete,
                    Btn::Up => Choice::Scroll(false),
                    Btn::Down => Choice::Scroll(true),
                    _ => Choice::Eat,
                }
            }
            _ => Choice::Pass,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn preview_gate_requires_fresh_acknowledged_press_and_keeps_back_available() {
        let mut g = InputGate::default();
        assert_eq!(
            g.decide(&Input::Press(Btn::A), Btn::RStick, true, false),
            Choice::Pass
        );
        g.open = true;
        assert_eq!(
            g.decide(&Input::Press(Btn::A), Btn::RStick, true, false),
            Choice::Eat
        );
        g.decide(&Input::Release(Btn::A), Btn::RStick, true, false);
        assert_eq!(
            g.decide(&Input::Press(Btn::A), Btn::RStick, false, false),
            Choice::Eat
        );
        g.decide(&Input::Release(Btn::A), Btn::RStick, true, false);
        assert_eq!(
            g.decide(&Input::Press(Btn::A), Btn::RStick, true, false),
            Choice::Send
        );
        assert_eq!(
            g.decide(&Input::Press(Btn::A), Btn::RStick, true, false),
            Choice::Eat
        );
        assert_eq!(
            g.decide(&Input::Press(Btn::B), Btn::RStick, false, true),
            Choice::Back
        );
        g.open = false;
        assert_eq!(
            g.decide(&Input::Release(Btn::A), Btn::RStick, true, false),
            Choice::Eat
        );
        assert_eq!(
            g.decide(&Input::Release(Btn::B), Btn::RStick, true, false),
            Choice::Eat
        );
    }
    #[test]
    fn preview_gate_allows_voice_collision_and_blocks_busy_mutations() {
        let mut g = InputGate {
            open: true,
            ..Default::default()
        };
        assert_eq!(
            g.decide(&Input::Press(Btn::Menu), Btn::Menu, true, false),
            Choice::Pass
        );
        assert_eq!(
            g.decide(&Input::Release(Btn::Menu), Btn::Menu, true, false),
            Choice::Pass
        );
        assert_eq!(
            g.decide(&Input::Transcript("hello".into()), Btn::Menu, true, false),
            Choice::Pass
        );
        assert_eq!(
            g.decide(&Input::Press(Btn::A), Btn::Menu, true, true),
            Choice::Eat
        );
        g.decide(&Input::Release(Btn::A), Btn::Menu, true, false);
        assert_eq!(
            g.decide(&Input::Press(Btn::A), Btn::Menu, true, false),
            Choice::Send
        );
        assert_eq!(
            g.decide(&Input::Press(Btn::X), Btn::Menu, true, true),
            Choice::Eat
        );
        g.decide(&Input::Release(Btn::X), Btn::Menu, true, false);
        assert_eq!(
            g.decide(&Input::Press(Btn::X), Btn::Menu, true, false),
            Choice::Delete
        );
        assert_eq!(
            g.decide(&Input::Press(Btn::Down), Btn::Menu, false, true),
            Choice::Scroll(true)
        );
        assert_eq!(
            g.decide(&Input::Disconnected, Btn::Menu, false, true),
            Choice::Pass
        );
        assert!(!g.open);
        assert!(g.held.is_empty());
        g.lost = true;
        assert_eq!(
            g.decide(&Input::Press(Btn::Menu), Btn::Menu, true, false),
            Choice::Pass
        );
        assert!(g.lost);
        assert_eq!(
            g.decide(&Input::Release(Btn::Menu), Btn::Menu, true, false),
            Choice::Pass
        );
    }
}

enum Done {
    Launch(u64, Result<(), String>),
    Preflight(u64, u64, Result<Snapshot, String>),
    Delivery(u64, bool, Result<(), String>),
}
/// The preview keeps its renderer passive; blocking CLI work never runs on the input loop.
pub struct VoicePreview {
    cancellation: Cancellation,
    state: PathBuf,
    publisher: Option<Publisher>,
    gate: InputGate,
    generation: Arc<AtomicU64>,
    revision: u64,
    scroll: usize,
    width: usize,
    last_view: Option<View>,
    displayed_draft: Option<u64>,
    pending_preflight: Option<(u64, u64)>,
    tx: mpsc::Sender<Done>,
    rx: mpsc::Receiver<Done>,
    workers: Vec<JoinHandle<()>>,
    notice: Option<String>,
}
impl VoicePreview {
    pub fn new(cancellation: Cancellation, state: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            cancellation,
            state,
            publisher: None,
            gate: InputGate::default(),
            generation: Arc::new(AtomicU64::new(0)),
            revision: 1,
            scroll: 0,
            width: 80,
            last_view: None,
            displayed_draft: None,
            pending_preflight: None,
            tx,
            rx,
            workers: vec![],
            notice: None,
        }
    }
    pub fn is_open(&self) -> bool {
        self.publisher.is_some()
    }
    pub fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }
    pub fn hide(&mut self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.publisher = None;
        self.gate.open = false;
        self.gate.lost = false;
        self.pending_preflight = None;
        self.last_view = None;
        self.displayed_draft = None;
    }
    fn hide_unavailable(&mut self) {
        self.hide();
        self.gate.lost = true;
    }
    pub fn open(&mut self, core: &Core, cfg: &Config) {
        if core.draft().is_none() {
            self.notice = Some("No voice draft to review".into());
            return;
        }
        if self.is_open() {
            self.publish(core, cfg);
            return;
        }
        self.scroll = 0;
        self.revision += 1;
        let view = self.view(core, cfg).expect("draft checked");
        match Publisher::new(&self.state, &view) {
            Ok(publisher) => {
                let directory = publisher.directory().to_path_buf();
                self.publisher = Some(publisher);
                self.last_view = Some(view);
                self.displayed_draft = core.draft().map(|d| d.id);
                self.gate.open = true;
                self.gate.lost = false;
                let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
                let current = self.generation.clone();
                let stop = self.cancellation.clone();
                let tx = self.tx.clone();
                self.workers.push(std::thread::spawn(move || {
                    let result = crate::herdr::run_with_cancellation(
                        &[
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
                            &format!("GAMEPAD_MENU_DIR={}", directory.display()),
                        ],
                        Duration::from_secs(10),
                        || stop.cancelled() || current.load(Ordering::SeqCst) != generation,
                    )
                    .map(|_| ());
                    let _ = tx.send(Done::Launch(generation, result));
                }));
            }
            Err(error) => {
                self.notice = Some(format!("Voice preview unavailable; draft kept: {error}"))
            }
        }
    }
    fn view(&self, core: &Core, cfg: &Config) -> Option<View> {
        let draft = core.draft()?;
        let status = if core.recording() {
            "Recording replacement…".into()
        } else if core.voice_busy() {
            "Transcribing replacement…".into()
        } else if self.pending_preflight.is_some() {
            "Checking destination…".into()
        } else {
            match &draft.state {
                DeliveryState::Ready if !draft.error.is_empty() => draft.error.clone(),
                DeliveryState::Ready => "Ready to send".into(),
                DeliveryState::Sending => "Sending…".into(),
                DeliveryState::Unknown(error) => {
                    format!("Delivery unknown; inspect agent before recording again: {error}")
                }
            }
        };
        let destination = draft
            .target
            .as_ref()
            .map(|p| {
                format!(
                    "{} [{}] / {}",
                    p.agent.as_deref().unwrap_or("No agent"),
                    p.pane_id,
                    p.workspace_id
                )
            })
            .unwrap_or("No captured agent".into());
        Some(View {
            voice_preview: Some(PreviewView {
                text: draft.text.clone(),
                destination,
                status,
                voice_button: cfg.labels.button(cfg.voice_button).into(),
            }),
            revision: self.revision,
            scope: String::new(),
            breadcrumb: "Voice message".into(),
            rows: vec![],
            selected: 0,
            details: vec![],
            status: String::new(),
            scroll: self.scroll,
            expanded: false,
            labels: cfg.labels,
            editor: None,
            menu_button: String::new(),
        })
    }
    fn publish(&mut self, core: &Core, cfg: &Config) {
        if !self.is_open() {
            return;
        }
        let id = core.draft().map(|d| d.id);
        if id != self.displayed_draft {
            self.scroll = 0;
            self.displayed_draft = id;
        }
        let Some(mut view) = self.view(core, cfg) else {
            self.hide();
            return;
        };
        if self.last_view.as_ref() == Some(&view) {
            return;
        }
        self.revision += 1;
        view.revision = self.revision;
        if let Err(error) = self.publisher.as_mut().unwrap().publish(&view) {
            self.notice = Some(format!("Voice preview unavailable; draft kept: {error}"));
            self.hide_unavailable();
        } else {
            self.last_view = Some(view);
        }
    }
    pub fn tick(&mut self, core: &mut Core, cfg: &Config) {
        while let Ok(done) = self.rx.try_recv() {
            match done {
                Done::Launch(generation, Err(error))
                    if generation == self.generation.load(Ordering::SeqCst) =>
                {
                    self.notice = Some(format!("Voice preview unavailable; draft kept: {error}"));
                    self.hide_unavailable();
                }
                Done::Preflight(id, generation, result)
                    if self.pending_preflight == Some((id, generation)) =>
                {
                    self.pending_preflight = None;
                    if !self.is_open() || !core.draft().is_some_and(|d| d.id == id) {
                        continue;
                    }
                    match result {
                        Ok(snapshot) => {
                            if let Some((id, action)) = core.draft_send(&snapshot) {
                                let draft = core.draft().unwrap().clone();
                                let stop = self.cancellation.clone();
                                let tx = self.tx.clone();
                                self.workers.push(std::thread::spawn(move || {
                                    let herdr = Herdr {
                                        runner: CancellableCli {
                                            timeout: Duration::from_secs(10),
                                            cancellation: stop.clone(),
                                        },
                                    };
                                    let checked = herdr
                                        .focused_snapshot()
                                        .and_then(|snapshot| draft.validate_target(&snapshot));
                                    let (attempted, result) =
                                        dispatch_checked(checked, stop.cancelled(), || {
                                            herdr.apply(&action)
                                        });
                                    let _ = tx.send(Done::Delivery(id, attempted, result));
                                }));
                            }
                        }
                        Err(error) => {
                            let message = format!("Destination unavailable; draft kept: {error}");
                            core.draft_preflight_failed(id, message.clone());
                            self.notice = Some(message);
                        }
                    }
                }
                Done::Delivery(id, true, result) => core.draft_delivery(id, result),
                Done::Delivery(id, false, Err(error)) => core.draft_rejected(id, error),
                _ => {}
            }
        }
        self.workers.retain(|worker| !worker.is_finished());
        if let Some(publisher) = &mut self.publisher {
            match publisher.poll() {
                Ok((_, width)) => {
                    if let Some(width) = width {
                        self.width = width.max(1);
                    }
                }
                Err(_) => self.hide_unavailable(),
            }
        }
        if let Some(view) = self.view(core, cfg).and_then(|v| v.voice_preview) {
            self.scroll = self.scroll.min(
                crate::menu_render::voice_body_lines(&view, self.width)
                    .len()
                    .saturating_sub(1),
            );
        }
        self.publish(core, cfg);
    }
    pub fn handle(&mut self, input: &Input, core: &mut Core, cfg: &Config) -> bool {
        if matches!(input, Input::Disconnected) {
            self.hide();
        }
        let ready = match self.publisher.as_mut().map(Publisher::poll) {
            Some(Ok((ack, _))) => ack == self.revision,
            Some(Err(_)) => {
                self.hide_unavailable();
                false
            }
            None => false,
        };
        let busy = core.voice_busy()
            || self.pending_preflight.is_some()
            || core
                .draft()
                .is_some_and(|d| d.state == DeliveryState::Sending);
        match self.gate.decide(input, cfg.voice_button, ready, busy) {
            Choice::Pass => false,
            Choice::Eat => true,
            Choice::Back => {
                self.hide();
                true
            }
            Choice::Delete => {
                if core.discard_draft() {
                    self.hide();
                }
                true
            }
            Choice::Scroll(down) => {
                let max = self
                    .view(core, cfg)
                    .and_then(|v| v.voice_preview)
                    .map(|v| {
                        crate::menu_render::voice_body_lines(&v, self.width)
                            .len()
                            .saturating_sub(1)
                    })
                    .unwrap_or(0);
                self.scroll = if down {
                    self.scroll.saturating_add(1).min(max)
                } else {
                    self.scroll.saturating_sub(1)
                };
                self.publish(core, cfg);
                true
            }
            Choice::Send => {
                if let Some(draft) = core.draft().filter(|d| d.state == DeliveryState::Ready) {
                    let id = draft.id;
                    let generation = self.generation.load(Ordering::SeqCst);
                    self.pending_preflight = Some((id, generation));
                    let stop = self.cancellation.clone();
                    let tx = self.tx.clone();
                    self.workers.push(std::thread::spawn(move || {
                        let result = Herdr {
                            runner: CancellableCli {
                                timeout: Duration::from_secs(10),
                                cancellation: stop,
                            },
                        }
                        .snapshot();
                        let _ = tx.send(Done::Preflight(id, generation, result));
                    }));
                    self.publish(core, cfg);
                }
                true
            }
        }
    }
}
fn dispatch_checked(
    checked: Result<(), String>,
    cancelled: bool,
    dispatch: impl FnOnce() -> Result<(), String>,
) -> (bool, Result<(), String>) {
    if let Err(error) = checked {
        return (false, Err(error));
    }
    if cancelled {
        return (false, Err("Stopped before sending; draft kept".into()));
    }
    (true, dispatch())
}
impl Drop for VoicePreview {
    fn drop(&mut self) {
        self.hide();
        self.cancellation.cancel();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod runtime_tests {
    use super::*;
    use crate::lifecycle::Daemon;
    use std::{fs, os::unix::io::AsRawFd};
    struct Harness {
        runtime: VoicePreview,
        core: Core,
        cfg: Config,
        _daemon: Daemon,
        lease: Option<fs::File>,
        path: PathBuf,
    }
    impl Drop for Harness {
        fn drop(&mut self) {
            self.runtime.hide();
            let _ = fs::remove_dir_all(&self.path);
        }
    }
    fn harness() -> Harness {
        static N: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "voice-preview-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let daemon = Daemon::acquire(&path).unwrap().unwrap();
        let cfg = crate::config::parse("[voice]\npreview_before_send=true", &path).unwrap();
        let mut core = Core::new(Btn::RStick);
        core.set_preview_enabled(true);
        let snap = crate::engine_tests::snap();
        core.handle(Input::Press(Btn::RStick), &snap);
        core.handle(Input::Release(Btn::RStick), &snap);
        core.handle(Input::Transcript("retained draft".into()), &snap);
        Harness {
            runtime: VoicePreview::new(daemon.cancellation(), path.clone()),
            core,
            cfg,
            _daemon: daemon,
            lease: None,
            path,
        }
    }
    fn show(h: &mut Harness) {
        let view = h.runtime.view(&h.core, &h.cfg).unwrap();
        let publisher = Publisher::new(&h.path, &view).unwrap();
        let lease = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(publisher.directory().join("viewer.lock"))
            .unwrap();
        assert_eq!(
            unsafe { libc::flock(lease.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
            0
        );
        fs::write(
            publisher.directory().join("ack"),
            h.runtime.revision.to_string(),
        )
        .unwrap();
        h.runtime.publisher = Some(publisher);
        h.runtime.gate.open = true;
        h.runtime.last_view = Some(view);
        h.runtime.displayed_draft = h.core.draft().map(|d| d.id);
        h.lease = Some(lease);
    }
    #[test]
    fn acknowledged_delete_removes_draft_but_stale_ack_and_busy_recording_keep_it() {
        let mut h = harness();
        show(&mut h);
        let dir = h
            .runtime
            .publisher
            .as_ref()
            .unwrap()
            .directory()
            .to_path_buf();
        fs::write(dir.join("ack"), "0").unwrap();
        assert!(h.runtime.handle(&Input::Press(Btn::X), &mut h.core, &h.cfg));
        assert!(h.core.draft().is_some());
        h.runtime
            .handle(&Input::Release(Btn::X), &mut h.core, &h.cfg);
        fs::write(dir.join("ack"), h.runtime.revision.to_string()).unwrap();
        h.core
            .handle(Input::Press(Btn::RStick), &Snapshot::default());
        assert!(h.runtime.handle(&Input::Press(Btn::X), &mut h.core, &h.cfg));
        assert!(h.core.draft().is_some());
        h.core
            .handle(Input::Release(Btn::RStick), &Snapshot::default());
        h.core.handle(
            Input::TranscriptFailed("failed".into()),
            &Snapshot::default(),
        );
        h.runtime
            .handle(&Input::Release(Btn::X), &mut h.core, &h.cfg);
        assert!(h.runtime.handle(&Input::Press(Btn::X), &mut h.core, &h.cfg));
        assert!(h.core.draft().is_none());
        assert!(!h.runtime.is_open());
    }
    #[test]
    fn hidden_draft_receives_delivery_and_stale_launch_cannot_close_new_popup() {
        let mut h = harness();
        show(&mut h);
        let (id, _) = h.core.draft_send(&crate::engine_tests::snap()).unwrap();
        h.runtime.hide();
        show(&mut h);
        h.runtime
            .tx
            .send(Done::Launch(0, Err("old popup".into())))
            .unwrap();
        h.runtime.tick(&mut h.core, &h.cfg);
        assert!(h.runtime.is_open());
        h.runtime.hide();
        h.runtime
            .tx
            .send(Done::Delivery(id, true, Err("lost reply".into())))
            .unwrap();
        h.runtime.tick(&mut h.core, &h.cfg);
        assert!(matches!(
            h.core.draft().unwrap().state,
            DeliveryState::Unknown(_)
        ));
        assert!(h.core.draft_send(&crate::engine_tests::snap()).is_none());
        h.core.discard_draft();
        h.runtime.tx.send(Done::Delivery(id, true, Ok(()))).unwrap();
        h.runtime.tick(&mut h.core, &h.cfg);
        assert!(!h.runtime.is_open());
    }
    #[test]
    fn popup_failure_preserves_text_and_preflight_is_cancelled_on_back() {
        let mut h = harness();
        show(&mut h);
        h.runtime.pending_preflight = Some((h.core.draft().unwrap().id, 0));
        h.runtime.handle(&Input::Press(Btn::B), &mut h.core, &h.cfg);
        assert!(!h.runtime.is_open());
        assert!(h.runtime.pending_preflight.is_none());
        let id = h.core.draft().unwrap().id;
        h.runtime
            .tx
            .send(Done::Preflight(id, 0, Ok(crate::engine_tests::snap())))
            .unwrap();
        h.runtime.tick(&mut h.core, &h.cfg);
        assert_eq!(h.core.draft().unwrap().state, DeliveryState::Ready);
        show(&mut h);
        let generation = h.runtime.generation.load(Ordering::SeqCst);
        h.runtime
            .tx
            .send(Done::Launch(generation, Err("launch failed".into())))
            .unwrap();
        h.runtime.tick(&mut h.core, &h.cfg);
        assert!(!h.runtime.is_open());
        assert_eq!(h.core.draft().unwrap().text, "retained draft");
        assert!(h.runtime.take_notice().unwrap().contains("draft kept"));
    }

    #[test]
    fn successful_delivery_closes_preview_without_leaking_the_send_release() {
        let mut h = harness();
        show(&mut h);
        assert_eq!(
            h.runtime
                .gate
                .decide(&Input::Press(Btn::A), Btn::RStick, true, false),
            Choice::Send
        );
        let (id, _) = h.core.draft_send(&crate::engine_tests::snap()).unwrap();
        h.runtime.tx.send(Done::Delivery(id, true, Ok(()))).unwrap();
        h.runtime.tick(&mut h.core, &h.cfg);
        assert!(h.core.draft().is_none());
        assert!(!h.runtime.is_open());
        assert!(h
            .runtime
            .handle(&Input::Release(Btn::A), &mut h.core, &h.cfg));
    }
    #[test]
    fn dead_viewer_consumes_a_button_that_was_held_before_opening() {
        let mut h = harness();
        assert!(!h.runtime.handle(&Input::Press(Btn::A), &mut h.core, &h.cfg));
        show(&mut h);
        h.runtime.tick(&mut h.core, &h.cfg);
        h.lease = None;
        h.runtime.tick(&mut h.core, &h.cfg);
        assert!(h.runtime.handle(&Input::Press(Btn::A), &mut h.core, &h.cfg));
        assert!(h
            .runtime
            .handle(&Input::Release(Btn::A), &mut h.core, &h.cfg));
        assert!(!h.runtime.handle(&Input::Press(Btn::A), &mut h.core, &h.cfg));
    }
    #[test]
    fn dead_viewer_does_not_leak_send_press_into_the_agent() {
        for detect_in_tick in [false, true] {
            let mut h = harness();
            show(&mut h);
            h.runtime.tick(&mut h.core, &h.cfg);
            h.lease = None;
            if detect_in_tick {
                h.runtime.tick(&mut h.core, &h.cfg);
            }
            assert!(
                h.runtime.handle(&Input::Press(Btn::A), &mut h.core, &h.cfg),
                "dead popup send leaked, tick={detect_in_tick}"
            );
            assert!(h
                .runtime
                .handle(&Input::Release(Btn::A), &mut h.core, &h.cfg));
            assert_eq!(h.core.draft().unwrap().state, DeliveryState::Ready);
            assert!(!h.runtime.handle(&Input::Press(Btn::A), &mut h.core, &h.cfg));
        }
    }
    #[test]
    fn replacing_a_scrolled_draft_returns_to_its_first_line() {
        let mut h = harness();
        show(&mut h);
        h.runtime.scroll = 4;
        let snap = crate::engine_tests::snap();
        h.core.handle(Input::Press(Btn::RStick), &snap);
        h.core.handle(Input::Release(Btn::RStick), &snap);
        h.core
            .handle(Input::Transcript("replacement ".repeat(50)), &snap);
        h.runtime.tick(&mut h.core, &h.cfg);
        assert_eq!(h.runtime.scroll, 0);
        assert!(h
            .runtime
            .last_view
            .as_ref()
            .unwrap()
            .voice_preview
            .as_ref()
            .unwrap()
            .text
            .starts_with("replacement"));
    }
    #[test]
    fn stale_preflight_after_reopen_cannot_touch_the_retained_draft() {
        let mut h = harness();
        show(&mut h);
        let id = h.core.draft().unwrap().id;
        h.runtime.pending_preflight = Some((id, 0));
        h.runtime.hide();
        show(&mut h);
        h.runtime
            .tx
            .send(Done::Preflight(id, 0, Ok(Snapshot::default())))
            .unwrap();
        h.runtime.tick(&mut h.core, &h.cfg);
        assert_eq!(h.core.draft().unwrap().error, "");
        assert_eq!(h.core.draft().unwrap().state, DeliveryState::Ready);
    }
    #[test]
    fn preflight_failure_shows_the_actual_reason_in_the_retained_preview() {
        let mut h=harness(); show(&mut h);
        let id=h.core.draft().unwrap().id;
        let generation=h.runtime.generation.load(Ordering::SeqCst);
        h.runtime.pending_preflight=Some((id,generation));
        h.runtime.tx.send(Done::Preflight(id,generation,Err("server offline".into()))).unwrap();
        h.runtime.tick(&mut h.core,&h.cfg);
        assert_eq!(h.core.draft().unwrap().error,"Destination unavailable; draft kept: server offline");
        assert!(h.runtime.view(&h.core,&h.cfg).unwrap().voice_preview.unwrap().status.contains("server offline"));
        assert_eq!(h.core.draft().unwrap().text,"retained draft");
    }
    #[test]
    fn dead_popup_allows_the_actions_button_to_reopen_immediately() {
        for voice in [Btn::RStick,Btn::Menu] {
            let mut gate=InputGate { lost:true,..Default::default() };
            let button=crate::engine::menu_button(voice);
            assert_eq!(gate.decide(&Input::Press(button),voice,false,false),Choice::Pass);
            assert_eq!(gate.decide(&Input::Release(button),voice,false,false),Choice::Pass);
            assert!(!gate.lost);
        }
    }
    #[test]
    fn failed_replacement_preserves_unknown_delivery_reason_in_the_view() {
        let mut h=harness(); show(&mut h);
        let snapshot=crate::engine_tests::snap();
        let (id,_)=h.core.draft_send(&snapshot).unwrap();
        h.core.draft_delivery(id,Err("original delivery error".into()));
        for completion in [Input::Transcript(" ".into()),Input::TranscriptFailed("bad audio".into())] {
            h.core.handle(Input::Press(Btn::RStick),&snapshot);
            h.core.handle(Input::Release(Btn::RStick),&snapshot);
            h.core.handle(completion,&snapshot);
            assert_eq!(h.core.draft().unwrap().state,DeliveryState::Unknown("original delivery error".into()));
            assert!(h.runtime.view(&h.core,&h.cfg).unwrap().voice_preview.unwrap().status.contains("original delivery error"));
        }
    }
    #[test]
    fn dispatch_requires_valid_target_and_live_daemon() {
        use std::cell::Cell;
        let calls = Cell::new(0);
        let send = || {
            calls.set(calls.get() + 1);
            Ok(())
        };
        assert_eq!(dispatch_checked(Ok(()), false, send), (true, Ok(())));
        assert_eq!(calls.get(), 1);
        assert_eq!(
            dispatch_checked(Err("target changed".into()), false, send),
            (false, Err("target changed".into()))
        );
        assert!(!dispatch_checked(Ok(()), true, send).0);
        assert_eq!(calls.get(), 1);
    }
}
