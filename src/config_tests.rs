use crate::config::*;
use crate::engine::Btn;
use std::path::{Path, PathBuf};

fn state() -> PathBuf { PathBuf::from("/tmp/state") }

#[test]
fn an_empty_config_gives_the_defaults() {
    let c = parse("", &state()).unwrap();
    assert_eq!(c, Config { model: Path::new("/tmp/state/models/ggml-base.en.bin").into(), language: "en".into(), voice_button: Btn::RStick, max_seconds: 60 });
}

#[test]
fn every_field_can_be_set() {
    let text = "[whisper]\nmodel = \"/m/small.bin\"\nlanguage = \"de\"\n[voice]\nbutton = \"menu\"\nmax_seconds = 20\n";
    let c = parse(text, &state()).unwrap();
    assert_eq!(c, Config { model: "/m/small.bin".into(), language: "de".into(), voice_button: Btn::Menu, max_seconds: 20 });
}

#[test]
fn an_empty_model_means_the_default_and_tilde_expands() {
    assert_eq!(parse("[whisper]\nmodel = \"\"\n", &state()).unwrap().model, Path::new("/tmp/state/models/ggml-base.en.bin"));
    let home = std::env::var("HOME").unwrap();
    assert_eq!(parse("[whisper]\nmodel = \"~/m.bin\"\n", &state()).unwrap().model, Path::new(&home).join("m.bin"));
}

#[test]
fn the_voice_button_must_be_one_that_can_be_held() {
    assert_eq!(parse("[voice]\nbutton = \"lstick\"\n", &state()).unwrap().voice_button, Btn::LStick);
    let err = parse("[voice]\nbutton = \"a\"\n", &state()).unwrap_err();
    assert!(err.contains("voice.button") && err.contains("rstick"), "{err}");
}

#[test]
fn max_seconds_is_at_least_one() {
    assert_eq!(parse("[voice]\nmax_seconds = 0\n", &state()).unwrap().max_seconds, 1);
    assert_eq!(parse("[voice]\nmax_seconds = -5\n", &state()).unwrap().max_seconds, 1);
}

#[test]
fn bad_toml_is_an_error_naming_the_file() {
    let err = parse("[whisper\n", &state()).unwrap_err();
    assert!(err.starts_with("config.toml:"), "{err}");
}

#[test]
fn dirs_follow_the_herdr_plugin_env_overrides() {
    let (state, config) = dirs_from(Some("/s".into()), Some("/c".into()), "/home/x");
    assert_eq!((state, config), (PathBuf::from("/s"), PathBuf::from("/c")));
    let (state, config) = dirs_from(None, None, "/home/x");
    assert_eq!(state, Path::new("/home/x/.local/state/herdr/plugins/herd.gamepad"));
    assert_eq!(config, Path::new("/home/x/.config/herdr/plugins/config/herd.gamepad"));
}

#[test]
fn max_seconds_is_at_most_an_hour() {
    assert_eq!(parse("[voice]\nmax_seconds = 10000\n", &state()).unwrap().max_seconds, 3600);
}

#[test]
fn a_wrongly_typed_value_is_an_error_not_a_default() {
    assert_eq!(parse("[voice]\nmax_seconds = 20.0\n", &state()).unwrap_err(), "config.toml: voice.max_seconds must be an integer");
    assert_eq!(parse("[voice]\nmax_seconds = \"20\"\n", &state()).unwrap_err(), "config.toml: voice.max_seconds must be an integer");
    assert_eq!(parse("[voice]\nbutton = 1\n", &state()).unwrap_err(), "config.toml: voice.button must be a string");
    assert_eq!(parse("[whisper]\nmodel = 5\n", &state()).unwrap_err(), "config.toml: whisper.model must be a string");
    assert_eq!(parse("[whisper]\nlanguage = 5\n", &state()).unwrap_err(), "config.toml: whisper.language must be a string");
}

#[test]
fn load_from_treats_a_missing_file_as_defaults_but_reports_other_read_errors() {
    let dir = std::env::temp_dir().join(format!("herd-gamepad-config-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(load_from(&dir, &state()), parse("", &state()), "no config.toml means the defaults");
    std::fs::create_dir_all(dir.join("config.toml")).unwrap();
    let err = load_from(&dir, &state()).unwrap_err();
    assert!(err.starts_with("config.toml:"), "{err}");
    std::fs::remove_dir_all(&dir).unwrap();
}
