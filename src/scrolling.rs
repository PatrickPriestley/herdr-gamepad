//! Right-stick gestures and bounded, asynchronous terminal scrollback updates.
use crate::engine::Input;
use std::time::{Duration, Instant};

pub struct Stick {
    axis: i8,
    armed: bool,
    active: bool,
    epoch: u64,
}
impl Default for Stick {
    fn default() -> Self {
        Self {
            axis: 0,
            armed: true,
            active: false,
            epoch: 0,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Intent {
    pub epoch: u64,
    pub steps: i8,
    pub expires: Instant,
}
impl Stick {
    pub fn observe(&mut self, input: &Input) {
        match input {
            Input::ScrollAxis(axis) => {
                self.axis = *axis;
                if axis.unsigned_abs() <= 20 {
                    self.active = false;
                    self.armed = true;
                } else if self.armed && !self.active && axis.unsigned_abs() >= 35 {
                    self.active = true;
                    self.epoch = self.epoch.wrapping_add(1);
                }
            }
            Input::Press(_) => self.suppress(),
            Input::Connected | Input::Disconnected => {
                self.active = false;
                self.armed = false;
            }
            _ => {}
        }
    }
    pub fn suppress(&mut self) {
        self.active = false;
        self.armed = self.axis.unsigned_abs() <= 20;
    }
    pub fn intent(&self, now: Instant) -> Option<Intent> {
        self.active.then(|| Intent {
            epoch: self.epoch,
            steps: self.axis.signum()
                * match self.axis.unsigned_abs() {
                    85.. => 3,
                    60.. => 2,
                    _ => 1,
                },
            expires: now + Duration::from_millis(100),
        })
    }
}

use serde_json::json;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

#[derive(Default)]
pub(crate) struct Shared {
    pub intent: Option<Intent>,
    pub failed: Option<u64>,
    pub last_epoch: Option<u64>,
    pub notice: Option<String>,
    pub stopped: bool,
}
impl Shared {
    pub fn publish(&mut self, intent: Option<Intent>) {
        self.last_epoch = intent.map(|intent| intent.epoch).or(self.last_epoch);
        self.intent = intent;
    }
}
#[derive(Default)]
pub(crate) struct Pump {
    epoch: Option<u64>,
    target: Option<String>,
    due: Option<Instant>,
}

pub(crate) trait Backend {
    fn focused(&mut self) -> Result<String, String>;
    fn scroll(
        &mut self,
        pane: &str,
        steps: i8,
        still_current: &dyn Fn() -> bool,
    ) -> Result<(), String>;
}
pub(crate) struct SocketBackend {
    pub path: Option<PathBuf>,
}
impl Backend for SocketBackend {
    fn focused(&mut self) -> Result<String, String> {
        let path = self.path.as_ref().ok_or("HERDR_SOCKET_PATH is missing")?;
        let result = crate::scroll_rpc::call(path, "pane.list", json!({}))?;
        if result.get("type").and_then(|v| v.as_str()) != Some("pane_list") {
            return Err("Unexpected pane list response".into());
        }
        result
            .get("panes")
            .and_then(|v| v.as_array())
            .and_then(|panes| {
                panes
                    .iter()
                    .find(|pane| pane.get("focused").and_then(|v| v.as_bool()) == Some(true))
            })
            .and_then(|pane| pane.get("pane_id"))
            .and_then(|v| v.as_str())
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .ok_or_else(|| "No pane is selected".into())
    }
    fn scroll(
        &mut self,
        pane: &str,
        steps: i8,
        still_current: &dyn Fn() -> bool,
    ) -> Result<(), String> {
        let path = self.path.as_ref().ok_or("HERDR_SOCKET_PATH is missing")?;
        let Some(result) = crate::scroll_rpc::call_if_current(
            path,
            "pane.get",
            json!({"pane_id":pane}),
            still_current,
        )?
        else {
            return Ok(());
        };
        let current = scroll_pane(&result, pane)?;
        if current.get("focused").and_then(|v| v.as_bool()) != Some(true) {
            return Err("stale_pane_target".into());
        }
        let (offset, max) = scroll_metrics(current)?;
        if max == 0 {
            return Err("No terminal scrollback is available in this pane".into());
        }
        let distance = u64::from(steps.unsigned_abs());
        let next = if steps > 0 {
            offset.saturating_add(distance).min(max)
        } else {
            offset.saturating_sub(distance)
        };
        if next == offset {
            return Ok(());
        }
        let Some(result) = crate::scroll_rpc::call_if_current(
            path,
            "pane.scroll",
            json!({"pane_id":pane,"offset_from_bottom":next}),
            still_current,
        )?
        else {
            return Ok(());
        };
        scroll_metrics(scroll_pane(&result, pane)?)?;
        Ok(())
    }
}

fn scroll_pane<'a>(
    result: &'a serde_json::Value,
    expected: &str,
) -> Result<&'a serde_json::Value, String> {
    if result.get("type").and_then(|v| v.as_str()) != Some("pane_info") {
        return Err("Unexpected pane info response".into());
    }
    result
        .get("pane")
        .filter(|pane| pane.get("pane_id").and_then(|v| v.as_str()) == Some(expected))
        .ok_or_else(|| "Unexpected scroll pane".into())
}

fn scroll_metrics(pane: &serde_json::Value) -> Result<(u64, u64), String> {
    let metrics = pane.get("scroll").ok_or("Invalid scroll metrics")?;
    let offset = metrics
        .get("offset_from_bottom")
        .and_then(|v| v.as_u64())
        .ok_or("Invalid scroll metrics")?;
    let max = metrics
        .get("max_offset_from_bottom")
        .and_then(|v| v.as_u64())
        .ok_or("Invalid scroll metrics")?;
    if offset > max {
        return Err("Invalid scroll metrics".into());
    }
    Ok((offset, max))
}

impl Pump {
    pub fn tick(&mut self, shared: &Mutex<Shared>, backend: &mut impl Backend, now: Instant) {
        let started = Instant::now();
        let Some(intent) = current(shared, now) else {
            return;
        };
        if self.epoch != Some(intent.epoch) {
            self.epoch = Some(intent.epoch);
            self.target = None;
            self.due = None;
        }
        if self.due.is_some_and(|due| now < due) {
            return;
        }
        self.due = Some(now + Duration::from_millis(100));
        let result = (|| {
            if self.target.is_none() {
                self.target = Some(backend.focused()?);
            }
            let Some(latest) = current(shared, now + started.elapsed())
                .filter(|latest| latest.epoch == intent.epoch)
            else {
                return Ok(());
            };
            backend.scroll(
                self.target.as_deref().expect("focus resolved"),
                latest.steps,
                &|| {
                    current(shared, now + started.elapsed())
                        .is_some_and(|latest| latest.epoch == intent.epoch)
                },
            )
        })();
        if let Err(error) = result {
            let mut state = shared.lock().expect("scroll state");
            if state.last_epoch == Some(intent.epoch) {
                state.failed = Some(intent.epoch);
                state.notice = Some(notice(&error));
            }
        }
    }
}

fn notice(error: &str) -> String {
    if error.contains("unknown_method")
        || error.contains("unknown variant")
        || error.contains("unknown method")
    {
        "This Herdr build does not expose terminal scrollback through pane.scroll.".into()
    } else if error.contains("stale_pane_target") {
        "Pane focus changed. Center the right stick before scrolling the selected pane.".into()
    } else {
        format!(
            "Joystick scrolling stopped: {}. Center the right stick to retry.",
            error.chars().take(160).collect::<String>()
        )
    }
}

fn current(shared: &Mutex<Shared>, now: Instant) -> Option<Intent> {
    let state = shared.lock().expect("scroll state");
    state.intent.filter(|intent| {
        !state.stopped && state.failed != Some(intent.epoch) && now < intent.expires
    })
}

/// A single worker consumes only the latest desired gesture; it never queues scroll updates.
pub struct Worker {
    shared: Arc<Mutex<Shared>>,
    thread: Option<JoinHandle<()>>,
}
impl Worker {
    pub fn new() -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let state = shared.clone();
        let path = std::env::var_os("HERDR_SOCKET_PATH").map(PathBuf::from);
        let thread = thread::spawn(move || {
            let mut pump = Pump::default();
            let mut backend = SocketBackend { path };
            loop {
                if state.lock().expect("scroll state").stopped {
                    break;
                }
                pump.tick(&state, &mut backend, Instant::now());
                thread::sleep(Duration::from_millis(10));
            }
        });
        Self {
            shared,
            thread: Some(thread),
        }
    }
    /// False means this gesture failed and must be released before retrying.
    pub fn update(&self, intent: Option<Intent>) -> bool {
        let mut state = self.shared.lock().expect("scroll state");
        if intent.is_some_and(|intent| state.failed == Some(intent.epoch)) {
            return false;
        }
        state.publish(intent);
        true
    }
    pub fn take_notice(&self) -> Option<String> {
        self.shared.lock().expect("scroll state").notice.take()
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.shared.lock().expect("scroll state").stopped = true;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod worker_tests {
    use super::*;
    #[test]
    fn idle_worker_acknowledges_stop_without_network_work() {
        let worker = Worker::new();
        worker.shared.lock().unwrap().stopped = true;
        let deadline = Instant::now() + Duration::from_secs(1);
        while !worker.thread.as_ref().unwrap().is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        let finished = worker.thread.as_ref().unwrap().is_finished();
        if !finished {
            std::mem::forget(worker);
        }
        assert!(finished, "worker ignored Stop");
    }
    #[test]
    fn latest_intent_replaces_old_work_and_failed_gesture_cannot_rearm() {
        let now = Instant::now();
        let worker = Worker {
            shared: Arc::new(Mutex::new(Shared::default())),
            thread: None,
        };
        let one = Intent {
            epoch: 1,
            steps: 1,
            expires: now,
        };
        let two = Intent {
            epoch: 2,
            steps: -3,
            expires: now,
        };
        assert!(worker.update(Some(one)));
        assert!(worker.update(Some(two)));
        assert_eq!(worker.shared.lock().unwrap().intent, Some(two));
        worker.shared.lock().unwrap().failed = Some(2);
        assert!(!worker.update(Some(two)));
        assert!(worker.update(None));
        assert!(worker.shared.lock().unwrap().intent.is_none());
        let last_epoch = worker.shared.lock().unwrap().last_epoch;
        assert_eq!(last_epoch, Some(2));
        assert!(worker.update(Some(one)));
        assert_eq!(worker.shared.lock().unwrap().intent, Some(one));
    }
}
