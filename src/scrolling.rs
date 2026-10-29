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
    pub agent_pages: bool,
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
            agent_pages: false,
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
    pub agent_pages: bool,
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
    fn set_agent_pages(&mut self, _enabled: bool) {}
    fn focused(&mut self) -> Result<String, String>;
    fn scroll(
        &mut self,
        pane: &str,
        steps: i8,
        still_current: &dyn Fn() -> bool,
    ) -> Result<(), String>;
}
#[derive(Default)]
pub(crate) struct SocketBackend {
    pub agent_pages: bool,
    pub page_due: Option<Instant>,
    pub binding: Option<Binding>,
    pub path: Option<PathBuf>,
}
impl Backend for SocketBackend {
    fn set_agent_pages(&mut self, enabled: bool) {
        self.agent_pages = enabled;
    }
    fn focused(&mut self) -> Result<String, String> {
        let path = self.path.as_ref().ok_or("HERDR_SOCKET_PATH is missing")?;
        let result = crate::scroll_rpc::call(path, "pane.list", json!({}))?;
        if result.get("type").and_then(|v| v.as_str()) != Some("pane_list") {
            return Err("Unexpected pane list response".into());
        }
        let pane = result
            .get("panes")
            .and_then(|v| v.as_array())
            .and_then(|panes| {
                panes
                    .iter()
                    .find(|pane| pane.get("focused").and_then(|v| v.as_bool()) == Some(true))
            })
            .ok_or("No pane is selected")?;
        let id = pane
            .get("pane_id")
            .and_then(|v| v.as_str())
            .filter(|id| !id.is_empty())
            .ok_or("No pane is selected")?;
        self.binding = Some(Binding::read(pane)?);
        self.page_due = None;
        Ok(id.to_owned())
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
        let identity = Binding::read(current)?;
        if let Some(bound) = &self.binding {
            if bound != &identity {
                return Err("Scroll target changed".into());
            }
        } else {
            self.binding = Some(identity.clone());
        }
        let (offset, max) = scroll_metrics(current)?;
        if self.agent_pages && matches!(identity.agent.as_deref(), Some("codex" | "claude")) {
            if self.page_due.is_some_and(|due| Instant::now() < due) {
                return Ok(());
            }
            if !agent_ready(path, pane, &identity, still_current)? {
                return Ok(());
            }
            if offset != 0 {
                let Some(reset) = crate::scroll_rpc::call_if_current(
                    path,
                    "pane.scroll",
                    json!({"pane_id":pane,"offset_from_bottom":0}),
                    still_current,
                )?
                else {
                    return Ok(());
                };
                let reset = scroll_pane(&reset, pane)?;
                if Binding::read(reset)? != identity
                    || reset.get("focused").and_then(|v| v.as_bool()) != Some(true)
                {
                    return Err("Scroll target changed".into());
                }
                if scroll_metrics(reset)?.0 != 0 {
                    return Err("Host scrollback did not return to the application".into());
                }
                if !agent_ready(path, pane, &identity, still_current)? {
                    return Ok(());
                }
            }
            let key = if steps > 0 { "pageup" } else { "pagedown" };
            let Some(reply) = crate::scroll_rpc::call_if_current(
                path,
                "agent.send_keys",
                json!({"target":pane,"keys":[key]}),
                still_current,
            )?
            else {
                return Ok(());
            };
            if reply.get("type").and_then(|v| v.as_str()) != Some("ok") {
                return Err("Unexpected agent key response".into());
            }
            self.page_due = Some(
                Instant::now()
                    + Duration::from_millis(match steps.unsigned_abs() {
                        3.. => 300,
                        2 => 450,
                        _ => 600,
                    }),
            );
            return Ok(());
        }
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

/// Freeze the terminal occupant for the lifetime of a gesture, including known session metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Binding {
    terminal: String,
    agent: Option<String>,
    session: serde_json::Value,
}
impl Binding {
    fn read(pane: &serde_json::Value) -> Result<Self, String> {
        let terminal = pane
            .get("terminal_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .ok_or("Missing scroll terminal identity")?
            .to_owned();
        let agent = match pane.get("agent") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::String(agent)) => Some(agent.clone()),
            _ => return Err("Invalid scroll agent identity".into()),
        };
        Ok(Self {
            terminal,
            agent,
            session: pane
                .get("agent_session")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        })
    }
}

fn agent_ready(
    path: &std::path::Path,
    pane: &str,
    identity: &Binding,
    current: &dyn Fn() -> bool,
) -> Result<bool, String> {
    let Some(reply) =
        crate::scroll_rpc::call_if_current(path, "agent.get", json!({"target":pane}), current)?
    else {
        return Ok(false);
    };
    if reply.get("type").and_then(|v| v.as_str()) != Some("agent_info") {
        return Err("Unexpected agent state response".into());
    }
    let agent = reply.get("agent").ok_or("Missing agent state")?;
    if agent.get("pane_id").and_then(|v| v.as_str()) != Some(pane)
        || agent.get("focused").and_then(|v| v.as_bool()) != Some(true)
        || Binding::read(agent)? != *identity
    {
        return Err("Scroll target changed".into());
    }
    let false_or_absent = |field| {
        matches!(
            agent.get(field),
            None | Some(serde_json::Value::Bool(false))
        )
    };
    if !matches!(
        agent.get("agent_status").and_then(|v| v.as_str()),
        Some("idle" | "working" | "done")
    ) || !false_or_absent("screen_detection_skipped")
        || !false_or_absent("launch_pending")
    {
        return Err("Agent scrolling paused: close dialogs and return to the conversation".into());
    }
    Ok(true)
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
            backend.set_agent_pages(latest.agent_pages);
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
            let mut backend = SocketBackend {
                path,
                ..Default::default()
            };
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
    pub fn configure_agent_pages(&self, enabled: bool) {
        let mut state = self.shared.lock().expect("scroll state");
        if state.agent_pages != enabled {
            state.agent_pages = enabled;
            state.failed = state.last_epoch;
            state.last_epoch = None;
            state.intent = None;
            state.notice = None;
        }
    }
    /// False means this gesture failed and must be released before retrying.
    pub fn update(&self, intent: Option<Intent>) -> bool {
        let mut state = self.shared.lock().expect("scroll state");
        if intent.is_some_and(|intent| state.failed == Some(intent.epoch)) {
            return false;
        }
        let agent_pages = state.agent_pages;
        state.publish(intent.map(|mut intent| {
            intent.agent_pages = agent_pages;
            intent
        }));
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
            agent_pages: false,
            epoch: 1,
            steps: 1,
            expires: now,
        };
        let two = Intent {
            agent_pages: false,
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

#[cfg(test)]
mod setting_tests {
    use super::*;
    #[test]
    fn changed_setting_cancels_old_intent_and_requires_a_new_gesture() {
        let worker = Worker {
            shared: Arc::new(Mutex::new(Shared::default())),
            thread: None,
        };
        let mut stick = Stick::default();
        stick.observe(&Input::ScrollAxis(90));
        let old = stick.intent(Instant::now());
        assert!(worker.update(old));
        worker.configure_agent_pages(true);
        assert!(worker.shared.lock().unwrap().intent.is_none());
        assert!(!worker.update(old));
        stick.suppress();
        stick.observe(&Input::ScrollAxis(90));
        assert!(stick.intent(Instant::now()).is_none());
        stick.observe(&Input::ScrollAxis(0));
        stick.observe(&Input::ScrollAxis(90));
        assert!(worker.update(stick.intent(Instant::now())));
        assert!(worker.shared.lock().unwrap().intent.unwrap().agent_pages);
        worker.configure_agent_pages(true);
        assert!(
            worker.shared.lock().unwrap().intent.is_some(),
            "unchanged preference cancelled gesture"
        );
        worker.configure_agent_pages(false);
        assert!(worker.shared.lock().unwrap().intent.is_none());
        stick.observe(&Input::ScrollAxis(0));
        stick.observe(&Input::ScrollAxis(90));
        assert!(worker.update(stick.intent(Instant::now())));
        assert!(!worker.shared.lock().unwrap().intent.unwrap().agent_pages);
    }
    #[test]
    fn setting_change_invalidates_inflight_write_and_its_late_error() {
        let worker = Worker {
            shared: Arc::new(Mutex::new(Shared::default())),
            thread: None,
        };
        struct Change<'a>(&'a Worker);
        impl Backend for Change<'_> {
            fn focused(&mut self) -> Result<String, String> {
                Ok("pane".into())
            }
            fn scroll(&mut self, _: &str, _: i8, current: &dyn Fn() -> bool) -> Result<(), String> {
                assert!(current());
                self.0.configure_agent_pages(true);
                assert!(!current(), "old write permission survived setting change");
                Err("old failure".into())
            }
        }
        let now = Instant::now();
        worker.shared.lock().unwrap().notice = Some("old notice".into());
        worker.update(Some(Intent {
            epoch: 1,
            agent_pages: false,
            steps: 1,
            expires: now + Duration::from_secs(1),
        }));
        Pump::default().tick(&worker.shared, &mut Change(&worker), now);
        assert!(
            worker.take_notice().is_none(),
            "late error survived setting change"
        );
    }
    #[test]
    fn pump_uses_settings_from_the_current_intent() {
        struct Capture(Vec<bool>, bool);
        impl Backend for Capture {
            fn set_agent_pages(&mut self, v: bool) {
                self.1 = v;
            }
            fn focused(&mut self) -> Result<String, String> {
                Ok("pane".into())
            }
            fn scroll(&mut self, _: &str, _: i8, _: &dyn Fn() -> bool) -> Result<(), String> {
                self.0.push(self.1);
                Ok(())
            }
        }
        let now = Instant::now();
        let state = Mutex::new(Shared::default());
        let mut pump = Pump::default();
        let mut backend = Capture(vec![], false);
        for (epoch, enabled) in [(1, true), (2, false)] {
            state.lock().unwrap().publish(Some(Intent {
                epoch,
                agent_pages: enabled,
                steps: 1,
                expires: now + Duration::from_secs(1),
            }));
            pump.tick(&state, &mut backend, now);
        }
        assert_eq!(backend.0, [true, false]);
    }
}
