use crate::engine::*;
use crate::herdr::*;
use std::cell::RefCell;
use std::time::{Duration, Instant};

/// A Runner that answers each argv from a table and records what was asked.
pub struct Fake { pub calls: RefCell<Vec<Vec<String>>>, pub answers: Vec<(&'static str, Result<&'static str, &'static str>)> }

impl Fake {
    pub fn new(answers: Vec<(&'static str, Result<&'static str, &'static str>)>) -> Self { Fake { calls: RefCell::new(vec![]), answers } }
    pub fn calls(&self) -> Vec<String> { self.calls.borrow().iter().map(|c| c.join(" ")).collect() }
}

impl Runner for Fake {
    fn run(&self, argv: &[&str]) -> Result<String, String> {
        let joined = argv.join(" ");
        self.calls.borrow_mut().push(argv.iter().map(|s| s.to_string()).collect());
        for (prefix, answer) in &self.answers {
            if joined == *prefix || joined.starts_with(&format!("{prefix} ")) { return answer.map(String::from).map_err(String::from); }
        }
        Err(format!("fake: no answer for {joined}"))
    }
}

const PANE_CURRENT: &str = r#"{"result":{"panes":[{"agent":"claude","agent_status":"working","cwd":"/Users/p/Code","focused":true,"pane_id":"w1M:p1","revision":6,"tab_id":"w1M:t1","terminal_id":"term_65ed","workspace_id":"w1M"}]}}"#;
const PANE_CURRENT_SHELL: &str = r#"{"result":{"panes":[{"cwd":"/Users/p","focused":true,"pane_id":"w5:p2","revision":1,"tab_id":"w5:t1","terminal_id":"term_1","workspace_id":"w5"}]}}"#;
const PANE_CURRENT_UNFOCUSED: &str = r#"{"result":{"panes":[{"agent":"claude","agent_status":"working","cwd":"/Users/p/Code","focused":false,"pane_id":"w1M:p1","revision":6,"tab_id":"w1M:t1","terminal_id":"term_65ed","workspace_id":"w1M"}]}}"#;
const WORKSPACES: &str = r#"{"id":"cli:workspace:list","result":{"type":"workspace_list","workspaces":[{"active_tab_id":"w5:t1","agent_status":"idle","focused":false,"label":"meal-plan","number":1,"pane_count":4,"tab_count":1,"workspace_id":"w5"},{"active_tab_id":"w1M:t1","agent_status":"working","focused":true,"label":"Code","number":9,"pane_count":1,"tab_count":1,"workspace_id":"w1M"}]}}"#;
const TABS: &str = r#"{"id":"cli:tab:list","result":{"tabs":[{"agent_status":"idle","focused":false,"label":"1","number":1,"pane_count":4,"tab_id":"w7:t1","workspace_id":"w7"},{"agent_status":"unknown","focused":true,"label":"cursor-probe","number":7,"pane_count":2,"tab_id":"w7:t7","workspace_id":"w7"}],"type":"tab_list"}}"#;
const AGENTS: &str = r#"{"id":"cli:agent:list","result":{"agents":[{"agent":"claude","agent_status":"idle","focused":false,"pane_id":"w5:p1","revision":3,"tab_id":"w5:t1","workspace_id":"w5"},{"agent":"codex","agent_status":"done","focused":false,"name":"reviewer2","pane_id":"w0:p6","tab_id":"w0:t1","workspace_id":"w0"},{"agent":"claude","agent_status":"blocked","pane_id":"w7:p21","tab_id":"w7:t1","workspace_id":"w7"},{"agent":"grok","agent_status":"mystery","pane_id":"w9:p1","tab_id":"w9:t1","workspace_id":"w9"}],"type":"agent_list"}}"#;

fn fake() -> Fake {
    Fake::new(vec![
        ("herdr pane list", Ok(PANE_CURRENT)),
        ("herdr workspace list", Ok(WORKSPACES)),
        ("herdr tab list --workspace w1M", Ok(TABS)),
        ("herdr agent list", Ok(AGENTS)),
    ])
}

#[test]
fn snapshot_reads_the_focused_pane_spaces_tabs_and_agents() {
    let f = fake();
    let s = Herdr { runner: &f }.snapshot().unwrap();
    assert_eq!(s.focused, Some(Pane { pane_id: "w1M:p1".into(), workspace_id: "w1M".into(), tab_id: "w1M:t1".into(), agent: Some("claude".into()), status: Some(Status::Working) }));
    assert_eq!(s.workspaces, vec![
        Workspace { id: "w5".into(), number: 1, focused: false },
        Workspace { id: "w1M".into(), number: 9, focused: true },
    ]);
    assert_eq!(s.tabs, vec![Tab { id: "w7:t1".into(), number: 1, focused: false }, Tab { id: "w7:t7".into(), number: 7, focused: true }]);
    assert_eq!(s.agents[0], Agent { pane_id: "w5:p1".into(), workspace_id: "w5".into(), status: Status::Idle });
    assert_eq!(s.agents[1].status, Status::Done);
    assert_eq!(s.agents[2].status, Status::Blocked);
    assert_eq!(s.agents[3].status, Status::Unknown, "a status this build does not know is Unknown, not an error");
    assert_eq!(f.calls(), ["herdr pane list", "herdr workspace list", "herdr tab list --workspace w1M", "herdr agent list"], "tabs are listed for the focused pane's space");
}

#[test]
fn a_shell_pane_has_no_agent_and_no_status() {
    let mut f = fake();
    f.answers[0].1 = Ok(PANE_CURRENT_SHELL);
    f.answers.push(("herdr tab list --workspace w5", Ok(TABS)));
    let s = Herdr { runner: &f }.snapshot().unwrap();
    assert_eq!(s.focused, Some(Pane { pane_id: "w5:p2".into(), workspace_id: "w5".into(), tab_id: "w5:t1".into(), agent: None, status: None }));
}

#[test]
fn an_unfocused_current_pane_is_no_focused_pane() {
    // A list containing only unfocused panes provides no safe action target.
    let mut f = fake();
    f.answers[0].1 = Ok(PANE_CURRENT_UNFOCUSED);
    let s = Herdr { runner: &f }.snapshot().unwrap();
    assert_eq!(s.focused, None);
    assert_eq!(f.calls(), ["herdr pane list", "herdr workspace list", "herdr agent list"], "no focused pane, so no space to list tabs for");
}

#[test]
fn a_failed_command_fails_the_snapshot_with_its_stderr() {
    let mut f = fake();
    f.answers[1].1 = Err(r#"{"error":{"code":"server_unavailable","message":"no server"}}"#);
    let err = Herdr { runner: &f }.snapshot().unwrap_err();
    assert_eq!(err, "server_unavailable: no server");
}

#[test]
fn bad_json_is_an_error_naming_the_command() {
    let mut f = fake();
    f.answers[3].1 = Ok("not json");
    let err = Herdr { runner: &f }.snapshot().unwrap_err();
    assert!(err.starts_with("agent list: bad json"), "{err}");
}

#[test]
fn each_action_is_one_herdr_command() {
    let cases: Vec<(Action, &str)> = vec![
        (Action::FocusWorkspace("w7".into()), "herdr workspace focus w7"),
        (Action::FocusTab("w7:t7".into()), "herdr tab focus w7:t7"),
        (Action::FocusDirection { pane_id: "w5:p1".into(), dir: Dir::Left }, "herdr pane focus --direction left --pane w5:p1"),
        (Action::FocusDirection { pane_id: "w5:p1".into(), dir: Dir::Down }, "herdr pane focus --direction down --pane w5:p1"),
        (Action::FocusAgent("w0:p3".into()), "herdr agent focus w0:p3"),
        (Action::ToggleZoom("w5:p1".into()), "herdr pane zoom --pane w5:p1 --toggle"),
        (Action::SendKeys { target: "w5:p1".into(), keys: vec![Key::Up, Key::Enter] }, "herdr agent send-keys w5:p1 up enter"),
        (Action::SendKeys { target: "w5:p1".into(), keys: vec![Key::Esc] }, "herdr agent send-keys w5:p1 esc"),
        (Action::SendKeys { target: "w5:p1".into(), keys: vec![Key::Left, Key::Right, Key::Down] }, "herdr agent send-keys w5:p1 left right down"),
        (Action::Prompt { target: "w5:p1".into(), text: "run the tests".into() }, "herdr agent prompt w5:p1 run the tests"),
        (Action::SendText { pane_id: "w5:p1".into(), text: "also lint".into() }, "herdr pane send-text w5:p1 also lint"),
        (toast("Sent", "run the tests", Sound::None), "herdr notification show Sent --body run the tests --sound none"),
        (toast("Not sent", "x", Sound::Request), "herdr notification show Not sent --body x --sound request"),
        (toast("Navigate mode", "", Sound::Done), "herdr notification show Navigate mode --sound done"),
    ];
    for (action, expect) in cases {
        assert_eq!(argv_for(&action).join(" "), expect, "{action:?}");
    }
}

#[test]
fn prompt_text_is_one_argument_even_with_spaces() {
    let argv = argv_for(&Action::Prompt { target: "w5:p1".into(), text: "run the tests".into() });
    assert_eq!(argv.len(), 5, "{argv:?}");
}

#[test]
fn recording_actions_are_not_herdr_commands() {
    assert!(argv_for(&Action::StartRecording).is_empty());
    assert!(argv_for(&Action::StopRecording).is_empty());
    assert!(argv_for(&Action::CancelRecording).is_empty());
    let f = Fake::new(vec![]);
    Herdr { runner: &f }.apply(&Action::StartRecording).unwrap();
    assert!(f.calls().is_empty(), "apply must not run a command for them");
}

#[test]
fn apply_runs_the_command_and_surfaces_the_error() {
    let f = Fake::new(vec![("herdr agent send-keys", Err(r#"{"error":{"code":"agent_not_found","message":"agent target w5:p2 not found"}}"#))]);
    let err = Herdr { runner: &f }.apply(&Action::SendKeys { target: "w5:p2".into(), keys: vec![Key::Enter] }).unwrap_err();
    assert_eq!(err, "agent_not_found: agent target w5:p2 not found");
    let f = Fake::new(vec![("herdr agent send-keys", Err("boom"))]);
    let err = Herdr { runner: &f }.apply(&Action::SendKeys { target: "w5:p2".into(), keys: vec![Key::Enter] }).unwrap_err();
    assert_eq!(err, "boom", "stderr that is not Herdr's JSON is passed through");
}

#[test]
fn missing_result_objects_parse_to_nothing() {
    let f = Fake::new(vec![
        ("herdr pane list", Ok(r#"{"result":{}}"#)),
        ("herdr workspace list", Ok(r#"{"result":{}}"#)),
        ("herdr tab list", Ok(r#"{"result":{}}"#)),
        ("herdr agent list", Ok(r#"{"result":{}}"#)),
    ]);
    let s = Herdr { runner: &f }.snapshot().unwrap();
    assert_eq!(s.focused, None);
    assert!(s.workspaces.is_empty(), "{:?}", s.workspaces);
    assert!(s.tabs.is_empty(), "{:?}", s.tabs);
    assert!(s.agents.is_empty(), "{:?}", s.agents);
    assert_eq!(f.calls(), ["herdr pane list", "herdr workspace list", "herdr agent list"], "no focused pane, so no space to list tabs for");
}

#[test]
fn an_error_without_a_code_is_passed_through_verbatim() {
    let raw = r#"{"error":{"message":"x"}}"#;
    let f = Fake::new(vec![("herdr agent send-keys", Err(raw))]);
    let err = Herdr { runner: &f }.apply(&Action::SendKeys { target: "w5:p2".into(), keys: vec![Key::Enter] }).unwrap_err();
    assert_eq!(err, raw);
}

#[test]
fn hyphen_leading_text_is_passed_as_is_without_a_double_dash() {
    // Probed herdr 0.9.1 on 2026-10-27: `agent prompt`, `pane send-text` and `notification show --body` take "- hi there" and "--wait" as text, and a `--` before the text fails with "unknown option".
    for text in ["- hi there", "--wait"] {
        let prompt = argv_for(&Action::Prompt { target: "w5:p1".into(), text: text.into() });
        let send_text = argv_for(&Action::SendText { pane_id: "w5:p1".into(), text: text.into() });
        let toast = argv_for(&toast("Sent", text, Sound::None));
        assert_eq!(prompt.last().map(String::as_str), Some(text), "{prompt:?}");
        assert_eq!(send_text.last().map(String::as_str), Some(text), "{send_text:?}");
        assert_eq!(toast.iter().position(|a| a == "--body").map(|i| toast[i + 1].as_str()), Some(text), "{toast:?}");
        for argv in [&prompt, &send_text, &toast] {
            assert!(!argv.iter().any(|a| a == "--"), "{argv:?}");
        }
    }
}

// The real runner, exercised hermetically through /bin/sh.

#[test]
fn cli_returns_stdout_on_success() {
    assert_eq!(Cli::default().run(&["sh", "-c", "echo hi"]), Ok("hi\n".to_string()));
}

#[test]
fn cli_returns_stderr_on_failure() {
    assert_eq!(Cli::default().run(&["sh", "-c", "echo bad >&2; exit 1"]), Err("bad".to_string()));
}

#[test]
fn cli_reports_a_missing_binary() {
    let err = Cli::default().run(&["/nonexistent/xyz"]).unwrap_err();
    assert!(err.contains("xyz"), "{err}");
}

#[test]
fn cli_falls_back_to_the_exit_status_when_stderr_is_empty() {
    let err = Cli::default().run(&["sh", "-c", "exit 3"]).unwrap_err();
    assert!(!err.is_empty() && err.contains("3"), "{err:?}");
}

#[test]
fn cli_rejects_empty_argv() {
    assert_eq!(Cli::default().run(&[]), Err("empty argv".to_string()));
}

#[test]
fn cli_times_out() {
    let started = Instant::now();
    let err = run_with_timeout(&["sh", "-c", "sleep 5"], Duration::from_millis(200)).unwrap_err();
    let took = started.elapsed();
    assert!(err.contains("timed out"), "{err}");
    assert!(took < Duration::from_secs(2), "took {took:?}");
    eprintln!("cli_times_out took {took:?}");
}

#[test]
fn cli_timeout_is_configurable() {
    let started = Instant::now();
    let err = Cli { timeout: Duration::from_millis(200) }.run(&["sh", "-c", "sleep 5"]).unwrap_err();
    let took = started.elapsed();
    assert!(err.contains("timed out"), "{err}");
    assert!(took < Duration::from_secs(2), "took {took:?}");
}

#[test]
fn cli_deadline_covers_pipes_inherited_after_parent_exits() {
    for script in ["sleep 2 >&2 & exit 0", "sleep 2 2>/dev/null & exit 0"] {
        let started = Instant::now();
        let result = run_with_timeout(&["/bin/sh", "-c", script], Duration::from_millis(100));
        assert!(matches!(&result, Err(e) if e.contains("timed out")), "{result:?}");
        assert!(started.elapsed() < Duration::from_secs(1), "deadline bypassed: {:?}", started.elapsed());
    }
}

fn check_timeout_cleans_descendants(wait_for_child: bool) {
    let path = std::env::temp_dir().join(format!("gamepad-descendant-{}-{wait_for_child}", std::process::id()));
    let script = if wait_for_child { "sleep 5 & echo $! > \"$1\"; wait" } else { "sleep 5 & echo $! > \"$1\"; exit 0" };
    let result = run_with_timeout(&["/bin/sh", "-c", script, "review", path.to_str().unwrap()], Duration::from_millis(200));
    let pid: i32 = std::fs::read_to_string(&path).unwrap().trim().parse().unwrap();
    std::fs::remove_file(path).unwrap();
    let until = Instant::now() + Duration::from_secs(1);
    while unsafe { libc::kill(pid, 0) } == 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    let survived = unsafe { libc::kill(pid, 0) } == 0;
    if survived { unsafe { libc::kill(pid, libc::SIGKILL); } }
    assert!(matches!(result, Err(ref e) if e.contains("timed out")), "{result:?}");
    assert!(!survived, "descendant {pid} survived the command timeout");
}

#[test]
fn timeout_cleans_descendants_after_parent_exit() { check_timeout_cleans_descendants(false); }

#[test]
fn timeout_cleans_descendants_while_parent_waits() { check_timeout_cleans_descendants(true); }

#[test]
fn daemon_snapshot_follows_ui_focus_instead_of_its_own_pane() {
    // Minimized from pane current/list queried with the live daemon's caller context.
    let daemon = r#"{"result":{"pane":{"pane_id":"w1M:p7","workspace_id":"w1M","tab_id":"w1M:t2","focused":false}}}"#;
    let panes = r#"{"result":{"panes":[{"pane_id":"w1M:p7","workspace_id":"w1M","tab_id":"w1M:t2","focused":false},{"pane_id":"w1M:p4","workspace_id":"w1M","tab_id":"w1M:t1","focused":true,"agent":"codex","agent_status":"idle"}]}}"#;
    let tabs = r#"{"result":{"tabs":[{"tab_id":"w1M:t1","number":1,"focused":true},{"tab_id":"w1M:t2","number":2,"focused":false}]}}"#;
    let f = Fake::new(vec![
        ("herdr pane current", Ok(daemon)),
        ("herdr pane list", Ok(panes)),
        ("herdr workspace list", Ok(WORKSPACES)),
        ("herdr tab list --workspace w1M", Ok(tabs)),
        ("herdr agent list", Ok(AGENTS)),
    ]);
    let snap = Herdr { runner: &f }.snapshot().unwrap();
    assert_eq!(snap.focused.as_ref().map(|p| p.pane_id.as_str()), Some("w1M:p4"));
    assert_eq!(Core::new(Btn::RStick).handle(Input::Press(Btn::Right), &snap), vec![Action::FocusDirection { pane_id: "w1M:p4".into(), dir: Dir::Right }]);
    assert_eq!(Core::new(Btn::RStick).handle(Input::Press(Btn::Rt), &snap), vec![Action::FocusTab("w1M:t2".into())]);
    let mut core = Core::new(Btn::RStick);
    core.handle(Input::Press(Btn::A), &snap);
    assert_eq!(core.mode(), Mode::Respond);
    core.handle(Input::Press(Btn::View), &snap);
    assert_eq!(core.mode(), Mode::Navigate);
}

#[test]
fn cancellation_interrupts_a_running_command_and_cleans_it_up() {
    let started = Instant::now();
    let result = run_with_cancellation(&["/bin/sh", "-c", "sleep 5"], Duration::from_secs(10), || started.elapsed() >= Duration::from_millis(100));
    assert!(matches!(&result, Err(e) if e.contains("cancelled")), "{result:?}");
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn cancellation_before_spawn_is_reported_without_starting_a_command() {
    let result = run_with_cancellation(&["/nonexistent/never-spawn"], Duration::from_secs(10), || true);
    assert_eq!(result, Err("command cancelled".into()));
}
