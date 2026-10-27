use crate::engine::*;
use crate::herdr::*;
use std::cell::RefCell;

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
            if joined.starts_with(prefix) { return answer.map(String::from).map_err(String::from); }
        }
        Err(format!("fake: no answer for {joined}"))
    }
}

const PANE_CURRENT: &str = r#"{"id":"cli:pane:current","result":{"pane":{"agent":"claude","agent_status":"working","cwd":"/Users/p/Code","focused":true,"pane_id":"w1M:p1","revision":6,"tab_id":"w1M:t1","terminal_id":"term_65ed","workspace_id":"w1M"},"type":"pane_current"}}"#;
const PANE_CURRENT_SHELL: &str = r#"{"id":"cli:pane:current","result":{"pane":{"cwd":"/Users/p","focused":true,"pane_id":"w5:p2","revision":1,"tab_id":"w5:t1","terminal_id":"term_1","workspace_id":"w5"},"type":"pane_current"}}"#;
const WORKSPACES: &str = r#"{"id":"cli:workspace:list","result":{"type":"workspace_list","workspaces":[{"active_tab_id":"w5:t1","agent_status":"idle","focused":false,"label":"meal-plan","number":1,"pane_count":4,"tab_count":1,"workspace_id":"w5"},{"active_tab_id":"w1M:t1","agent_status":"working","focused":true,"label":"Code","number":9,"pane_count":1,"tab_count":1,"workspace_id":"w1M"}]}}"#;
const TABS: &str = r#"{"id":"cli:tab:list","result":{"tabs":[{"agent_status":"idle","focused":false,"label":"1","number":1,"pane_count":4,"tab_id":"w7:t1","workspace_id":"w7"},{"agent_status":"unknown","focused":true,"label":"cursor-probe","number":7,"pane_count":2,"tab_id":"w7:t7","workspace_id":"w7"}],"type":"tab_list"}}"#;
const AGENTS: &str = r#"{"id":"cli:agent:list","result":{"agents":[{"agent":"claude","agent_status":"idle","focused":false,"pane_id":"w5:p1","revision":3,"tab_id":"w5:t1","workspace_id":"w5"},{"agent":"codex","agent_status":"done","focused":false,"name":"reviewer2","pane_id":"w0:p6","tab_id":"w0:t1","workspace_id":"w0"},{"agent":"claude","agent_status":"blocked","pane_id":"w7:p21","tab_id":"w7:t1","workspace_id":"w7"},{"agent":"grok","agent_status":"mystery","pane_id":"w9:p1","tab_id":"w9:t1","workspace_id":"w9"}],"type":"agent_list"}}"#;

fn fake() -> Fake {
    Fake::new(vec![
        ("herdr pane current", Ok(PANE_CURRENT)),
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
    assert_eq!(f.calls(), ["herdr pane current", "herdr workspace list", "herdr tab list --workspace w1M", "herdr agent list"], "tabs are listed for the focused pane's space");
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
fn a_failed_command_fails_the_snapshot_with_its_stderr() {
    let mut f = fake();
    f.answers[1].1 = Err(r#"{"error":{"code":"server_unavailable","message":"no server"}}"#);
    let err = Herdr { runner: &f }.snapshot().unwrap_err();
    assert!(err.contains("server_unavailable"), "{err}");
}

#[test]
fn bad_json_is_an_error_naming_the_command() {
    let mut f = fake();
    f.answers[3].1 = Ok("not json");
    let err = Herdr { runner: &f }.snapshot().unwrap_err();
    assert!(err.starts_with("agent list: bad json"), "{err}");
}
