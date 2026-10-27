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
    eprintln!("herd-gamepad: listening; {} controller(s) connected; hold {:?} to talk", gilrs.gamepads().count(), cfg.voice_button);
    loop {
        let mut inputs = vec![];
        if let Some(ev) = gilrs.next_event_blocking(Some(Duration::from_millis(100))) {
            if let Some(i) = pad::raw(&ev.event).and_then(|r| pad.input(r)) { inputs.push(i); }
        }
        inputs.extend(rx.try_iter());
        for mut input in inputs {
            // Pressing the voice button and releasing any other needs no snapshot; releasing the voice button
            // does, to capture where the user spoke.
            let needs_snapshot = match &input {
                Input::Press(b) => *b != cfg.voice_button,
                Input::Release(b) => *b == cfg.voice_button,
                _ => true,
            };
            let snap = match needs_snapshot.then(|| herdr.snapshot()) {
                None => Snapshot::default(),
                Some(Ok(s)) => s,
                Some(Err(e)) => {
                    eprintln!("herd-gamepad: snapshot: {e}");
                    let _ = herdr.apply(&toast("Herdr unavailable", &e, Sound::Request));
                    match input {
                        // The transcript cannot be routed; fail it so the next recording can start.
                        Input::Transcript(_) => input = Input::TranscriptFailed(format!("Herdr unavailable: {e}")),
                        // The recording must still stop; with no focused pane its transcript is dropped.
                        Input::Release(_) => {}
                        _ => continue,
                    }
                    Snapshot::default()
                }
            };
            eprintln!("herd-gamepad: {} in {:?}", describe(&input), core.mode());
            for action in core.handle(input, &snap) {
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
                    other => if let Err(e) = herdr.apply(&other) {
                        eprintln!("herd-gamepad: {other:?}: {e}");
                        let _ = herdr.apply(&toast("Herdr refused", &e, Sound::Request));
                    },
                }
            }
        }
    }
}

/// The daemon's lock file, `state/daemon.pid`, locked: Ok(None) when another process holds the lock.
/// Dropping the file releases the lock.
fn try_lock(state: &Path) -> Result<Option<File>, String> {
    let path = state.join("daemon.pid");
    let f = File::options().read(true).write(true).create(true).truncate(false).open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    match unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } {
        0 => Ok(Some(f)),
        _ => Ok(None),
    }
}

/// An input for the log: a transcript as its first 60 characters, everything else as is.
fn describe(input: &Input) -> String {
    match input {
        Input::Transcript(t) => format!("Transcript({:?})", preview(t)),
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
        let text = voice::write_wav(&wav, &samples)
            .and_then(|_| voice::transcribe(&Cli { timeout: voice::WHISPER_TIMEOUT }, &model, &wav, &lang));
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

pub const MAPPING: &str = "\
herd-gamepad                                   View = back to Navigate

Navigate (default)                 Respond (after A, or a D-pad press on a blocked agent)
  D-pad   focus pane                 D-pad   up / down / left / right
  LB RB   previous / next space      A       enter
  LT RT   previous / next tab        B       esc
  Y       next agent that needs you  LB RB LT RT Y X  as in Navigate, then back to Navigate
  X       zoom the pane
  A       Respond mode

Hold the right stick click (or voice.button in config.toml) to talk; release to send.
  idle or done agent: sent    working: typed, not sent    blocked or none: dropped

Press Enter to close.
";
