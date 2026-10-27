//! Herdr through the `herdr` CLI: a Snapshot of what is focused, and each Action as one command.

use crate::engine::*;
use serde_json::Value;
use std::io::{self, Read};
use std::os::unix::io::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
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
        .process_group(0)
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().map_err(|e| format!("{name}: {e}"))?;
    let deadline = Instant::now() + timeout;
    // Own and drain both pipes here: returning closes them, even if a descendant
    // escaped the process group. No detached reader threads can outlive the call.
    let result = (|| {
        let mut stdout = child.stdout.take().expect("stdout was piped");
        let mut stderr = child.stderr.take().expect("stderr was piped");
        nonblocking(&stdout).and_then(|_| nonblocking(&stderr)).map_err(|e| format!("{name}: {e}"))?;
        let (mut out, mut err) = (Vec::new(), Vec::new());
        loop {
            if Instant::now() >= deadline { return Err(format!("{name}: timed out after {timeout:?}")); }
            let out_done = drain_available(&mut stdout, &mut out).map_err(|e| format!("{name}: stdout: {e}"))?;
            let err_done = drain_available(&mut stderr, &mut err).map_err(|e| format!("{name}: stderr: {e}"))?;
            let status = child.try_wait().map_err(|e| format!("{name}: {e}"))?;
            if let Some(status) = status.filter(|_| out_done && err_done) {
                let stdout = String::from_utf8_lossy(&out).into_owned();
                let stderr = String::from_utf8_lossy(&err).trim().to_string();
                return if status.success() { Ok(stdout) }
                    else if stderr.is_empty() { Err(format!("{name}: {status}")) }
                    else { Err(stderr) };
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    if result.is_err() {
        // Each invocation gets its own process group, so timeout also terminates
        // descendants holding the pipes after their direct parent has exited.
        unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL); }
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

fn nonblocking(pipe: &impl AsRawFd) -> io::Result<()> {
    let fd = pipe.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags == -1 { return Err(io::Error::last_os_error()); }
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// A bounded batch prevents a continuously writing child from starving the
/// other pipe or the deadline. True means EOF; WouldBlock means try again later.
fn drain_available(pipe: &mut impl Read, buf: &mut Vec<u8>) -> io::Result<bool> {
    let mut chunk = [0; 8192];
    for _ in 0..16 {
        match pipe.read(&mut chunk) {
            Ok(0) => return Ok(true),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(false),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
    Ok(false)
}

pub struct Herdr<R: Runner> { pub runner: R }

impl<R: Runner> Herdr<R> {
    pub fn snapshot(&self) -> Result<Snapshot, String> {
        let focused = parse_pane(&self.json(&["herdr", "pane", "list"])?);
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
    // `pane current` resolves the daemon's inherited caller pane, not UI focus.
    let p = v["result"]["panes"].as_array()?.iter()
        .find(|p| p["focused"].as_bool() == Some(true))?;
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
        Action::StartRecording | Action::StopRecording | Action::CancelRecording => vec![],
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
