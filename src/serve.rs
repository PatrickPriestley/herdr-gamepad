//! The daemon: controller events in, herdr commands out, with push-to-talk transcribed off-thread.

use crate::config::Config;
use crate::engine::*;
use crate::herdr::{Cli, Herdr, Runner};
use crate::pad::{self, Pad};
use crate::voice::{self, Recorder};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

pub fn serve() -> Result<(), String> {
    let (cfg, state) = crate::config::load()?;
    if !cfg.model.exists() {
        return Err(format!("whisper model missing at {}; run: herd-gamepad setup", cfg.model.display()));
    }
    let herdr = Herdr { runner: Cli::default() };
    let mut gilrs = gilrs::Gilrs::new().map_err(|e| format!("gilrs: {e}"))?;
    let mut pad = Pad::new();
    let mut core = Core::new(cfg.voice_button);
    let mut recorder: Option<Recorder> = None;
    let (tx, rx) = mpsc::channel::<Input>();
    std::fs::write(state.join("daemon.pid"), std::process::id().to_string()).map_err(|e| e.to_string())?;
    eprintln!("herd-gamepad: listening; {} controller(s) connected; hold {:?} to talk", gilrs.gamepads().count(), cfg.voice_button);
    loop {
        let mut inputs = vec![];
        if let Some(ev) = gilrs.next_event_blocking(Some(Duration::from_millis(100))) {
            if let Some(i) = pad::raw(&ev.event).and_then(|r| pad.input(r)) { inputs.push(i); }
        }
        inputs.extend(rx.try_iter());
        for input in inputs {
            // A release only matters to the voice button; it needs no snapshot.
            let snap = if matches!(input, Input::Release(_)) { Snapshot::default() } else {
                herdr.snapshot().unwrap_or_else(|e| { eprintln!("herd-gamepad: snapshot: {e}"); Snapshot::default() })
            };
            eprintln!("herd-gamepad: {input:?} in {:?}", core.mode());
            for action in core.handle(input, &snap) {
                match action {
                    Action::StartRecording => match Recorder::start(cfg.max_seconds) {
                        Ok(r) => recorder = Some(r),
                        Err(e) => eprintln!("herd-gamepad: record: {e}"),
                    },
                    Action::StopRecording => if let Some(r) = recorder.take() {
                        spawn_transcribe(r.stop(), &cfg, &state, tx.clone());
                    },
                    other => if let Err(e) = herdr.apply(&other) {
                        eprintln!("herd-gamepad: {other:?}: {e}");
                        let _ = herdr.apply(&toast("Herdr refused", &e, Sound::Request));
                    },
                }
            }
        }
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
            Err(e) => eprintln!("herd-gamepad: transcribe: {e}"),
        }
    });
}

/// `ensure`: open the daemon's tab unless a daemon is already running.
pub fn ensure() -> Result<(), String> {
    let (state, _) = crate::config::dirs();
    if let Ok(pid) = std::fs::read_to_string(state.join("daemon.pid")) {
        if let Ok(pid) = pid.trim().parse::<i32>() {
            if unsafe { libc::kill(pid, 0) } == 0 { println!("herd-gamepad already running (pid {pid})"); return Ok(()); }
        }
    }
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
