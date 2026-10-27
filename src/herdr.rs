//! Herdr through the `herdr` CLI: a Snapshot of what is focused, and each Action as one command.

use crate::engine::*;
use serde_json::Value;
use std::io::Read;
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Runs a command; Ok(stdout) on exit 0, Err(stderr) otherwise. Faked in tests.
pub trait Runner {
    fn run(&self, argv: &[&str]) -> Result<String, String>;
}

impl<R: Runner> Runner for &R {
    fn run(&self, argv: &[&str]) -> Result<String, String> { (*self).run(argv) }
}

/// The real runner: a child process, killed if it outlives `timeout`.
pub struct Cli { pub timeout: Duration }

impl Default for Cli {
    fn default() -> Self { Cli { timeout: Duration::from_secs(10) } }
}

impl Runner for Cli {
    fn run(&self, argv: &[&str]) -> Result<String, String> { run_with_timeout(argv, self.timeout) }
}

/// Runs argv: Ok(stdout) on exit 0; Err(stderr, or the exit status when stderr is empty) otherwise.
/// A command still running at the deadline is killed and reaped, so a wedged Herdr server cannot freeze the daemon.
pub fn run_with_timeout(argv: &[&str], timeout: Duration) -> Result<String, String> {
    let name = *argv.first().ok_or("empty argv")?;
    let mut child = Command::new(name).args(&argv[1..])
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().map_err(|e| format!("{name}: {e}"))?;
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            Ok(None) => {
                reap(&mut child);
                // The drain threads end when the pipes close, which killing the child does; they are not joined
                // here because a grandchild that inherited a pipe could hold them open until it exits.
                return Err(format!("{name}: timed out after {}s", timeout.as_secs()));
            }
            Err(e) => {
                reap(&mut child);
                return Err(format!("{name}: {e}"));
            }
        }
    };
    let stdout = String::from_utf8_lossy(&stdout.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&stderr.join().unwrap_or_default()).trim().to_string();
    if status.success() {
        Ok(stdout)
    } else if stderr.is_empty() {
        Err(format!("{name}: {status}"))
    } else {
        Err(stderr)
    }
}

/// Kills and waits for a child so it cannot linger as a zombie.
fn reap(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Reads a pipe to its end on its own thread, so a full pipe can never block the child.
fn drain<P: Read + Send + 'static>(pipe: Option<P>) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut p) = pipe { let _ = p.read_to_end(&mut buf); }
        buf
    })
}

pub struct Herdr<R: Runner> { pub runner: R }

impl<R: Runner> Herdr<R> {
    pub fn snapshot(&self) -> Result<Snapshot, String> {
        let focused = parse_pane(&self.json(&["herdr", "pane", "current"])?);
        let workspaces = parse_workspaces(&self.json(&["herdr", "workspace", "list"])?);
        let tabs = match &focused {
            Some(p) => parse_tabs(&self.json(&["herdr", "tab", "list", "--workspace", &p.workspace_id])?),
            None => vec![],
        };
        let agents = parse_agents(&self.json(&["herdr", "agent", "list"])?);
        Ok(Snapshot { focused, workspaces, tabs, agents })
    }

    pub fn apply(&self, action: &Action) -> Result<(), String> {
        let argv = argv_for(action);
        if argv.is_empty() { return Ok(()); }
        let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        self.runner.run(&refs).map(|_| ()).map_err(|e| error_message(&e))
    }

    fn json(&self, argv: &[&str]) -> Result<Value, String> {
        let out = self.runner.run(argv).map_err(|e| error_message(&e))?;
        serde_json::from_str(&out).map_err(|e| format!("{}: bad json: {e}", argv.get(1..).unwrap_or(&[]).join(" ")))
    }
}

/// Herdr's JSON error as "code: message"; anything else as it came.
fn error_message(stderr: &str) -> String {
    match serde_json::from_str::<Value>(stderr) {
        Ok(v) if v["error"]["code"].is_string() => format!("{}: {}", text(&v["error"], "code"), text(&v["error"], "message")),
        _ => stderr.to_string(),
    }
}

fn status(v: &Value) -> Option<Status> {
    Some(match v.as_str()? {
        "idle" => Status::Idle,
        "working" => Status::Working,
        "blocked" => Status::Blocked,
        "done" => Status::Done,
        _ => Status::Unknown,
    })
}

fn text(v: &Value, key: &str) -> String { v[key].as_str().unwrap_or_default().to_string() }

fn number(v: &Value) -> u32 { v.as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0) }

fn parse_pane(v: &Value) -> Option<Pane> {
    let p = &v["result"]["pane"];
    if !p.is_object() { return None; }
    Some(Pane {
        pane_id: text(p, "pane_id"),
        workspace_id: text(p, "workspace_id"),
        tab_id: text(p, "tab_id"),
        agent: p["agent"].as_str().map(String::from),
        status: status(&p["agent_status"]),
    })
}

fn parse_workspaces(v: &Value) -> Vec<Workspace> {
    v["result"]["workspaces"].as_array().map(|a| a.iter().map(|w| Workspace {
        id: text(w, "workspace_id"),
        number: number(&w["number"]),
        focused: w["focused"].as_bool().unwrap_or(false),
    }).collect()).unwrap_or_default()
}

fn parse_tabs(v: &Value) -> Vec<Tab> {
    v["result"]["tabs"].as_array().map(|a| a.iter().map(|t| Tab {
        id: text(t, "tab_id"),
        number: number(&t["number"]),
        focused: t["focused"].as_bool().unwrap_or(false),
    }).collect()).unwrap_or_default()
}

fn parse_agents(v: &Value) -> Vec<Agent> {
    v["result"]["agents"].as_array().map(|a| a.iter().map(|g| Agent {
        pane_id: text(g, "pane_id"),
        workspace_id: text(g, "workspace_id"),
        status: status(&g["agent_status"]).unwrap_or(Status::Unknown),
    }).collect()).unwrap_or_default()
}

/// The herdr command for an action; empty for the recording actions, which serve handles itself.
pub fn argv_for(a: &Action) -> Vec<String> {
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    match a {
        Action::FocusWorkspace(id) => s(&["herdr", "workspace", "focus", id]),
        Action::FocusTab(id) => s(&["herdr", "tab", "focus", id]),
        Action::FocusDirection { pane_id, dir } => s(&["herdr", "pane", "focus", "--direction", dir_name(*dir), "--pane", pane_id]),
        Action::FocusAgent(target) => s(&["herdr", "agent", "focus", target]),
        Action::ToggleZoom(pane_id) => s(&["herdr", "pane", "zoom", "--pane", pane_id, "--toggle"]),
        Action::SendKeys { target, keys } => {
            let mut v = s(&["herdr", "agent", "send-keys", target]);
            v.extend(keys.iter().map(|k| key_name(*k).to_string()));
            v
        }
        Action::Prompt { target, text } => s(&["herdr", "agent", "prompt", target, text]),
        Action::SendText { pane_id, text } => s(&["herdr", "pane", "send-text", pane_id, text]),
        Action::Toast { title, body, sound } => {
            let mut v = s(&["herdr", "notification", "show", title]);
            if !body.is_empty() { v.extend(s(&["--body", body])); }
            v.extend(s(&["--sound", sound_name(*sound)]));
            v
        }
        Action::StartRecording | Action::StopRecording => vec![],
    }
}

fn dir_name(d: Dir) -> &'static str {
    match d { Dir::Left => "left", Dir::Right => "right", Dir::Up => "up", Dir::Down => "down" }
}

fn key_name(k: Key) -> &'static str {
    match k { Key::Up => "up", Key::Down => "down", Key::Left => "left", Key::Right => "right", Key::Enter => "enter", Key::Esc => "esc" }
}

fn sound_name(s: Sound) -> &'static str {
    match s { Sound::None => "none", Sound::Done => "done", Sound::Request => "request" }
}
