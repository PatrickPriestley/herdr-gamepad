//! The daemon: controller events in, herdr commands out, with push-to-talk transcribed off-thread.

use crate::config::Config;
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
    let (cfg, state) = crate::config::load()?;
    if !cfg.model.exists() {
        return Err(format!("whisper model missing at {}; run: herd-gamepad setup", cfg.model.display()));
    }
    let cancellation = daemon.cancellation();
    let herdr = Herdr { runner: CancellableCli { timeout: Duration::from_secs(10), cancellation: cancellation.clone() } };
    let mut worker = Transcriber { cancellation: cancellation.clone(), thread: None };
    let mut gilrs = gilrs::Gilrs::new().map_err(|e| format!("gilrs: {e}"))?;
    let mut pad = Pad::new();
    let mut core = Core::new(cfg.voice_button);
    let mut recorder: Option<Recorder> = None;
    let (tx, rx) = mpsc::channel::<Input>();
    crate::logging::event(format_args!("herd-gamepad: listening; {} controller(s) connected; hold {} to talk", gilrs.gamepads().count(), cfg.voice_button.label()));
    while !cancellation.cancelled() {
        let inputs = next_inputs(|| {
            gilrs.next_event()
                .map(|ev| pad::raw(&ev.event).and_then(|r| pad.input(r)))
        }, &rx);
        for mut input in inputs {
            if cancellation.cancelled() { break; }
            let snap = match needs_snapshot(&input, cfg.voice_button).then(|| herdr.snapshot()) {
                None => Snapshot::default(),
                Some(Ok(s)) => s,
                Some(Err(e)) => {
                    crate::logging::event(format_args!("herd-gamepad: snapshot: {e}"));
                    let _ = herdr.apply(&toast("Herdr unavailable", &e, Sound::Request));
                    match on_snapshot_failure(&mut core, input, &e) {
                        Some(i) => input = i,
                        None => continue,
                    }
                    Snapshot::default()
                }
            };
            if cancellation.cancelled() { break; }
            crate::logging::event(format_args!("herd-gamepad: {} in {:?}", describe(&input), core.mode()));
            apply_actions(&herdr, core.handle(input, &snap), |action| {
                match action {
                    Action::StartRecording => match Recorder::start(cfg.max_seconds) {
                        Ok(r) => recorder = Some(r),
                        Err(e) => {
                            crate::logging::event(format_args!("herd-gamepad: record: {e}"));
                            let _ = herdr.apply(&toast("Recording failed", &e, Sound::Request));
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
        }
    }
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

/// Apply one input's actions, delegating hardware recording to the daemon.
pub fn apply_actions(herdr: &Herdr<impl Runner>, actions: Vec<Action>, mut record: impl FnMut(Action)) {
    for action in actions {
        match action {
            Action::StartRecording | Action::StopRecording | Action::CancelRecording => record(action),
            other => if let Err(e) = herdr.apply(&other) {
                crate::logging::event(format_args!("herd-gamepad: {other:?}: {e}"));
                let _ = herdr.apply(&toast("Herdr refused", &e, Sound::Request));
                // The remaining transcript action announces successful delivery.
                if matches!(other, Action::Prompt { .. }) { break; }
            },
        }
    }
}

/// Whether the core needs to see Herdr for `input`. Starting a recording, releasing any other button, the
/// controller coming or going and a failed transcription route nothing; releasing the voice button does need
/// one, to capture where the user spoke.
pub fn needs_snapshot(input: &Input, voice_button: Btn) -> bool {
    match input {
        Input::Press(b) => *b != voice_button,
        Input::Release(b) => *b == voice_button,
        Input::Connected | Input::Disconnected | Input::TranscriptFailed(_) => false,
        Input::Transcript(_) => true,
    }
}

/// What the core gets when the snapshot for `input` failed with `err`: None skips the core. The inputs that
/// clear state go through with an empty snapshot (the recording must still stop, and then its transcript is
/// dropped as "nothing was focused"); a transcript that cannot be routed fails instead, so the next recording
/// can start.
pub fn on_snapshot_failure(core: &mut Core, input: Input, err: &str) -> Option<Input> {
    core.cancel_close();
    match input {
        Input::Transcript(_) => Some(Input::TranscriptFailed(format!("Herdr unavailable: {err}"))),
        Input::Release(_) | Input::Disconnected | Input::TranscriptFailed(_) => Some(input),
        Input::Press(_) | Input::Connected => None,
    }
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

NAVIGATE
D-pad                  Focus a neighbouring pane
Left / right shoulder  Previous / next space
Left / right trigger   Previous / next tab
Top face button        Next agent needing attention
Left face button       Toggle pane zoom
Bottom face button     Enter Respond mode

RESPOND
D-pad                  Move through the agent's options
Bottom face button     Enter / confirm
Right face button      Escape
Back / Select          Return to Navigate
Shoulders, triggers,
  top or left face     Navigate as above and leave Respond

CLOSE ACTIONS
Menu / Options opens close actions; if voice uses Menu, use left stick click.
Left face button       Select close pane
Top face button        Select close space
Bottom face button     Confirm selected close
Right face / Back      Cancel; pressing the menu button again also cancels
Closing stops running processes in the pane or space. Focus changes cancel.
Finish dictation before opening close actions.

A D-pad press on a blocked agent auto-switches to Respond.
Each confirmation needs acknowledgement first; follow the on-screen prompt.
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
