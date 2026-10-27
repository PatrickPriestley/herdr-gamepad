//! The daemon: controller events in, herdr commands out, with push-to-talk transcribed off-thread.

use crate::config::Config;
use crate::engine::*;
use crate::herdr::{Cli, Herdr, Runner};
use crate::pad::{self, Pad};
use crate::voice::{self, Recorder};
use std::fs::File;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

pub fn serve() -> Result<(), String> {
    let (cfg, state) = crate::config::load()?;
    if !cfg.model.exists() {
        return Err(format!("whisper model missing at {}; run: herd-gamepad setup", cfg.model.display()));
    }
    // The lock lives as long as the loop: a second daemon would send every key twice.
    let mut _lock = try_lock(&state)?.ok_or("another herd-gamepad is already running")?;
    let _ = _lock.set_len(0).and_then(|_| write!(_lock, "{}", std::process::id()));
    let herdr = Herdr { runner: Cli::default() };
    let mut gilrs = gilrs::Gilrs::new().map_err(|e| format!("gilrs: {e}"))?;
    let mut pad = Pad::new();
    let mut core = Core::new(cfg.voice_button);
    let mut recorder: Option<Recorder> = None;
    let (tx, rx) = mpsc::channel::<Input>();
    eprintln!("herd-gamepad: listening; {} controller(s) connected; hold {} to talk", gilrs.gamepads().count(), cfg.voice_button.label());
    loop {
        let mut inputs = vec![];
        if let Some(ev) = gilrs.next_event_blocking(Some(Duration::from_millis(100))) {
            if let Some(i) = pad::raw(&ev.event).and_then(|r| pad.input(r)) { inputs.push(i); }
        }
        inputs.extend(rx.try_iter());
        for mut input in inputs {
            let snap = match needs_snapshot(&input, cfg.voice_button).then(|| herdr.snapshot()) {
                None => Snapshot::default(),
                Some(Ok(s)) => s,
                Some(Err(e)) => {
                    eprintln!("herd-gamepad: snapshot: {e}");
                    let _ = herdr.apply(&toast("Herdr unavailable", &e, Sound::Request));
                    match on_snapshot_failure(input, &e) {
                        Some(i) => input = i,
                        None => continue,
                    }
                    Snapshot::default()
                }
            };
            eprintln!("herd-gamepad: {} in {:?}", describe(&input), core.mode());
            apply_actions(&herdr, core.handle(input, &snap), |action| {
                match action {
                    Action::StartRecording => match Recorder::start(cfg.max_seconds) {
                        Ok(r) => recorder = Some(r),
                        Err(e) => {
                            eprintln!("herd-gamepad: record: {e}");
                            let _ = herdr.apply(&toast("Recording failed", &e, Sound::Request));
                        }
                    },
                    Action::StopRecording => match recorder.take() {
                        Some(r) => spawn_transcribe(r.stop(), &cfg, &state, tx.clone()),
                        None => { let _ = tx.send(Input::TranscriptFailed("recording did not start".into())); }
                    },
                    Action::CancelRecording => drop(recorder.take()),
                    _ => unreachable!("only recording actions are delegated"),
                }
            });
        }
    }
}

/// Apply one input's actions, delegating hardware recording to the daemon.
pub fn apply_actions(herdr: &Herdr<impl Runner>, actions: Vec<Action>, mut record: impl FnMut(Action)) {
    for action in actions {
        match action {
            Action::StartRecording | Action::StopRecording | Action::CancelRecording => record(action),
            other => if let Err(e) = herdr.apply(&other) {
                eprintln!("herd-gamepad: {other:?}: {e}");
                let _ = herdr.apply(&toast("Herdr refused", &e, Sound::Request));
                // The remaining transcript action announces successful delivery.
                if matches!(other, Action::Prompt { .. } | Action::SendText { .. }) { break; }
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
pub fn on_snapshot_failure(input: Input, err: &str) -> Option<Input> {
    match input {
        Input::Transcript(_) => Some(Input::TranscriptFailed(format!("Herdr unavailable: {err}"))),
        Input::Release(_) | Input::Disconnected | Input::TranscriptFailed(_) => Some(input),
        Input::Press(_) | Input::Connected => None,
    }
}

/// The daemon's lock file, `state/daemon.pid`, locked: Ok(None) when another process holds the lock.
/// Dropping the file releases the lock.
fn try_lock(state: &Path) -> Result<Option<File>, String> {
    let path = state.join("daemon.pid");
    let f = File::options().read(true).write(true).create(true).truncate(false).open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 { return Ok(Some(f)); }
    let e = std::io::Error::last_os_error();
    match e.raw_os_error() {
        // EAGAIN on macOS: the only errno that means "someone else holds it".
        Some(libc::EWOULDBLOCK) => Ok(None),
        _ => Err(format!("daemon.pid lock: {e}")),
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

fn spawn_transcribe(samples: Vec<f32>, cfg: &Config, state: &Path, tx: mpsc::Sender<Input>) {
    if !voice::long_enough(samples.len()) {
        let _ = tx.send(Input::Transcript(String::new()));
        return;
    }
    let (model, lang, wav): (PathBuf, String, PathBuf) = (cfg.model.clone(), cfg.language.clone(), state.join("voice.wav"));
    std::thread::spawn(move || {
        // A panic here would leave the core "transcribing" for ever; it is reported as a failure instead.
        let text = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            voice::write_wav(&wav, &samples)
                .and_then(|_| voice::transcribe(&Cli { timeout: voice::WHISPER_TIMEOUT }, &model, &wav, &lang))
        }))
        .unwrap_or_else(|_| Err("transcription thread panicked".to_string()));
        match text {
            Ok(t) => { let _ = tx.send(Input::Transcript(t)); }
            Err(e) => {
                eprintln!("herd-gamepad: transcribe: {e}");
                let _ = tx.send(Input::TranscriptFailed(e));
            }
        }
    });
}

/// `ensure`: open the daemon's tab unless a daemon is already running.
pub fn ensure() -> Result<(), String> {
    let (state, _) = crate::config::dirs();
    // The lock is released at the end of this statement, before the daemon starts.
    let running = try_lock(&state)?.is_none();
    if running { println!("herd-gamepad already running"); return Ok(()); }
    Cli::default().run(&["herdr", "plugin", "pane", "open", "--plugin", crate::config::PLUGIN_ID, "--entrypoint", "daemon", "--no-focus"]).map(|_| ())
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
    println!("Idle/done: sent. Working: typed without Enter. Blocked/unknown/no agent: dropped.");
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

A D-pad press on a blocked agent auto-switches to Respond.
Each confirmation needs acknowledgement first; follow the on-screen prompt.
Button positions define the actions, even when your controller uses other labels.
";
