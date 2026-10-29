//! The plugin's settings file and its Herdr-assigned directories.

use crate::engine::Btn;
use std::path::{Path, PathBuf};

pub const PLUGIN_ID: &str = "herd.gamepad";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config { pub labels: crate::menu::LabelStyle, pub model: PathBuf, pub language: String, pub voice_button: Btn, pub max_seconds: u64, pub preview_before_send: bool }

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
    Ok((load_from(&config, &state)?, state))
}

/// The config in `config_dir`; a missing config.toml means the defaults, any other read error is reported.
pub fn load_from(config_dir: &Path, state: &Path) -> Result<Config, String> {
    let text = match std::fs::read_to_string(config_dir.join("config.toml")) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("config.toml: {e}")),
    };
    parse(&text, state)
}

pub fn parse(text: &str, state: &Path) -> Result<Config, String> {
    let t: toml::Table = text.parse().map_err(|e| format!("config.toml: {e}"))?;
    let model = match string(&t, "whisper", "model")?.filter(|s| !s.is_empty()) {
        Some(s) => expand(s),
        None => state.join("models/ggml-base.en.bin"),
    };
    let language = string(&t, "whisper", "language")?.unwrap_or("en").to_string();
    let voice_button = match string(&t, "voice", "button")?.unwrap_or("rstick") {
        "rstick" => Btn::RStick,
        "lstick" => Btn::LStick,
        "menu" => Btn::Menu,
        other => return Err(format!("config.toml: voice.button {other:?} is not one of rstick, lstick, menu")),
    };
    let max_seconds = integer(&t, "voice", "max_seconds")?.unwrap_or(60).clamp(1, 3600) as u64;
    let labels = match string(&t, "ui", "labels")?.unwrap_or("both") {
        "both" => crate::menu::LabelStyle::Both,
        "playstation" => crate::menu::LabelStyle::PlayStation,
        "xbox" => crate::menu::LabelStyle::Xbox,
        other => return Err(format!("config.toml: unknown ui.labels {other:?}")),
    };
    let preview_before_send = boolean(&t, "voice", "preview_before_send")?.unwrap_or(false);
    Ok(Config { labels, model, language, voice_button, max_seconds, preview_before_send })
}

/// `[a] b` as a string: None when absent, an error when present with another type.
fn string<'t>(t: &'t toml::Table, a: &str, b: &str) -> Result<Option<&'t str>, String> {
    match t.get(a).and_then(|v| v.get(b)) {
        None => Ok(None),
        Some(v) => v.as_str().map(Some).ok_or_else(|| format!("config.toml: {a}.{b} must be a string")),
    }
}

/// `[a] b` as an integer: None when absent, an error when present with another type.
fn integer(t: &toml::Table, a: &str, b: &str) -> Result<Option<i64>, String> {
    match t.get(a).and_then(|v| v.get(b)) {
        None => Ok(None),
        Some(v) => v.as_integer().map(Some).ok_or_else(|| format!("config.toml: {a}.{b} must be an integer")),
    }
}

fn boolean(t: &toml::Table, a: &str, b: &str) -> Result<Option<bool>, String> {
    match t.get(a).and_then(|v| v.get(b)) {
        None => Ok(None),
        Some(v) => v.as_bool().map(Some).ok_or_else(|| format!("config.toml: {a}.{b} must be a boolean")),
    }
}

fn expand(s: &str) -> PathBuf {
    match s.strip_prefix("~/") {
        Some(rest) => Path::new(&std::env::var("HOME").unwrap_or_default()).join(rest),
        None => PathBuf::from(s),
    }
}
