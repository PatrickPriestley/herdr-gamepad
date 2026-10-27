//! Herdr through the `herdr` CLI: a Snapshot of what is focused, and each Action as one command.

use crate::engine::*;
use serde_json::Value;

/// Runs a command; Ok(stdout) on exit 0, Err(stderr) otherwise. Faked in tests.
pub trait Runner {
    fn run(&self, argv: &[&str]) -> Result<String, String>;
}

impl<R: Runner> Runner for &R {
    fn run(&self, argv: &[&str]) -> Result<String, String> { (*self).run(argv) }
}

pub struct Cli;

impl Runner for Cli {
    fn run(&self, argv: &[&str]) -> Result<String, String> {
        let out = std::process::Command::new(argv[0]).args(&argv[1..]).output()
            .map_err(|e| format!("{}: {e}", argv[0]))?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }
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

    fn json(&self, argv: &[&str]) -> Result<Value, String> {
        let out = self.runner.run(argv)?;
        serde_json::from_str(&out).map_err(|e| format!("{}: bad json: {e}", argv[1..].join(" ")))
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

pub fn parse_pane(v: &Value) -> Option<Pane> {
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

pub fn parse_workspaces(v: &Value) -> Vec<Workspace> {
    v["result"]["workspaces"].as_array().map(|a| a.iter().map(|w| Workspace {
        id: text(w, "workspace_id"),
        number: w["number"].as_u64().unwrap_or(0) as u32,
        focused: w["focused"].as_bool().unwrap_or(false),
    }).collect()).unwrap_or_default()
}

pub fn parse_tabs(v: &Value) -> Vec<Tab> {
    v["result"]["tabs"].as_array().map(|a| a.iter().map(|t| Tab {
        id: text(t, "tab_id"),
        number: t["number"].as_u64().unwrap_or(0) as u32,
        focused: t["focused"].as_bool().unwrap_or(false),
    }).collect()).unwrap_or_default()
}

pub fn parse_agents(v: &Value) -> Vec<Agent> {
    v["result"]["agents"].as_array().map(|a| a.iter().map(|g| Agent {
        pane_id: text(g, "pane_id"),
        workspace_id: text(g, "workspace_id"),
        status: status(&g["agent_status"]).unwrap_or(Status::Unknown),
    }).collect()).unwrap_or_default()
}

impl<R: Runner> Herdr<R> {
    pub fn apply(&self, action: &Action) -> Result<(), String> {
        let argv = argv_for(action);
        if argv.is_empty() { return Ok(()); }
        let refs: Vec<&str> = argv.iter().map(String::as_str).collect();
        self.runner.run(&refs).map(|_| ()).map_err(|e| error_message(&e))
    }
}

/// Herdr's JSON error as "code: message"; anything else as it came.
fn error_message(stderr: &str) -> String {
    match serde_json::from_str::<Value>(stderr) {
        Ok(v) if v["error"]["code"].is_string() => format!("{}: {}", text(&v["error"], "code"), text(&v["error"], "message")),
        _ => stderr.to_string(),
    }
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
