//! Optimistic, format-preserving configuration persistence.
use crate::{
    config::{self, Config},
    engine::Btn,
    menu::{LabelStyle, SettingsSnapshot},
};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::{fs::OpenOptionsExt, io::AsRawFd},
    path::Path,
};

#[derive(Debug)]
pub enum SaveError {
    Failed(String),
    Conflict(SettingsSnapshot),
}
impl From<String> for SaveError {
    fn from(e: String) -> Self {
        Self::Failed(e)
    }
}
fn source(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(s) => Ok(Some(s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("config.toml: {e}")),
    }
}
// Atomic replacement would replace a symlink itself rather than its target.
// Reject it explicitly, including dangling links, instead of changing dotfile ownership.
fn ensure_writable_destination(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(
            "Cannot save settings through a config.toml symlink; edit its target directly".into(),
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("config.toml destination: {error}")),
    }
}

// Store the exact content, including absence, rather than a collision-prone hash.
fn version(text: &Option<String>) -> String {
    serde_json::to_string(text).unwrap()
}
pub fn load(path: &Path, state: &Path) -> Result<(SettingsSnapshot, Config), String> {
    let text = source(path)?;
    snapshot(path, state, &text)
}
fn snapshot(
    path: &Path,
    state: &Path,
    text: &Option<String>,
) -> Result<(SettingsSnapshot, Config), String> {
    let cfg = config::parse(text.as_deref().unwrap_or(""), state)?;
    Ok((
        SettingsSnapshot {
            labels: cfg.labels,
            voice_button: cfg.voice_button,
            max_seconds: cfg.max_seconds,
            model: cfg.model.to_string_lossy().into(),
            language: cfg.language.clone(),
            config_path: path.to_string_lossy().into(),
            version: version(text),
        },
        cfg,
    ))
}
pub fn save(
    path: &Path,
    state: &Path,
    original: &SettingsSnapshot,
    updated: &SettingsSnapshot,
) -> Result<(SettingsSnapshot, Config), SaveError> {
    save_before_commit(path, state, original, updated, || {})
}
fn save_before_commit(
    path: &Path,
    state: &Path,
    original: &SettingsSnapshot,
    updated: &SettingsSnapshot,
    before_commit: impl FnOnce(),
) -> Result<(SettingsSnapshot, Config), SaveError> {
    ensure_writable_destination(path)?;
    let dir = path
        .parent()
        .ok_or("Configuration has no parent".to_string())?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(dir.join("config.lock"))
        .map_err(|e| e.to_string())?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Err("Another settings writer is active".to_string().into());
    }
    let old = source(path)?;
    if version(&old) != original.version {
        return Err(SaveError::Conflict(snapshot(path, state, &old)?.0));
    }
    let rendered = edit(old.as_deref().unwrap_or(""), original, updated)?;
    let committed = snapshot(path, state, &Some(rendered.clone()))?;
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let temporary = dir.join(format!(
        "config.{}.{}.tmp",
        std::process::id(),
        SERIAL.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    let result = (|| {
        if let Ok(meta) = fs::metadata(path) {
            file.set_permissions(meta.permissions())
                .map_err(|e| e.to_string())?;
        }
        file.write_all(rendered.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        before_commit();
        ensure_writable_destination(path)?;
        let latest = source(path)?;
        if latest != old {
            return Err(SaveError::Conflict(snapshot(path, state, &latest)?.0));
        }
        fs::rename(&temporary, path).map_err(|e| e.to_string())?;
        Ok(committed)
    })();
    let _ = fs::remove_file(temporary);
    result
}
fn edit(
    text: &str,
    original: &SettingsSnapshot,
    updated: &SettingsSnapshot,
) -> Result<String, String> {
    let mut doc = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| e.to_string())?;
    let button = match updated.voice_button {
        Btn::RStick => "rstick",
        Btn::LStick => "lstick",
        Btn::Menu => "menu",
        _ => return Err("Unsupported voice button".into()),
    };
    if !(1..=3600).contains(&updated.max_seconds) {
        return Err("Recording duration must be 1–3600 seconds".into());
    }
    if original.labels != updated.labels {
        replace(
            &mut doc,
            "ui",
            "labels",
            toml_edit::Value::from(match updated.labels {
                LabelStyle::Both => "both",
                LabelStyle::PlayStation => "playstation",
                LabelStyle::Xbox => "xbox",
            }),
        )?;
    }
    if original.voice_button != updated.voice_button {
        replace(&mut doc, "voice", "button", toml_edit::Value::from(button))?;
    }
    if original.max_seconds != updated.max_seconds {
        replace(
            &mut doc,
            "voice",
            "max_seconds",
            toml_edit::Value::from(updated.max_seconds as i64),
        )?;
    }
    Ok(doc.to_string())
}
fn replace(
    doc: &mut toml_edit::DocumentMut,
    table: &str,
    key: &str,
    mut value: toml_edit::Value,
) -> Result<(), String> {
    if doc.get(table).is_none() {
        doc[table] = toml_edit::Item::Table(toml_edit::Table::new());
    }
    let t = doc[table]
        .as_table_like_mut()
        .ok_or_else(|| format!("{table} is not a settings table"))?;
    if let Some(old) = t.get(key).and_then(toml_edit::Item::as_value) {
        *value.decor_mut() = old.decor().clone();
    }
    t.insert(key, toml_edit::Item::Value(value));
    Ok(())
}
#[cfg(test)]
mod tests;
