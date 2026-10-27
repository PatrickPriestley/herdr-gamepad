//! The plugin's settings file and its Herdr-assigned directories.

use crate::engine::Btn;
use std::path::{Path, PathBuf};

pub const PLUGIN_ID: &str = "herd.gamepad";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config { pub model: PathBuf, pub language: String, pub voice_button: Btn, pub max_seconds: u64 }

/// (state dir, config dir), as Herdr sets them for a plugin process, with the documented fallbacks.
pub fn dirs() -> (PathBuf, PathBuf) {
    let (state, config) = dirs_from(
        std::env::var_os("HERDR_PLUGIN_STATE_DIR").map(PathBuf::from),
        std::env::var_os("HERDR_PLUGIN_CONFIG_DIR").map(PathBuf::from),
        &std::env::var("HOME").unwrap_or_default(),
    );
    let _ = std::fs::create_dir_all(&state);
    (state, config)
}

pub fn dirs_from(state: Option<PathBuf>, config: Option<PathBuf>, home: &str) -> (PathBuf, PathBuf) {
    (
        state.unwrap_or_else(|| Path::new(home).join(".local/state/herdr/plugins").join(PLUGIN_ID)),
        config.unwrap_or_else(|| Path::new(home).join(".config/herdr/plugins/config").join(PLUGIN_ID)),
    )
}

pub fn load() -> Result<(Config, PathBuf), String> {
    let (state, config) = dirs();
    let text = std::fs::read_to_string(config.join("config.toml")).unwrap_or_default();
    Ok((parse(&text, &state)?, state))
}

pub fn parse(text: &str, state: &Path) -> Result<Config, String> {
    let t: toml::Table = text.parse().map_err(|e| format!("config.toml: {e}"))?;
    let get = |a: &str, b: &str| t.get(a).and_then(|v| v.get(b));
    let model = match get("whisper", "model").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
        Some(s) => expand(s),
        None => state.join("models/ggml-base.en.bin"),
    };
    let language = get("whisper", "language").and_then(|v| v.as_str()).unwrap_or("en").to_string();
    let voice_button = match get("voice", "button").and_then(|v| v.as_str()).unwrap_or("rstick") {
        "rstick" => Btn::RStick,
        "lstick" => Btn::LStick,
        "menu" => Btn::Menu,
        other => return Err(format!("config.toml: voice.button {other:?} is not one of rstick, lstick, menu")),
    };
    let max_seconds = get("voice", "max_seconds").and_then(|v| v.as_integer()).unwrap_or(60).max(1) as u64;
    Ok(Config { model, language, voice_button, max_seconds })
}

fn expand(s: &str) -> PathBuf {
    match s.strip_prefix("~/") {
        Some(rest) => Path::new(&std::env::var("HOME").unwrap_or_default()).join(rest),
        None => PathBuf::from(s),
    }
}
