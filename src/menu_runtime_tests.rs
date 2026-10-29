use super::*;
use crate::lifecycle::Daemon;
use std::{fs, os::unix::io::AsRawFd};
struct Harness {
    runtime: MenuRuntime,
    core: Core,
    cfg: Config,
    reads: Receiver<ReadJob>,
    mutations: Receiver<Operation>,
    completions: mpsc::Sender<Completion>,
    results: mpsc::Sender<ReadDone>,
    _daemon: Daemon,
    dir: PathBuf,
}
impl Drop for Harness {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}
fn harness() -> Harness {
    static N: AtomicU64 = AtomicU64::new(0);
    let dir = std::env::temp_dir().join(format!(
        "runtime-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let daemon = Daemon::acquire(&dir).unwrap().unwrap();
    let (active, cfg) = crate::settings::load(&dir.join("config.toml"), &dir).unwrap();
    let (rt, rr) = mpsc::sync_channel(1);
    let (dt, dr) = mpsc::channel();
    let (mt, mr) = mpsc::sync_channel(1);
    let (ct, cr) = mpsc::channel();
    let workers = Workers {
        reads: Some(rt),
        mutations: Some(mt),
        results: dr,
        completed: cr,
        threads: vec![],
        interest: Arc::new(AtomicU64::new(0)),
        stop: daemon.cancellation(),
    };
    let runtime = MenuRuntime {
        workers,
        model: None,
        publisher: None,
        opening: false,
        notice: None,
        session: 0,
        sequence: 0,
        slot: OperationSlot::default(),
        active,
        committed: None,
        state: dir.clone(),
        config: dir.join("config.toml"),
        held: vec![],
        swallowed: vec![],
        repeat: None,
        read_busy: false,
        launch: None,
        queued_read: None,
        follow: None,
        last_reconcile: Instant::now(),
        last_context: Instant::now(),
    };
    Harness {
        runtime,
        core: Core::new(cfg.voice_button),
        cfg,
        reads: rr,
        mutations: mr,
        completions: ct,
        results: dt,
        _daemon: daemon,
        dir,
    }
}
fn input(h: &mut Harness, i: engine::Input) -> bool {
    h.runtime.handle(&i, &mut h.core, &mut h.cfg)
}
fn tick(h: &mut Harness) {
    h.runtime.tick(&mut h.core, &mut h.cfg);
}
fn open(h: &mut Harness) {
    assert!(input(h, engine::Input::Press(Btn::Menu)));
    assert!(matches!(
        h.reads.recv_timeout(Duration::from_millis(200)).unwrap(),
        ReadJob::Context(1)
    ));
    h.results.send(ReadDone::Context(1, Ok(None))).unwrap();
    tick(h);
    assert!(h.runtime.model.is_some());
    assert!(matches!(
        h.reads.recv_timeout(Duration::from_millis(200)).unwrap(),
        ReadJob::Launch(1, _)
    ));
    input(h, engine::Input::Release(Btn::Menu));
}
struct Lease(std::fs::File);
impl Drop for Lease {
    fn drop(&mut self) {
        assert_eq!(unsafe { libc::flock(self.0.as_raw_fd(), libc::LOCK_UN) }, 0);
    }
}
fn ack(h: &mut Harness) -> Lease {
    let p = h.runtime.publisher.as_ref().unwrap();
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(p.directory().join("viewer.lock"))
        .unwrap();
    assert_eq!(
        unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) },
        0
    );
    fs::write(
        p.directory().join("ack"),
        h.runtime.model.as_ref().unwrap().revision().to_string(),
    )
    .unwrap();
    Lease(file)
}
#[test]
fn slow_open_cancel_never_reaches_normal_mode_and_ignores_late_context() {
    let mut h = harness();
    assert!(input(&mut h, engine::Input::Press(Btn::Menu)));
    assert!(h.runtime.opening);
    assert!(input(&mut h, engine::Input::Press(Btn::B)));
    assert!(!h.runtime.is_open());
    assert!(input(&mut h, engine::Input::Release(Btn::B)));
    h.results.send(ReadDone::Context(1, Ok(None))).unwrap();
    tick(&mut h);
    assert!(h.runtime.model.is_none());
    assert_eq!(h.core.mode(), engine::Mode::Navigate);
}
#[test]
fn voice_busy_refuses_menu_and_disconnect_resets_held_state() {
    let mut h = harness();
    h.core.handle(
        engine::Input::Press(Btn::RStick),
        &engine::Snapshot::default(),
    );
    assert!(input(&mut h, engine::Input::Press(Btn::Menu)));
    assert!(!h.runtime.opening);
    assert!(h
        .runtime
        .take_notice()
        .unwrap()
        .contains("Finish dictation"));
    assert!(h.reads.try_recv().is_err());
    input(&mut h, engine::Input::Disconnected);
    assert!(h.runtime.held.is_empty());
    assert!(h.runtime.swallowed.is_empty());
}
#[test]
fn menu_consumes_shoulders_and_held_face_does_not_repeat_on_unrelated_release() {
    let mut h = harness();
    open(&mut h);
    let _lease = ack(&mut h);
    let before = h.runtime.model.as_ref().unwrap().revision();
    assert!(input(&mut h, engine::Input::Press(Btn::Rb)));
    assert_eq!(before, h.runtime.model.as_ref().unwrap().revision());
    input(&mut h, engine::Input::Press(Btn::Down));
    let selected = h.runtime.model.as_ref().unwrap().view().selected;
    input(&mut h, engine::Input::Release(Btn::Rb));
    input(&mut h, engine::Input::Press(Btn::Down));
    assert_eq!(selected, h.runtime.model.as_ref().unwrap().view().selected);
}
#[test]
fn committed_settings_survive_renderer_death_and_rebind_release_is_consumed() {
    let mut h = harness();
    open(&mut h);
    let lease = ack(&mut h);
    tick(&mut h);
    let mut saved = h.runtime.active.clone();
    saved.voice_button = Btn::Menu;
    saved.labels = LabelStyle::Xbox;
    let mut cfg = h.cfg.clone();
    cfg.voice_button = Btn::Menu;
    cfg.labels = LabelStyle::Xbox;
    h.completions
        .send(Completion {
            id: RequestId {
                session: 1,
                serial: 1,
            },
            outcome: Outcome::Success("Saved".into()),
            workspace: None,
            pane: None,
            worktree: None,
            committed: Some((saved, cfg)),
            conflict: None,
        })
        .unwrap();
    tick(&mut h);
    assert_eq!(h.cfg.voice_button, Btn::RStick);
    input(&mut h, engine::Input::Press(Btn::Menu));
    assert_eq!(h.cfg.voice_button, Btn::Menu);
    assert!(input(&mut h, engine::Input::Release(Btn::Menu)));
    assert!(!h.core.recording());
    drop(lease);
}
#[test]
fn renderer_death_closes_before_activation() {
    let mut h = harness();
    open(&mut h);
    let lease = ack(&mut h);
    tick(&mut h);
    drop(lease);
    assert!(input(&mut h, engine::Input::Press(Btn::A)));
    assert!(h.runtime.model.is_none());
    assert_eq!(h.core.mode(), engine::Mode::Navigate);
}
#[test]
fn late_create_success_after_dismissal_never_focuses() {
    let mut h = harness();
    open(&mut h);
    input(&mut h, engine::Input::Press(Btn::B));
    h.completions
        .send(Completion {
            id: RequestId {
                session: 1,
                serial: 1,
            },
            outcome: Outcome::Success("created".into()),
            workspace: Some("new".into()),
            pane: None,
            worktree: None,
            committed: None,
            conflict: None,
        })
        .unwrap();
    tick(&mut h);
    assert!(h.runtime.follow.is_none());
    assert!(h.reads.try_recv().is_err());
}
#[test]
fn newer_menu_cannot_inherit_a_queued_follow_focus() {
    let mut h = harness();
    h.runtime.follow = Some((
        "old-created-space".into(),
        None,
        Response::Mutation {
            id: RequestId {
                session: 0,
                serial: 1,
            },
            outcome: Outcome::Success("created".into()),
        },
    ));
    input(&mut h, engine::Input::Press(Btn::Menu));
    assert!(matches!(
        h.reads.recv_timeout(Duration::from_millis(200)).unwrap(),
        ReadJob::Context(1)
    ));
    assert!(h.runtime.follow.is_none());
}
#[test]
fn pending_slot_survives_popup_cancel_and_reopen() {
    let mut h = harness();
    open(&mut h);
    let operation = Operation {
        id: RequestId {
            session: 1,
            serial: 9,
        },
        context: None,
        expected_repository: None,
        expected_repository_root: None,
        command: Command::ToggleZoom,
    };
    assert!(h.runtime.slot.reserve(operation.clone()));
    input(&mut h, engine::Input::Press(Btn::B));
    input(&mut h, engine::Input::Release(Btn::B));
    // Release the cancelled launch's read lane, then reopen.
    h.results
        .send(ReadDone::Launch(1, Err("cancelled".into())))
        .unwrap();
    tick(&mut h);
    input(&mut h, engine::Input::Press(Btn::Menu));
    assert!(matches!(
        h.reads.recv_timeout(Duration::from_millis(200)).unwrap(),
        ReadJob::Context(2)
    ));
    h.results.send(ReadDone::Context(2, Ok(None))).unwrap();
    tick(&mut h);
    assert!(h.runtime.slot.blocked());
    assert!(h
        .runtime
        .model
        .as_ref()
        .unwrap()
        .view()
        .details
        .iter()
        .any(|s| s.contains("1:9")));
    let mut another = operation;
    another.id = RequestId {
        session: 2,
        serial: 1,
    };
    assert!(!h.runtime.slot.reserve(another));
}
#[test]
fn malformed_periodic_context_dismisses_and_consumes_held_activation() {
    let mut h = harness();
    open(&mut h);
    input(&mut h, engine::Input::Press(Btn::A));
    h.results
        .send(ReadDone::Context(1, Err("malformed snapshot".into())))
        .unwrap();
    tick(&mut h);
    assert!(!h.runtime.is_open());
    assert!(input(&mut h, engine::Input::Release(Btn::A)));
    assert_eq!(h.core.mode(), engine::Mode::Navigate);
}
#[test]
fn failed_popup_launch_closes_open_session() {
    let mut h = harness();
    open(&mut h);
    h.results
        .send(ReadDone::Launch(1, Err("unavailable".into())))
        .unwrap();
    tick(&mut h);
    assert!(!h.runtime.is_open());
    assert_eq!(h.runtime.workers.interest.load(Ordering::SeqCst), 0);
}
#[test]
fn voice_collision_uses_l3_and_menu_input_never_starts_recording() {
    let mut h = harness();
    h.cfg.voice_button = Btn::Menu;
    h.core.rebind(Btn::Menu);
    assert!(!input(&mut h, engine::Input::Press(Btn::Menu)));
    assert!(input(&mut h, engine::Input::Press(Btn::LStick)));
    assert!(h.runtime.opening);
    assert!(input(&mut h, engine::Input::Release(Btn::Menu)));
    assert!(!h.core.recording());
    assert!(input(&mut h, engine::Input::Release(Btn::LStick)));
    assert!(input(&mut h, engine::Input::Press(Btn::LStick)));
    assert!(!h.runtime.is_open());
}
#[test]
fn read_worker_shutdown_joins_before_daemon_lock_release() {
    let mut h = harness();
    let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let completed = finished.clone();
    let interest = h.runtime.workers.interest.clone();
    interest.store(1, Ordering::SeqCst);
    let (tx, rx) = mpsc::channel();
    h.runtime.workers.threads.push(std::thread::spawn(move || {
        tx.send(()).unwrap();
        while interest.load(Ordering::SeqCst) != 0 {
            std::thread::sleep(Duration::from_millis(1));
        }
        completed.store(true, Ordering::SeqCst);
    }));
    rx.recv_timeout(Duration::from_secs(1)).unwrap();
    let replacement = Workers::new(
        h._daemon.cancellation(),
        h.dir.join("unused.toml"),
        h.dir.clone(),
    );
    let workers = std::mem::replace(&mut h.runtime.workers, replacement);
    drop(workers);
    assert!(finished.load(Ordering::SeqCst));
    assert!(Daemon::acquire(&h.dir).unwrap().is_none());
}
#[test]
fn saved_config_applies_after_viewer_death_without_model_receiving_it() {
    let mut h = harness();
    open(&mut h);
    let lease = ack(&mut h);
    tick(&mut h);
    drop(lease);
    tick(&mut h);
    assert!(!h.runtime.is_open());
    let mut saved = h.runtime.active.clone();
    saved.voice_button = Btn::Menu;
    let mut cfg = h.cfg.clone();
    cfg.voice_button = Btn::Menu;
    h.completions
        .send(Completion {
            id: RequestId {
                session: 1,
                serial: 55,
            },
            outcome: Outcome::Success("saved".into()),
            workspace: None,
            pane: None,
            worktree: None,
            committed: Some((saved, cfg)),
            conflict: None,
        })
        .unwrap();
    tick(&mut h);
    assert_eq!(h.cfg.voice_button, Btn::Menu);
    assert!(h.runtime.committed.is_none());
}
#[test]
fn opening_leaves_respond_and_interact_and_ordinary_actions_pass_through_after_exit() {
    for agent in [true, false] {
        let mut h = harness();
        let mut snap = crate::engine_tests::snap();
        if !agent {
            snap.focused.as_mut().unwrap().agent = None;
        }
        h.core.handle(engine::Input::Press(Btn::A), &snap);
        assert_eq!(
            h.core.mode(),
            if agent {
                engine::Mode::Respond
            } else {
                engine::Mode::Interact
            }
        );
        assert!(input(&mut h, engine::Input::Press(Btn::Menu)));
        assert_eq!(h.core.mode(), engine::Mode::Navigate);
        assert!(input(&mut h, engine::Input::Press(Btn::B)));
        assert!(input(&mut h, engine::Input::Release(Btn::B)));
        assert!(!input(&mut h, engine::Input::Press(Btn::X)));
        assert_eq!(
            h.core.handle(engine::Input::Press(Btn::X), &snap),
            vec![engine::Action::ToggleZoom("w5:p1".into())]
        );
    }
}
#[test]
fn full_dictation_survives_both_menu_refusals_and_menu_opens_after_prompt() {
    let mut h = harness();
    let snap = crate::engine_tests::snap();
    assert!(!input(&mut h, engine::Input::Press(Btn::RStick)));
    assert_eq!(
        h.core.handle(engine::Input::Press(Btn::RStick), &snap),
        vec![engine::Action::StartRecording]
    );
    assert!(input(&mut h, engine::Input::Press(Btn::Menu)));
    assert!(h.core.recording());
    input(&mut h, engine::Input::Release(Btn::Menu));
    assert!(!input(&mut h, engine::Input::Release(Btn::RStick)));
    assert_eq!(
        h.core.handle(engine::Input::Release(Btn::RStick), &snap),
        vec![engine::Action::StopRecording]
    );
    assert!(input(&mut h, engine::Input::Press(Btn::Menu)));
    assert!(!h.runtime.opening);
    assert!(h.runtime.take_notice().unwrap().contains("dictation"));
    input(&mut h, engine::Input::Release(Btn::Menu));
    assert!(!input(&mut h, engine::Input::Transcript("hello".into())));
    assert!(
        matches!(h.core.handle(engine::Input::Transcript("hello".into()),&snap).first(),Some(engine::Action::Prompt{text,..}) if text=="hello")
    );
    assert!(input(&mut h, engine::Input::Press(Btn::Menu)));
    assert!(h.runtime.opening);
}
#[test]
fn every_voice_binding_remains_usable_after_menu_dismissal() {
    for voice in [Btn::RStick, Btn::LStick, Btn::Menu] {
        let mut h = harness();
        h.cfg.voice_button = voice;
        h.core.rebind(voice);
        let button = engine::menu_button(voice);
        assert!(input(&mut h, engine::Input::Press(button)));
        input(&mut h, engine::Input::Release(button));
        assert!(input(&mut h, engine::Input::Press(voice)));
        assert!(input(&mut h, engine::Input::Release(voice)));
        assert!(!h.core.recording());
        assert!(input(&mut h, engine::Input::Press(button)));
        assert!(input(&mut h, engine::Input::Release(button)));
        assert!(!input(&mut h, engine::Input::Press(voice)));
        assert_eq!(
            h.core
                .handle(engine::Input::Press(voice), &engine::Snapshot::default()),
            vec![engine::Action::StartRecording]
        );
    }
}
fn tap(h: &mut Harness, button: Btn) {
    if let Some(p) = &h.runtime.publisher {
        fs::write(
            p.directory().join("ack"),
            h.runtime.model.as_ref().unwrap().revision().to_string(),
        )
        .unwrap();
    }
    assert!(input(h, engine::Input::Press(button)));
    assert!(input(h, engine::Input::Release(button)));
}
fn open_captured(h: &mut Harness) {
    assert!(input(h, engine::Input::Press(Btn::Menu)));
    assert!(matches!(
        h.reads.recv_timeout(Duration::from_millis(200)).unwrap(),
        ReadJob::Context(1)
    ));
    h.results
        .send(ReadDone::Context(
            1,
            Ok(Some(Context {
                workspace: "w".into(),
                tab: "t".into(),
                pane: "p".into(),
                workspace_label: "Space".into(),
                pane_label: "Pane".into(),
                cwd: Some("/repo".into()),
            })),
        ))
        .unwrap();
    tick(h);
    assert!(matches!(
        h.reads.recv_timeout(Duration::from_millis(200)).unwrap(),
        ReadJob::Launch(1, _)
    ));
    h.results.send(ReadDone::Launch(1, Ok(()))).unwrap();
    input(h, engine::Input::Release(Btn::Menu));
    tick(h);
}
#[test]
fn physical_controller_text_editing_and_navigation_dispatch_one_frozen_operation() {
    let mut h = harness();
    open_captured(&mut h);
    let _lease = ack(&mut h);
    tap(&mut h, Btn::A);
    assert!(h
        .runtime
        .model
        .as_ref()
        .unwrap()
        .view()
        .breadcrumb
        .ends_with("New space"));
    tap(&mut h, Btn::A);
    tap(&mut h, Btn::X);
    tap(&mut h, Btn::Y);
    tap(&mut h, Btn::Right);
    tap(&mut h, Btn::A);
    assert_eq!(
        h.runtime
            .model
            .as_ref()
            .unwrap()
            .view()
            .editor
            .as_ref()
            .unwrap()
            .buffer,
        "New spacB"
    );
    for _ in 0..5 {
        tap(&mut h, Btn::Down);
    }
    tap(&mut h, Btn::A);
    assert!(h.runtime.model.as_ref().unwrap().view().editor.is_none());
    tap(&mut h, Btn::Down);
    tap(&mut h, Btn::Up);
    assert_eq!(h.runtime.model.as_ref().unwrap().view().selected, 0);
    tap(&mut h, Btn::Down);
    tap(&mut h, Btn::A);
    let operation = h
        .mutations
        .recv_timeout(Duration::from_millis(200))
        .unwrap();
    assert_eq!(
        operation.command,
        Command::CreateSpace {
            name: "New spacB".into(),
            cwd: "/repo".into()
        }
    );
    assert_eq!(operation.context.unwrap().pane, "p");
    tap(&mut h, Btn::A);
    assert!(h.mutations.try_recv().is_err());
    assert!(h.runtime.slot.blocked());
}
#[test]
fn dpad_repeats_only_after_delay_and_matching_release_stops_it() {
    let mut h = harness();
    open(&mut h);
    let _lease = ack(&mut h);
    input(&mut h, engine::Input::Press(Btn::Down));
    assert_eq!(h.runtime.model.as_ref().unwrap().view().selected, 1);
    tick(&mut h);
    assert_eq!(h.runtime.model.as_ref().unwrap().view().selected, 1);
    h.runtime.repeat = Some((Btn::Down, Instant::now() - Duration::from_millis(1)));
    tick(&mut h);
    assert_eq!(h.runtime.model.as_ref().unwrap().view().selected, 2);
    assert!(h.runtime.repeat.unwrap().1 > Instant::now());
    input(&mut h, engine::Input::Release(Btn::A));
    assert!(h.runtime.repeat.is_some());
    input(&mut h, engine::Input::Release(Btn::Down));
    assert!(h.runtime.repeat.is_none());
}
#[test]
fn stale_launch_failure_and_old_context_cannot_close_new_session() {
    let mut h = harness();
    open(&mut h);
    tap(&mut h, Btn::B);
    h.results.send(ReadDone::Launch(1, Ok(()))).unwrap();
    tick(&mut h);
    input(&mut h, engine::Input::Press(Btn::Menu));
    assert!(matches!(
        h.reads.recv_timeout(Duration::from_millis(200)).unwrap(),
        ReadJob::Context(2)
    ));
    h.results
        .send(ReadDone::Launch(1, Err("old error".into())))
        .unwrap();
    h.results
        .send(ReadDone::Context(1, Err("old error".into())))
        .unwrap();
    tick(&mut h);
    assert!(h.runtime.opening);
    assert!(h.runtime.take_notice().is_none());
}
#[test]
fn back_from_submenu_does_not_act_like_opening_cancel() {
    let mut h = harness();
    open_captured(&mut h);
    let _lease = ack(&mut h);
    tap(&mut h, Btn::Right);
    assert!(h
        .runtime
        .model
        .as_ref()
        .unwrap()
        .view()
        .breadcrumb
        .ends_with("New space"));
    tap(&mut h, Btn::B);
    assert!(h.runtime.is_open());
    assert_eq!(h.runtime.model.as_ref().unwrap().view().breadcrumb, "Main");
    tap(&mut h, Btn::Right);
    tap(&mut h, Btn::Left);
    assert_eq!(h.runtime.model.as_ref().unwrap().view().breadcrumb, "Main");
}
#[test]
fn successful_create_waits_for_live_context_focus_then_closes_and_retains_result() {
    let mut h = harness();
    open_captured(&mut h);
    let _lease = ack(&mut h);
    tap(&mut h, Btn::A);
    tap(&mut h, Btn::Down);
    tap(&mut h, Btn::A);
    let operation = h
        .mutations
        .recv_timeout(Duration::from_millis(200))
        .unwrap();
    h.completions
        .send(Completion {
            id: operation.id,
            outcome: Outcome::Success("Created /actual/path".into()),
            workspace: Some("created".into()),
            pane: Some("created:p".into()),
            worktree: Some("/actual/path".into()),
            committed: None,
            conflict: None,
        })
        .unwrap();
    tick(&mut h);
    assert!(h.runtime.model.as_ref().unwrap().is_open());
    assert!(matches!(
        h.runtime.slot.state(),
        Some(OperationState::Finished(_))
    ));
    let response = match h.reads.recv_timeout(Duration::from_millis(200)).unwrap() {
        ReadJob::Focus(session, workspace, Some(context), response) => {
            assert_eq!(session, 1);
            assert_eq!(workspace, "created");
            assert_eq!(context.pane, "p");
            response
        }
        _ => panic!("expected guarded focus"),
    };
    h.results.send(ReadDone::Focus(response, Ok(()))).unwrap();
    tick(&mut h);
    assert!(!h.runtime.is_open());
    assert_eq!(h.core.mode(), engine::Mode::Navigate);
    assert_eq!(h.runtime.workers.interest.load(Ordering::SeqCst), 0);
}
#[test]
fn read_responses_reach_current_screen_and_periodic_context_reads_are_bounded() {
    let mut h = harness();
    open_captured(&mut h);
    let _lease = ack(&mut h);
    tap(&mut h, Btn::Down);
    tap(&mut h, Btn::A);
    let read = match h.reads.recv_timeout(Duration::from_millis(200)).unwrap() {
        ReadJob::Read(r) => r,
        _ => panic!("expected repository read"),
    };
    assert!(h
        .runtime
        .model
        .as_ref()
        .unwrap()
        .view()
        .status
        .contains("Loading"));
    h.results
        .send(ReadDone::Read(Response::Read {
            id: read.id,
            result: Err("not a repository".into()),
        }))
        .unwrap();
    tick(&mut h);
    assert_eq!(
        h.runtime.model.as_ref().unwrap().view().status,
        "not a repository"
    );
    h.runtime.last_context = Instant::now() - Duration::from_secs(1);
    tick(&mut h);
    assert!(matches!(
        h.reads.recv_timeout(Duration::from_millis(200)).unwrap(),
        ReadJob::Context(1)
    ));
    tick(&mut h);
    assert!(h.reads.try_recv().is_err());
}
#[test]
fn unknown_reconciliation_is_scheduled_without_retry_and_definitive_result_reaches_model() {
    let mut h = harness();
    open_captured(&mut h);
    let _lease = ack(&mut h);
    for _ in 0..3 {
        tap(&mut h, Btn::Down);
    }
    tap(&mut h, Btn::A);
    tap(&mut h, Btn::A);
    tap(&mut h, Btn::Down);
    tap(&mut h, Btn::A);
    let operation = h
        .mutations
        .recv_timeout(Duration::from_millis(200))
        .unwrap();
    assert_eq!(operation.command, Command::ClosePane);
    h.completions
        .send(Completion {
            id: operation.id,
            outcome: Outcome::Unknown("lost response".into()),
            workspace: None,
            pane: None,
            worktree: None,
            committed: None,
            conflict: None,
        })
        .unwrap();
    tick(&mut h);
    assert!(h.runtime.slot.blocked());
    assert!(
        h.reads.try_recv().is_err(),
        "reconciliation waits for its interval"
    );
    h.runtime.last_reconcile = Instant::now() - Duration::from_secs(3);
    tick(&mut h);
    assert!(matches!(
        h.reads.recv_timeout(Duration::from_millis(200)).unwrap(),
        ReadJob::Reconcile(1, _)
    ));
    h.results
        .send(ReadDone::Reconcile(
            operation.id,
            Outcome::Unknown("inconclusive".into()),
        ))
        .unwrap();
    tick(&mut h);
    assert!(h.runtime.slot.blocked());
    assert!(h.runtime.model.as_ref().unwrap().pending().is_some());
    h.results
        .send(ReadDone::Reconcile(
            operation.id,
            Outcome::Success("confirmed absent".into()),
        ))
        .unwrap();
    tick(&mut h);
    assert!(!h.runtime.slot.blocked());
    assert!(!h.runtime.is_open());
    assert!(h.mutations.try_recv().is_err());
}
#[test]
fn two_saves_use_committed_version_and_apply_together_only_on_exit() {
    let mut h = harness();
    open(&mut h);
    let _lease = ack(&mut h);
    tap_label(&mut h, "Settings");
    tap(&mut h, Btn::A);
    tap_label(&mut h, "Save");
    let operation = h
        .mutations
        .recv_timeout(Duration::from_millis(200))
        .unwrap();
    let mut saved = match operation.command {
        Command::SaveSettings { updated, .. } => updated,
        _ => panic!("expected save"),
    };
    saved.version = "committed-v2".into();
    let mut cfg = h.cfg.clone();
    cfg.labels = saved.labels;
    h.completions
        .send(Completion {
            id: operation.id,
            outcome: Outcome::Success("saved".into()),
            workspace: None,
            pane: None,
            worktree: None,
            committed: Some((saved.clone(), cfg)),
            conflict: None,
        })
        .unwrap();
    tick(&mut h);
    assert_eq!(
        h.runtime.model.as_ref().unwrap().settings().version,
        "committed-v2"
    );
    assert_eq!(h.cfg.labels, LabelStyle::Both);
    tap_label(&mut h, "Voice button:");
    tap_label(&mut h, "Save");
    let second = h
        .mutations
        .recv_timeout(Duration::from_millis(200))
        .unwrap();
    let mut next = match second.command {
        Command::SaveSettings { original, updated } => {
            assert_eq!(original.version, "committed-v2");
            updated
        }
        _ => panic!("expected save"),
    };
    next.version = "committed-v3".into();
    let mut cfg = h.cfg.clone();
    cfg.labels = next.labels;
    cfg.voice_button = next.voice_button;
    h.completions
        .send(Completion {
            id: second.id,
            outcome: Outcome::Success("saved".into()),
            workspace: None,
            pane: None,
            worktree: None,
            committed: Some((next, cfg)),
            conflict: None,
        })
        .unwrap();
    tick(&mut h);
    assert_eq!(h.cfg.voice_button, Btn::RStick);
    tap(&mut h, Btn::View);
    assert_eq!(h.cfg.labels, LabelStyle::PlayStation);
    assert_eq!(h.cfg.voice_button, Btn::LStick);
    assert_eq!(h.runtime.active.version, "committed-v3");
}
#[test]
fn runtime_keeps_external_disk_baseline_separate_from_active_controls() {
    let mut h = harness();
    fs::write(
        &h.runtime.config,
        "[voice]\nbutton='menu'\n[ui]\nlabels='xbox'\n",
    )
    .unwrap();
    open(&mut h);
    assert_eq!(
        h.runtime.model.as_ref().unwrap().settings().voice_button,
        Btn::Menu
    );
    assert_eq!(
        h.runtime.model.as_ref().unwrap().settings().labels,
        LabelStyle::Xbox
    );
    assert_eq!(
        h.runtime.model.as_ref().unwrap().view().menu_button,
        "Options/Menu"
    );
    assert_eq!(
        h.runtime.model.as_ref().unwrap().view().labels,
        LabelStyle::Both
    );
}
#[test]
fn a_conflict_reloads_form_but_never_becomes_active_configuration() {
    let mut h = harness();
    open(&mut h);
    let _lease = ack(&mut h);
    tap_label(&mut h, "Settings");
    tap_label(&mut h, "Save");
    let operation = h
        .mutations
        .recv_timeout(Duration::from_millis(200))
        .unwrap();
    let mut current = h.runtime.active.clone();
    current.version = "external".into();
    current.voice_button = Btn::Menu;
    h.completions
        .send(Completion {
            id: operation.id,
            outcome: Outcome::Failed("conflict".into()),
            workspace: None,
            pane: None,
            worktree: None,
            committed: None,
            conflict: Some(current),
        })
        .unwrap();
    tick(&mut h);
    assert_eq!(
        h.runtime.model.as_ref().unwrap().settings().version,
        "external"
    );
    assert!(h
        .runtime
        .model
        .as_ref()
        .unwrap()
        .view()
        .status
        .contains("changed externally"));
    tap(&mut h, Btn::View);
    assert_eq!(h.cfg.voice_button, Btn::RStick);
    assert_ne!(h.runtime.active.version, "external");
}
#[test]
fn session_reader_runs_only_for_current_live_session_and_cancels_slow_reads() {
    let h = harness();
    let interest = Arc::new(AtomicU64::new(7));
    let runner = SessionRunner {
        stop: h._daemon.cancellation(),
        interest: interest.clone(),
        session: 7,
    };
    assert_eq!(
        runner.run(&["/bin/echo", "reader-ready"]).unwrap(),
        "reader-ready\n"
    );
    interest.store(8, Ordering::SeqCst);
    assert_eq!(
        runner.run(&["/bin/echo", "must-not-run"]).unwrap_err(),
        "command cancelled"
    );
    interest.store(7, Ordering::SeqCst);
    let cancellation = h._daemon.cancellation();
    let thread = std::thread::spawn(move || runner.run(&["/bin/sh", "-c", "sleep 5"]));
    std::thread::sleep(Duration::from_millis(30));
    let start = Instant::now();
    cancellation.cancel();
    assert_eq!(thread.join().unwrap().unwrap_err(), "command cancelled");
    assert!(start.elapsed() < Duration::from_secs(1));
}
#[test]
fn transcript_completions_passthrough_even_during_menu_session() {
    let mut h = harness();
    open(&mut h);
    assert!(!input(&mut h, engine::Input::Transcript("done".into())));
    assert!(!input(
        &mut h,
        engine::Input::TranscriptFailed("error".into())
    ));
    assert!(h.runtime.is_open());
}
#[test]
fn horizontal_navigation_is_single_press_but_editor_cursor_can_repeat() {
    let mut h = harness();
    open_captured(&mut h);
    let _lease = ack(&mut h);
    input(&mut h, engine::Input::Press(Btn::Right));
    assert!(h
        .runtime
        .model
        .as_ref()
        .unwrap()
        .view()
        .breadcrumb
        .ends_with("New space"));
    assert!(
        h.runtime.repeat.is_none(),
        "submenu Right must not auto-descend"
    );
    input(&mut h, engine::Input::Release(Btn::Right));
    tap(&mut h, Btn::A);
    let p = h.runtime.publisher.as_ref().unwrap();
    fs::write(
        p.directory().join("ack"),
        h.runtime.model.as_ref().unwrap().revision().to_string(),
    )
    .unwrap();
    input(&mut h, engine::Input::Press(Btn::Right));
    assert!(h.runtime.repeat.is_some(), "text cursor still repeats");
    input(&mut h, engine::Input::Release(Btn::Right));
    tap(&mut h, Btn::B);
    input(&mut h, engine::Input::Press(Btn::Left));
    assert_eq!(h.runtime.model.as_ref().unwrap().view().breadcrumb, "Main");
    assert!(
        h.runtime.repeat.is_none(),
        "Left must not back through all screens"
    );
}
#[test]
fn horizontal_repeat_is_retired_when_editor_is_cancelled_while_direction_held() {
    let mut h = harness();
    open_captured(&mut h);
    let _lease = ack(&mut h);
    tap(&mut h, Btn::Right);
    tap(&mut h, Btn::A);
    input(&mut h, engine::Input::Press(Btn::Left));
    assert!(h.runtime.repeat.is_some());
    tap(&mut h, Btn::B);
    h.runtime.repeat = Some((Btn::Left, Instant::now() - Duration::from_millis(1)));
    h.runtime.tick(&mut h.core, &mut h.cfg);
    assert!(h
        .runtime
        .model
        .as_ref()
        .unwrap()
        .view()
        .breadcrumb
        .ends_with("New space"));
    assert!(h.runtime.repeat.is_none());
}

#[test]
fn late_settings_save_waits_for_recording_and_transcription_before_rebinding() {
    for completion in [
        engine::Input::Transcript("hello".into()),
        engine::Input::TranscriptFailed("cancelled".into()),
    ] {
        let mut h = harness();
        open(&mut h);
        input(&mut h, engine::Input::Press(Btn::Menu));
        input(&mut h, engine::Input::Release(Btn::Menu));
        assert!(!input(&mut h, engine::Input::Press(Btn::RStick)));
        assert_eq!(
            h.core.handle(
                engine::Input::Press(Btn::RStick),
                &engine::Snapshot::default()
            ),
            vec![engine::Action::StartRecording]
        );
        let mut saved = h.runtime.active.clone();
        saved.voice_button = Btn::LStick;
        saved.max_seconds = 30;
        let mut cfg = h.cfg.clone();
        cfg.voice_button = Btn::LStick;
        cfg.max_seconds = 30;
        h.completions
            .send(Completion {
                id: RequestId {
                    session: 1,
                    serial: 1,
                },
                outcome: Outcome::Success("Saved".into()),
                workspace: None,
                pane: None,
                worktree: None,
                committed: Some((saved, cfg)),
                conflict: None,
            })
            .unwrap();
        tick(&mut h);
        assert_eq!(
            h.cfg.voice_button,
            Btn::RStick,
            "a late save must not rebind a recording"
        );
        assert_eq!(h.cfg.max_seconds, 60, "runtime settings activate together");
        assert!(!input(&mut h, engine::Input::Release(Btn::RStick)));
        assert_eq!(
            h.core.handle(
                engine::Input::Release(Btn::RStick),
                &engine::Snapshot::default()
            ),
            vec![engine::Action::StopRecording]
        );
        tick(&mut h);
        assert_eq!(
            h.cfg.voice_button,
            Btn::RStick,
            "keep the recording's configuration through transcription"
        );
        assert!(!input(&mut h, completion.clone()));
        h.core.handle(completion, &engine::Snapshot::default());
        tick(&mut h);
        assert_eq!(h.cfg.voice_button, Btn::LStick);
        assert_eq!(h.cfg.max_seconds, 30);
        assert_eq!(h.runtime.active.voice_button, Btn::LStick);
        assert!(h.runtime.committed.is_none());
        assert!(!input(&mut h, engine::Input::Press(Btn::LStick)));
        assert_eq!(
            h.core.handle(
                engine::Input::Press(Btn::LStick),
                &engine::Snapshot::default()
            ),
            vec![engine::Action::StartRecording]
        );
    }
}

#[test]
fn preview_preference_activates_on_exit_and_off_restores_automatic_sending() {
    let mut h = harness();
    open(&mut h);
    let mut updated = h.runtime.active.clone();
    updated.preview_before_send = true;
    let saved = crate::settings::save(&h.runtime.config, &h.runtime.state, &h.runtime.active, &updated).unwrap();
    h.runtime.committed = Some(saved);
    tick(&mut h);
    assert!(!h.cfg.preview_before_send, "saved settings wait for menu exit");
    h.runtime.close(&mut h.core, &mut h.cfg);
    assert!(h.cfg.preview_before_send);
    let snap = crate::engine_tests::snap();
    h.core.handle(engine::Input::Press(Btn::RStick), &snap);
    h.core.handle(engine::Input::Release(Btn::RStick), &snap);
    let actions = h.core.handle(engine::Input::Transcript("keep this draft".into()), &snap);
    assert!(matches!(actions.first(), Some(engine::Action::InsertText { text, .. }) if text == "keep this draft"));
    let engine::Action::InsertText { target, .. } = actions.into_iter().next().unwrap() else { unreachable!() };
    h.core.insertion_result(target, Ok(()));
    let mut updated = h.runtime.active.clone();
    updated.preview_before_send = false;
    let saved = crate::settings::save(&h.runtime.config, &h.runtime.state, &h.runtime.active, &updated).unwrap();
    h.runtime.committed = Some(saved);
    tick(&mut h);
    assert!(!h.cfg.preview_before_send);
    assert!(h.mutations.try_recv().is_err());
    h.core.handle(engine::Input::Press(Btn::RStick), &snap);
    h.core.handle(engine::Input::Release(Btn::RStick), &snap);
    let actions = h.core.handle(engine::Input::Transcript("automatic again".into()), &snap);
    assert!(matches!(actions.first(), Some(engine::Action::Prompt { text, .. }) if text == "automatic again"));
}

fn tap_label(h: &mut Harness, label: &str) {
    let index = h.runtime.model.as_ref().unwrap().view().rows.iter().position(|r| r.label.starts_with(label)).expect("runtime menu row");
    for _ in 0..30 { tap(h, Btn::Up); }
    for _ in 0..index { tap(h, Btn::Down); }
    tap(h, Btn::A);
}

#[test]
fn disabling_preview_explains_that_native_editor_text_remains_unsent() {
    for (before, enabled, expect_notice) in [(false, false, false), (true, false, true), (false, true, false), (true, true, false)] {
        let mut h = harness();
        let mut initial = h.runtime.active.clone();
        initial.preview_before_send = before;
        h.runtime.committed = Some(crate::settings::save(&h.runtime.config, &h.runtime.state, &h.runtime.active, &initial).unwrap());
        tick(&mut h);
        h.runtime.take_notice();
        open(&mut h);
        let mut updated = h.runtime.active.clone();
        updated.preview_before_send = enabled;
        let saved = crate::settings::save(&h.runtime.config, &h.runtime.state, &h.runtime.active, &updated).unwrap();
        h.runtime.committed = Some(saved);
        tick(&mut h);
        assert!(h.runtime.take_notice().is_none(), "settings have not activated while menu is open");
        h.runtime.close(&mut h.core, &mut h.cfg);
        let notice = h.runtime.take_notice();
        assert_eq!(notice.as_deref(), expect_notice.then_some("Preview off. Existing input remains unsent; clear or submit it with the keyboard before dictating again."));
        assert!(h.runtime.take_notice().is_none(), "notice is consumed once");
    }
}

#[test]
fn agent_pages_preference_activates_only_after_saved_menu_exit() {
    for (before, enabled) in [(false, true), (true, false)] {
        let mut h = harness();
        let mut initial = h.runtime.active.clone();
        initial.agent_pages = before;
        h.runtime.committed = Some(crate::settings::save(&h.runtime.config, &h.runtime.state, &h.runtime.active, &initial).unwrap());
        tick(&mut h);
        open(&mut h);
        let mut updated = h.runtime.active.clone();
        updated.agent_pages = enabled;
        let saved = crate::settings::save(&h.runtime.config, &h.runtime.state, &h.runtime.active, &updated).unwrap();
        h.runtime.committed = Some(saved);
        tick(&mut h);
        assert_eq!(h.cfg.agent_pages, before, "save waits for menu exit");
        h.runtime.close(&mut h.core, &mut h.cfg);
        assert_eq!(h.cfg.agent_pages, enabled);
        assert_eq!(h.runtime.active.agent_pages, enabled);
        assert!(h.mutations.try_recv().is_err());
    }
}
