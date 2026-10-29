//! The daemon: controller events in, herdr commands out, with push-to-talk transcribed off-thread.

use crate::config::Config;
use crate::feedback::Feedback;
use crate::engine::*;
use crate::herdr::{Cli, CancellableCli, Herdr, Runner};
use crate::lifecycle::{Daemon, Cancellation};
use crate::pad::{self, Pad};
use crate::voice::{self, Recorder};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

pub fn serve() -> Result<(), String> {
    let (state, _) = crate::config::dirs();
    // Acquire ownership before opening the shared rotating log.
    let Some(daemon) = Daemon::acquire(&state)? else { println!("herd-gamepad already running"); return Ok(()); };
    crate::logging::init(&state)?;
    let (mut cfg, state) = crate::config::load()?;
    if !cfg.model.exists() {
        return Err(format!("whisper model missing at {}; run: herd-gamepad setup", cfg.model.display()));
    }
    let cancellation = daemon.cancellation();
    let herdr = Herdr { runner: CancellableCli { timeout: Duration::from_secs(10), cancellation: cancellation.clone() } };
    let mut worker = Transcriber { cancellation: cancellation.clone(), thread: None };
    let mut gilrs = gilrs::Gilrs::new().map_err(|e| format!("gilrs: {e}"))?;
    let mut pad = Pad::new();
    let mut stick = crate::scrolling::Stick::default();
    let scroll = crate::scrolling::Worker::new();
    let mut core = Core::new(cfg.voice_button);
    core.set_preview_enabled(cfg.preview_before_send);
    let mut feedback = Feedback::default();
    let mut menu = crate::menu_runtime::MenuRuntime::new(cancellation.clone(), state.clone(), crate::config::dirs().1.join("config.toml"))?;
    let mut recorder: Option<Recorder> = None;
    let (tx, rx) = mpsc::channel::<Input>();
    crate::logging::event(format_args!("herd-gamepad: listening; {} controller(s) connected; hold {} to talk", gilrs.gamepads().count(), cfg.voice_button.label()));
    while !cancellation.cancelled() {
        core.begin_tick();
        menu.tick(&mut core, &mut cfg);
        publish_scrolling(&mut stick, &scroll, menu.is_open() || core.voice_busy());
        if let Some(message) = scroll.take_notice() {
            crate::logging::event(format_args!("herd-gamepad: {message}"));
            feedback.emit(&herdr, &toast("Joystick scrolling", &message, Sound::None));
        }
        if let Some(message)=menu.take_notice(){feedback.emit(&herdr,&toast("Gamepad actions",&message,Sound::Request));}
        let inputs = next_inputs(|| {
            gilrs.next_event()
                .map(|ev| pad::raw(&ev.event).and_then(|r| pad.input(r)))
        }, &rx);
        for mut input in inputs {
            if cancellation.cancelled() { break; }
            let is_scroll = observe_scrolling(&mut stick, &input, menu.is_open() || core.voice_busy());
            publish_scrolling(&mut stick, &scroll, menu.is_open() || core.voice_busy());
            if is_scroll { continue; }
            if menu.handle(&input, &mut core, &mut cfg) {
                if let Some(message)=menu.take_notice(){feedback.emit(&herdr,&toast("Gamepad actions",&message,Sound::Request));}
                continue;
            }
            let snap = match needs_snapshot(&input, cfg.voice_button).then(|| herdr.snapshot()) {
                None => Snapshot::default(),
                Some(Ok(s)) => s,
                Some(Err(e)) => {
                    crate::logging::event(format_args!("herd-gamepad: snapshot: {e}"));
                    feedback.emit(&herdr,&toast("Herdr unavailable", &e, Sound::Request));
                    match on_snapshot_failure(&mut core, input, &e) {
                        Some(i) => input = i,
                        None => continue,
                    }
                    Snapshot::default()
                }
            };
            if cancellation.cancelled() { break; }
            let description = describe_for_log(&input, &core);
            crate::logging::event(format_args!("herd-gamepad: {} in {:?}", description, core.mode()));
            let actions=core.handle(input,&snap);
            let mut recording_error=None;
            let insertions=apply_actions(&herdr, actions, &mut feedback, |action| {
                match action {
                    Action::StartRecording => match Recorder::start(cfg.max_seconds) {
                        Ok(r) => recorder = Some(r),
                        Err(e) => {
                            crate::logging::event(format_args!("herd-gamepad: record: {e}"));
                            recording_error=Some(e);
                        }
                    },
                    Action::StopRecording => match recorder.take() {
                        Some(r) => worker.spawn(r.stop(), &cfg, &state, tx.clone()),
                        None => { let _ = tx.send(Input::TranscriptFailed("recording did not start".into())); }
                    },
                    Action::CancelRecording => drop(recorder.take()),
                    _ => unreachable!("only recording actions are delegated"),
                }
            });
            for outcome in insertions {
                let actions=finish_native(&mut core,outcome);
                for action in actions { feedback.emit(&herdr,&action); }
            }
            if let Some(e)=recording_error {
                core.handle(Input::TranscriptFailed(e.clone()), &snap);
                feedback.emit(&herdr,&toast("Recording failed",&e,Sound::Request));
            }
        }
    }
    drop(scroll);
    drop(menu);
    drop(recorder);
    drop(worker); // Join/cancel transcription before acknowledging Stop by releasing daemon's lock.
    crate::logging::event(format_args!("herd-gamepad: stopped"));
    Ok(())
}

/// One daemon tick: completed voice, then at most 64 nonblocking controller events.
/// The poll returns None for an empty queue, Some(None) for an ignored raw event.
pub fn next_inputs(mut poll: impl FnMut() -> Option<Option<Input>>, rx: &mpsc::Receiver<Input>) -> Vec<Input> {
    // A finished transcript clears transcribing before a simultaneous new voice press.
    let mut inputs: Vec<Input> = rx.try_iter().collect();
    // An ignored axis event is still work in the controller queue. Drain a bounded
    // batch rather than sleeping after each one; keep a flood from starving voice.
    for _ in 0..64 {
        match poll() {
            Some(Some(input)) => inputs.push(input),
            Some(None) => (),
            None => break,
        }
    }
    // Keep the controller adapter nonblocking: gilrs's filtered blocking wait can reset
    // its timeout indefinitely under stick jitter, starving this channel and Stop.
    std::thread::sleep(Duration::from_millis(10));
    inputs
}

#[derive(Debug, PartialEq, Eq)]
pub enum NativeOutcome {
    Cleared(Result<(),String>),
    Inserted(Pane,Result<(),String>),
    Submitted(Pane,Result<(),crate::herdr::NativeSubmitError>),
}

pub fn finish_native(core: &mut Core, outcome: NativeOutcome) -> Vec<Action> {
    match outcome {
        NativeOutcome::Cleared(result)=>vec![match result {
            Ok(())=>toast("Input line cleared","",Sound::None),
            Err(error)=>toast("Clear unconfirmed",&error,Sound::Request),
        }],
        NativeOutcome::Inserted(target,result)=>core.insertion_result(target,result),
        NativeOutcome::Submitted(target,result)=>match result {
            Err(crate::herdr::NativeSubmitError::Refused(error))=>core.submission_result(target,Some(error)),
            Err(crate::herdr::NativeSubmitError::Unconfirmed(error))=>{
                core.submission_result(target,None);
                vec![toast("Submission unconfirmed", &error, Sound::Request)]
            },
            Ok(())=>core.submission_result(target,None),
        },
    }
}

/// Apply one input's actions, delegating hardware recording to the daemon.
pub fn apply_actions(herdr: &Herdr<impl Runner>, actions: Vec<Action>, feedback: &mut Feedback, mut record: impl FnMut(Action)) -> Vec<NativeOutcome> {
    let mut insertions=Vec::new();
    for action in actions {
        match action {
            Action::StartRecording | Action::StopRecording | Action::CancelRecording => record(action),
            Action::Toast {..} => feedback.emit(herdr,&action),
            Action::InsertText { ref target, .. } => {
                let result=herdr.apply(&action);
                // Return delivery status without logging private transcript contents.
                insertions.push(NativeOutcome::Inserted(target.clone(),result));
            },
            Action::ClearInput {..} => insertions.push(NativeOutcome::Cleared(herdr.apply(&action))),
            Action::SubmitText {ref target} => {
                insertions.push(NativeOutcome::Submitted(target.clone(),herdr.submit_native(target)));
            },
            other => if let Err(e) = herdr.apply(&other) {
                crate::logging::event(format_args!("herd-gamepad: {other:?}: {e}"));
                feedback.emit(herdr,&toast("Herdr refused", &e, Sound::Request));
                // The remaining transcript action announces successful delivery.
                if matches!(other, Action::Prompt { .. }) { break; }
            },
        }
    }
    insertions
}

/// Whether the core needs to see Herdr for `input`. Starting a recording, releasing any other button, the
/// controller coming or going and a failed transcription route nothing; releasing the voice button does need
/// one, to capture where the user spoke.
pub fn needs_snapshot(input: &Input, voice_button: Btn) -> bool {
    match input {
        Input::Press(b) => *b != voice_button,
        Input::Release(b) => *b == voice_button,
        Input::ScrollAxis(_) | Input::Connected | Input::Disconnected | Input::TranscriptFailed(_) => false,
        Input::Transcript(_) => true,
    }
}

/// What the core gets when the snapshot for `input` failed with `err`: None skips the core. The inputs that
/// clear state go through with an empty snapshot (the recording must still stop, and then its transcript is
/// dropped as "nothing was focused"); a transcript that cannot be routed fails instead, so the next recording
/// can start.
pub fn on_snapshot_failure(core: &mut Core, input: Input, err: &str) -> Option<Input> {
    core.snapshot_failed();
    match input {
        Input::Transcript(_) if core.preview_in_progress() => Some(input),
        Input::Transcript(_) => Some(Input::TranscriptFailed(format!("Herdr unavailable: {err}"))),
        Input::Release(_) | Input::Disconnected | Input::TranscriptFailed(_) => Some(input),
        Input::ScrollAxis(_) | Input::Press(_) | Input::Connected => None,
    }
}

pub(crate) fn describe_for_log(input: &Input, core: &Core) -> String {
    if core.preview_in_progress() && matches!(input, Input::Transcript(_)) {
        "Transcript ready for preview".into()
    } else { describe(input) }
}

/// An input for the log: a transcript as its first 60 characters, everything else as is.
fn describe(input: &Input) -> String {
    match input {
        Input::Transcript(t) => format!("Transcript({:?})", preview(t)),
        Input::Press(b) => format!("Press({})", b.label()),
        Input::Release(b) => format!("Release({})", b.label()),
        other => format!("{other:?}"),
    }
}

struct Transcriber { cancellation: Cancellation, thread: Option<std::thread::JoinHandle<()>> }
impl Drop for Transcriber {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(thread) = self.thread.take() { let _ = thread.join(); }
    }
}
impl Transcriber {
fn spawn(&mut self, samples: Vec<f32>, cfg: &Config, state: &Path, tx: mpsc::Sender<Input>) {
    if let Some(thread) = self.thread.take() { let _ = thread.join(); }
    if !voice::long_enough(samples.len()) {
        let _ = tx.send(Input::Transcript(String::new()));
        return;
    }
    let (model, lang, wav): (PathBuf, String, PathBuf) = (cfg.model.clone(), cfg.language.clone(), state.join("voice.wav"));
    let cancellation = self.cancellation.clone();
    self.thread = Some(std::thread::spawn(move || {
        // A panic here would leave the core "transcribing" for ever; it is reported as a failure instead.
        let text = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            voice::write_wav_cancellable(&wav, &samples, || cancellation.cancelled())
                .and_then(|_| voice::transcribe(&CancellableCli { timeout: voice::WHISPER_TIMEOUT, cancellation: cancellation.clone() }, &model, &wav, &lang))
        }))
        .unwrap_or_else(|_| Err("transcription thread panicked".to_string()));
        if cancellation.cancelled() { return; }
        match text {
            Ok(t) => { let _ = tx.send(Input::Transcript(t)); }
            Err(e) => {
                crate::logging::event(format_args!("herd-gamepad: transcribe: {e}"));
                let _ = tx.send(Input::TranscriptFailed(e));
            }
        }
    }));
}
}

/// Compatibility command: request the managed Start action.
pub fn ensure() -> Result<(), String> {
    Cli::default().run(&["herdr", "plugin", "action", "invoke", "start", "--plugin", crate::config::PLUGIN_ID]).map(|_| ())
}
pub fn stop() -> Result<(), String> {
    let (state, _) = crate::config::dirs();
    let stopped = crate::lifecycle::stop(&state, Duration::from_secs(5))?;
    println!("herd-gamepad {}", if stopped { "stopped" } else { "already stopped" });
    Ok(())
}
pub fn logs() -> Result<(), String> {
    let (state, _) = crate::config::dirs();
    println!("herd-gamepad — recent logs (snapshot)\n{}", crate::logging::tail(&state)?);
    println!("Reopen to refresh. Press Enter to close.");
    let _ = std::io::stdin().read_line(&mut String::new());
    Ok(())
}

const MODEL_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin";

/// `setup`: check whisper-cli is installed and download the model the config points at when missing.
pub fn setup() -> Result<(), String> {
    let (cfg, _) = crate::config::load()?;
    Cli::default().run(&["whisper-cli", "--help"]).map_err(|_| "whisper-cli not found; run: brew install whisper-cpp".to_string())?;
    if cfg.model.exists() { println!("model present: {}", cfg.model.display()); return Ok(()); }
    let dir = cfg.model.parent().ok_or("model path has no parent")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = cfg.model.with_extension("part");
    println!("downloading {MODEL_URL} (148 MB)…");
    // A 148 MB download cannot finish under the 10 s default.
    Cli { timeout: Duration::from_secs(600) }.run(&["curl", "-fsSL", "-o", &tmp.to_string_lossy(), MODEL_URL])?;
    std::fs::rename(&tmp, &cfg.model).map_err(|e| e.to_string())?;
    println!("model saved: {}", cfg.model.display());
    Ok(())
}

pub fn mapping() -> Result<(), String> {
    let (cfg, _) = crate::config::load()?;
    print!("{MAPPING}\nHold {} to talk; release to transcribe.\n", cfg.voice_button.label());
    println!("Idle/done/working: submitted automatically. Blocked/unknown/no agent: dropped.");
    println!("\nPress Enter to close.");
    let _ = std::io::stdin().read_line(&mut String::new());
    Ok(())
}

pub const MAPPING: &str = "\
herd-gamepad — controller mapping

Physical control       Xbox          PlayStation
Bottom face button     A             Cross (X)
Right face button      B             Circle (O)
Left face button       X             Square
Top face button        Y             Triangle
Left / right shoulder  LB / RB       L1 / R1
Left / right trigger   LT / RT       L2 / R2
Back / Select          View          Create (PS5) / Share (PS4)
Menu                   Menu          Options
Left / right stick     Stick clicks  L3 / R3
Right stick up/down    Scroll selected pane (patched Herdr required)

NAVIGATE
D-pad                  Focus a neighbouring pane
Left / right shoulder  Previous / next space
Left / right trigger   Previous / next tab
Triangle (Xbox: Y)     Next agent needing attention
Square (Xbox: X)       Toggle pane zoom
Cross (Xbox: A)        Enter Respond / Interact

RESPOND
D-pad                  Move through the agent's options
Cross (Xbox: A)        Enter / confirm
Square (Xbox: X)       Clear input line (Codex / Claude)
Circle (Xbox: B)       Return to Navigate
Create / Share (View)  Escape / back in the app
Shoulders, triggers,
  Triangle             Navigate as above and leave Respond

INTERACT (NON-AGENT PANES)
Cross enters this mode on the focused pane without sending a key.
D-pad                  Send arrow keys to the pane
Cross (Xbox: A)        Enter / open
Square (Xbox: X)       Space / select
Circle (Xbox: B)       Return to Navigate
Create / Share (View)  Escape / back in the app
Shoulders, triggers,
  Triangle             Navigate as above and leave Interact
Focus changes or an agent appearing leave Interact without sending a key.
PR Tracker: left/right choose categories; Square opens details; up/down move.

SCROLLING (ALL MODES)
Tilt right stick up/down to scroll the selected or expanded pane.
Stronger tilt scrolls faster. Return to center after buttons or focus changes.
Paused during dictation and actions menus. Requires Herdr pane.scroll_wheel.

ACTIONS MENU
Menu / Options opens actions; if voice uses Menu, use left stick click.
D-pad                 Select a row; Right opens submenus; Left goes back
Cross (Xbox: A)        Activate; close actions require a separate confirmation
Square (Xbox: X)       Expand details; delete in text entry
Triangle (Xbox: Y)     Change text-entry character page
Circle (Xbox: B)       Back; View or the menu button closes
New space, worktrees, pane actions, Close…, Settings, Logs and Controls.
Closing stops running processes in the pane or space. Focus changes cancel.
Finish dictation before opening actions.
Native preview: Cross sends; Square clears the dictated input line.
Clear uses standard Codex/Claude editor shortcuts; multiline drafts clear only the current line.

D-pad always moves pane focus in Navigate; Cross enters Respond on an agent.
One Cross (Xbox: A) press answers a waiting agent, including from Navigate.
Routine status updates are in Gamepad logs. Errors are shown at most once per 5 seconds.
Actions use a live popup; wait for the selected target to appear before confirming.
Button positions define the actions, even when your controller uses other labels.
";

#[cfg(test)]
mod shutdown_tests {
    use super::*;
    #[test]
    fn worker_shutdown_cancels_and_joins_before_releasing_daemon_lock() {
        let state = std::env::temp_dir().join(format!("gamepad-worker-{}", std::process::id()));
        let daemon = Daemon::acquire(&state).unwrap().unwrap();
        let cancellation = daemon.cancellation();
        let (ready_tx, ready_rx) = mpsc::channel();
        let worker_cancel = cancellation.clone();
        let finished = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread_finished = finished.clone();
        let worker = Transcriber { cancellation, thread: Some(std::thread::spawn(move || {
            ready_tx.send(()).unwrap();
            let runner = CancellableCli { timeout: Duration::from_secs(10), cancellation: worker_cancel };
            let result = runner.run(&["/bin/sh", "-c", "sleep 5"]);
            assert_eq!(result.unwrap_err(), "command cancelled");
            std::thread::sleep(Duration::from_millis(50));
            thread_finished.store(true, std::sync::atomic::Ordering::SeqCst);
        })) };
        ready_rx.recv().unwrap();
        let start = std::time::Instant::now();
        drop(worker);
        assert!(start.elapsed() < Duration::from_secs(1));
        assert!(finished.load(std::sync::atomic::Ordering::SeqCst), "shutdown must join the worker");
        assert!(daemon.cancellation().cancelled());
        assert!(Daemon::acquire(&state).unwrap().is_none());
        drop(daemon);
        assert!(Daemon::acquire(&state).unwrap().is_some());
        std::fs::remove_dir_all(state).unwrap();
    }
}

/// Process scrolling ownership before menu/core button handling.
pub(crate) fn observe_scrolling(stick: &mut crate::scrolling::Stick, input: &Input, blocked: bool) -> bool {
    stick.observe(input);
    if blocked { stick.suppress(); }
    matches!(input, Input::ScrollAxis(_))
}
fn publish_scrolling(stick: &mut crate::scrolling::Stick, worker: &crate::scrolling::Worker, blocked: bool) {
    if blocked { stick.suppress(); }
    if !worker.update(stick.intent(std::time::Instant::now())) {
        stick.suppress();
        worker.update(None);
    }
}
