//! Validated menu I/O. Targets are frozen by the model and revalidated before dispatch.
use crate::{herdr::Runner, menu::*};
use serde_json::Value;
use std::path::Path;
pub fn context(r: &impl Runner) -> Result<Option<Context>, String> {
    // Read focused pane identity last: workspace metadata can be slower.
    let spaces = json(r, &["herdr", "workspace", "list"])?;
    let panes = json(r, &["herdr", "pane", "list"])?;
    parse_context(&panes, &spaces)
}
pub fn parse_context(panes: &Value, spaces: &Value) -> Result<Option<Context>, String> {
    let panes = array(&panes["result"], "panes")?;
    let spaces = array(&spaces["result"], "workspaces")?;
    let mut focused = None;
    for p in panes {
        for key in ["pane_id", "workspace_id", "tab_id"] {
            required(p, key)?;
        }
        if boolean(p, "focused")? {
            if focused.is_some() {
                return Err("Multiple focused panes".into());
            }
            focused = Some(p);
        }
    }
    let mut focused_spaces = 0;
    let mut space_ids = std::collections::HashSet::new();
    for w in spaces {
        if !space_ids.insert(required(w, "workspace_id")?) {
            return Err("Duplicate workspace ID".into());
        }
        required(w, "label")?;
        if boolean(w, "focused")? {
            focused_spaces += 1;
        }
        if focused_spaces > 1 {
            return Err("Multiple focused workspaces".into());
        }
    }
    let Some(p) = focused else { return Ok(None) };
    let workspace = required(p, "workspace_id")?;
    let w = spaces
        .iter()
        .find(|w| w["workspace_id"].as_str() == Some(&workspace))
        .ok_or("Focused workspace missing")?;
    if !boolean(w, "focused")? {
        return Err("Contradictory workspace focus".into());
    }
    let pane = required(p, "pane_id")?;
    let label = optional(p, "label")?
        .or(optional(p, "terminal_title_stripped")?)
        .unwrap_or_else(|| pane.clone());
    let foreground = optional(p, "foreground_cwd")?;
    let cwd = optional(p, "cwd")?;
    Ok(Some(Context {
        workspace,
        tab: required(p, "tab_id")?,
        pane,
        pane_label: label,
        workspace_label: required(w, "label")?,
        cwd: foreground
            .filter(|s| Path::new(s).is_absolute())
            .or(cwd.filter(|s| Path::new(s).is_absolute())),
    }))
}
fn array<'a>(v: &'a Value, key: &str) -> Result<&'a Vec<Value>, String> {
    v[key]
        .as_array()
        .ok_or_else(|| format!("Invalid {key} array"))
}
fn required(v: &Value, key: &str) -> Result<String, String> {
    v[key]
        .as_str()
        .filter(|s| !s.is_empty())
        .map(String::from)
        .ok_or_else(|| format!("Invalid {key}"))
}
fn optional(v: &Value, key: &str) -> Result<Option<String>, String> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok((!s.is_empty()).then(|| s.clone())),
        _ => Err(format!("Invalid {key}")),
    }
}
fn boolean(v: &Value, key: &str) -> Result<bool, String> {
    v[key].as_bool().ok_or_else(|| format!("Invalid {key}"))
}
fn json(r: &impl Runner, args: &[&str]) -> Result<Value, String> {
    serde_json::from_str(&r.run(args)?).map_err(|e| e.to_string())
}
#[cfg(test)]
#[path = "menu_worker_tests.rs"]
mod tests;

pub fn repository(r: &impl Runner, workspace: &str) -> Result<Repository, String> {
    let v = json(r, &["herdr", "worktree", "list", "--workspace", workspace])?;
    let source = &v["result"]["source"];
    let root = required(source, "repo_root")?;
    let checkout = required(source, "source_checkout_path")?;
    if !Path::new(&root).is_absolute() || !Path::new(&checkout).is_absolute() {
        return Err("Repository paths must be absolute".into());
    }
    let mut entries = vec![];
    for e in array(&v["result"], "worktrees")? {
        let path = required(e, "path")?;
        if !Path::new(&path).is_absolute() {
            return Err("Worktree path must be absolute".into());
        }
        entries.push(Worktree {
            path,
            branch: optional(e, "branch")?,
            workspace: optional(e, "open_workspace_id")?,
            primary: !boolean(e, "is_linked_worktree")?,
            bare: boolean(e, "is_bare")?,
        });
    }
    let base = r
        .run(&[
            "git",
            "-C",
            &checkout,
            "rev-parse",
            "--verify",
            "HEAD^{commit}",
        ])?
        .trim()
        .to_string();
    if base.len() != 40 && base.len() != 64 || !base.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid source commit".into());
    }
    Ok(Repository {
        repo_key: required(source, "repo_key")?,
        name: required(source, "repo_name")?,
        root,
        source_checkout: checkout,
        base_commit: base,
        entries,
    })
}
fn same_path(a: &str, b: &str) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}
#[derive(Debug)]
pub struct Resources {
    pub workspace: Option<String>,
    pub pane: Option<String>,
    pub worktree: Option<String>,
}
#[derive(Debug)]
pub struct Completion {
    pub id: RequestId,
    pub outcome: Outcome,
    pub workspace: Option<String>,
    pub pane: Option<String>,
    pub worktree: Option<String>,
    pub committed: Option<(SettingsSnapshot, crate::config::Config)>,
    pub conflict: Option<SettingsSnapshot>,
}
impl Completion {
    fn failed(id: RequestId, e: String) -> Self {
        Self {
            id,
            outcome: Outcome::Failed(e),
            workspace: None,
            pane: None,
            worktree: None,
            committed: None,
            conflict: None,
        }
    }
}
pub fn execute(
    r: &impl Runner,
    op: &Operation,
    interested: impl Fn() -> bool,
    config: &Path,
    state: &Path,
) -> Completion {
    execute_using(r, r, op, interested, config, state)
}
pub fn execute_using(
    r: &impl Runner,
    dispatch: &impl Runner,
    op: &Operation,
    interested: impl Fn() -> bool,
    config: &Path,
    state: &Path,
) -> Completion {
    if !interested() {
        return Completion::failed(op.id, "Cancelled before dispatch".into());
    }
    if let Command::SaveSettings { original, updated } = &op.command {
        return match crate::settings::save(config, state, original, updated) {
            Ok(saved) => Completion {
                id: op.id,
                outcome: Outcome::Success("Settings saved".into()),
                committed: Some(saved),
                workspace: None,
                pane: None,
                worktree: None,
                conflict: None,
            },
            Err(crate::settings::SaveError::Conflict(current)) => Completion {
                id: op.id,
                outcome: Outcome::Failed("Configuration changed externally".into()),
                committed: None,
                workspace: None,
                pane: None,
                worktree: None,
                conflict: Some(current),
            },
            Err(crate::settings::SaveError::Failed(e)) => Completion::failed(op.id, e),
        };
    }
    let args = match prepare(r, op) {
        Ok(a) => a,
        Err(e) => return Completion::failed(op.id, e),
    };
    // Repository and Git probes may have taken time; recapture focus after them.
    if op.expected_repository.is_some() {
        match context(r) {
            Ok(Some(fresh)) if op.context.as_ref().is_some_and(|c| c.same_identity(&fresh)) => {}
            _ => {
                return Completion::failed(
                    op.id,
                    "Cancelled: focused context changed before dispatch".into(),
                )
            }
        }
    }
    // The final check occurs after potentially slow context/repository reads.
    if !interested() {
        return Completion::failed(op.id, "Cancelled before dispatch".into());
    }
    let refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    let result = dispatch.run(&refs);
    match result {
        Err(e) => {
            let refusal = serde_json::from_str::<Value>(&e).ok().is_some_and(|v| {
                matches!(
                    v["error"]["code"].as_str(),
                    Some(
                        "dirty_worktree_requires_force"
                            | "workspace_group_close_required"
                            | "pane_not_found"
                            | "workspace_not_found"
                            | "worktree_not_found"
                    )
                )
            });
            Completion {
                id: op.id,
                outcome: if refusal {
                    Outcome::Failed(e)
                } else {
                    Outcome::Unknown(e)
                },
                workspace: None,
                pane: None,
                worktree: None,
                committed: None,
                conflict: None,
            }
        }
        Ok(s) => match success(op, &s) {
            Ok(resources) => Completion {
                id: op.id,
                outcome: Outcome::Success(format!(
                    "Operation completed{}{}{}",
                    resources
                        .workspace
                        .as_ref()
                        .map(|s| format!("; workspace {s}"))
                        .unwrap_or_default(),
                    resources
                        .pane
                        .as_ref()
                        .map(|s| format!("; pane {s}"))
                        .unwrap_or_default(),
                    resources
                        .worktree
                        .as_ref()
                        .map(|s| format!("; checkout {s}"))
                        .unwrap_or_default()
                )),
                workspace: resources.workspace,
                pane: resources.pane,
                worktree: resources.worktree,
                committed: None,
                conflict: None,
            },
            Err(e) => Completion {
                id: op.id,
                outcome: Outcome::Unknown(e),
                workspace: None,
                pane: None,
                worktree: None,
                committed: None,
                conflict: None,
            },
        },
    }
}
fn prepare(r: &impl Runner, op: &Operation) -> Result<Vec<String>, String> {
    let captured = op.context.as_ref().ok_or("No captured context")?;
    let fresh = context(r)?.ok_or("Cancelled: focus disappeared")?;
    if !captured.same_identity(&fresh) {
        return Err("Cancelled: focused context changed".into());
    }
    if matches!(
        op.command,
        Command::CreateWorktree { .. }
            | Command::OpenWorktree { .. }
            | Command::RemoveWorktree { .. }
    ) {
        let repo = repository(r, &captured.workspace)?;
        if Some(&repo.repo_key) != op.expected_repository.as_ref() {
            return Err("Repository changed".into());
        }
        if !op
            .expected_repository_root
            .as_ref()
            .is_some_and(|root| same_path(root, &repo.root))
        {
            return Err("Repository root changed".into());
        }
        match &op.command {
            Command::CreateWorktree {
                branch,
                base,
                repository,
            } => {
                validate(Field::Branch, branch)?;
                if !same_path(&repo.root, repository) {
                    return Err("Repository root changed".into());
                }
                r.run(&[
                    "git",
                    "-C",
                    &repo.source_checkout,
                    "check-ref-format",
                    &format!("refs/heads/{branch}"),
                ])?;
                let branches = r.run(&[
                    "git",
                    "-C",
                    &repo.source_checkout,
                    "for-each-ref",
                    "--format=%(refname)",
                    "refs/heads/",
                ])?;
                if branches
                    .lines()
                    .any(|line| line == format!("refs/heads/{branch}"))
                {
                    return Err("Branch already exists".into());
                }
                if (base.len() != 40 && base.len() != 64)
                    || !base.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    return Err("Invalid pinned commit".into());
                }
                let actual = r.run(&[
                    "git",
                    "-C",
                    &repo.source_checkout,
                    "rev-parse",
                    "--verify",
                    &format!("{base}^{{commit}}"),
                ])?;
                if actual.trim() != base {
                    return Err("Pinned commit unavailable".into());
                }
            }
            Command::OpenWorktree { path, workspace: _ }
            | Command::RemoveWorktree { path, workspace: _ } => {
                let entry = repo
                    .entries
                    .iter()
                    .find(|e| same_path(&e.path, path))
                    .ok_or("Worktree disappeared")?;
                let expected = match &op.command {
                    Command::OpenWorktree { workspace, .. } => workspace.as_ref(),
                    Command::RemoveWorktree { workspace, .. } => Some(workspace),
                    _ => unreachable!(),
                };
                if entry.workspace.as_ref() != expected || entry.bare {
                    return Err("Worktree association changed".into());
                }
                if matches!(op.command, Command::RemoveWorktree { .. }) && entry.primary {
                    return Err("Primary checkout cannot be removed".into());
                }
            }
            _ => {}
        }
    }
    argv(op)
}
pub fn argv(op: &Operation) -> Result<Vec<String>, String> {
    let c = op.context.as_ref().ok_or("Missing captured context")?;
    let s = |args: &[&str]| args.iter().map(|s| s.to_string()).collect();
    Ok(match &op.command {
        Command::CreateSpace { name, cwd } => {
            validate(Field::Name, name)?;
            if !Path::new(cwd).is_absolute() {
                return Err("Invalid directory".into());
            }
            s(&[
                "herdr",
                "workspace",
                "create",
                "--cwd",
                cwd,
                "--label",
                name,
                "--no-focus",
            ])
        }
        Command::Split { direction, cwd } => {
            if !Path::new(cwd).is_absolute() {
                return Err("Invalid directory".into());
            }
            s(&[
                "herdr",
                "pane",
                "split",
                "--pane",
                &c.pane,
                "--direction",
                if *direction == SplitDirection::Right {
                    "right"
                } else {
                    "down"
                },
                "--cwd",
                cwd,
                "--focus",
            ])
        }
        Command::ToggleZoom => s(&["herdr", "pane", "zoom", "--pane", &c.pane, "--toggle"]),
        Command::RenamePane { label } => {
            if let Some(label) = label {
                validate(Field::Label, label)?;
                if label == "--clear" {
                    return Err("Herdr reserves --clear; choose another pane label".into());
                }
            }
            s(&[
                "herdr",
                "pane",
                "rename",
                &c.pane,
                label.as_deref().unwrap_or("--clear"),
            ])
        }
        Command::ClosePane => s(&["herdr", "pane", "close", &c.pane]),
        Command::CloseWorkspace => s(&["herdr", "workspace", "close", &c.workspace]),
        Command::CreateWorktree { branch, base, .. } => s(&[
            "herdr",
            "worktree",
            "create",
            "--workspace",
            &c.workspace,
            "--branch",
            branch,
            "--base",
            base,
            "--no-focus",
        ]),
        Command::OpenWorktree {
            workspace: Some(workspace),
            ..
        } => s(&["herdr", "workspace", "focus", workspace]),
        Command::OpenWorktree { path, .. } => s(&[
            "herdr",
            "worktree",
            "open",
            "--workspace",
            &c.workspace,
            "--path",
            path,
            "--no-focus",
        ]),
        Command::RemoveWorktree { workspace, .. } => {
            s(&["herdr", "worktree", "remove", "--workspace", workspace])
        }
        Command::SaveSettings { .. } => return Err("Settings use atomic persistence".into()),
    })
}
fn success(op: &Operation, text: &str) -> Result<Resources, String> {
    let v: Value = serde_json::from_str(text).map_err(|_| "Malformed success response")?;
    let result = &v["result"];
    let kind = required(result, "type")?;
    let captured = op.context.as_ref().ok_or("Missing captured context")?;
    let mut resources = Resources {
        workspace: None,
        pane: None,
        worktree: None,
    };
    let equal = |actual: String, expected: &str| {
        if actual == expected {
            Ok(())
        } else {
            Err("Response target differs from captured target".to_string())
        }
    };
    match &op.command {
        Command::CreateSpace { name, .. } if kind == "workspace_created" => {
            let workspace = required(&result["workspace"], "workspace_id")?;
            equal(required(&result["workspace"], "label")?, name)?;
            equal(required(&result["root_pane"], "workspace_id")?, &workspace)?;
            resources.pane = Some(required(&result["root_pane"], "pane_id")?);
            resources.workspace = Some(workspace);
        }
        Command::CreateWorktree { .. } if kind == "worktree_created" => {
            let workspace = required(&result["workspace"], "workspace_id")?;
            let path = worktree_response(result, op, &workspace)?;
            if !boolean(&result["worktree"], "is_linked_worktree")? {
                return Err("Created checkout is not linked".into());
            }
            equal(required(&result["root_pane"], "workspace_id")?, &workspace)?;
            resources.pane = Some(required(&result["root_pane"], "pane_id")?);
            resources.workspace = Some(workspace);
            resources.worktree = Some(path);
        }
        Command::OpenWorktree {
            path,
            workspace: None,
        } if kind == "worktree_opened" => {
            let workspace = required(&result["workspace"], "workspace_id")?;
            boolean(result, "already_open")?;
            let actual = worktree_response(result, op, &workspace)?;
            if !same_path(&actual, path) {
                return Err("Opened a different worktree".into());
            }
            resources.workspace = Some(workspace);
            resources.worktree = Some(actual);
        }
        Command::OpenWorktree {
            workspace: Some(workspace),
            ..
        } if kind == "workspace_info" => {
            equal(required(&result["workspace"], "workspace_id")?, workspace)?;
            if !boolean(&result["workspace"], "focused")? {
                return Err("Workspace was not focused".into());
            }
        }
        Command::RemoveWorktree { path, workspace } if kind == "worktree_removed" => {
            equal(required(result, "workspace_id")?, workspace)?;
            if boolean(result, "forced")? {
                return Err("Unexpected forced removal".into());
            }
            let actual = required(result, "path")?;
            if !same_path(&actual, path) {
                return Err("Removed a different worktree".into());
            }
            resources.worktree = Some(actual);
        }
        Command::Split { .. } if kind == "pane_info" => {
            equal(
                required(&result["pane"], "workspace_id")?,
                &captured.workspace,
            )?;
            equal(required(&result["pane"], "tab_id")?, &captured.tab)?;
            let pane = required(&result["pane"], "pane_id")?;
            if pane == captured.pane {
                return Err("Split returned original pane".into());
            }
            resources.pane = Some(pane);
        }
        Command::RenamePane { label } if kind == "pane_info" => {
            equal(required(&result["pane"], "pane_id")?, &captured.pane)?;
            equal(
                required(&result["pane"], "workspace_id")?,
                &captured.workspace,
            )?;
            if optional(&result["pane"], "label")?.as_ref() != label.as_ref() {
                return Err("Pane label does not match requested value".into());
            }
            resources.pane = Some(captured.pane.clone());
        }
        Command::ToggleZoom if kind == "pane_zoom" => {
            equal(required(&result["zoom"], "pane_id")?, &captured.pane)?;
            boolean(&result["zoom"], "zoomed")?;
            resources.pane = Some(captured.pane.clone());
        }
        Command::ClosePane | Command::CloseWorkspace if kind == "ok" => {}
        _ => return Err("Unexpected success response type".into()),
    }
    if resources
        .worktree
        .as_ref()
        .is_some_and(|p| !Path::new(p).is_absolute())
    {
        return Err("Returned worktree path is not absolute".into());
    }
    Ok(resources)
}
fn worktree_response(result: &Value, op: &Operation, workspace: &str) -> Result<String, String> {
    let entry = &result["worktree"];
    let metadata = &result["workspace"]["worktree"];
    let path = required(entry, "path")?;
    if required(entry, "open_workspace_id")? != workspace {
        return Err("Returned worktree association differs".into());
    }
    if Some(required(metadata, "repo_key")?.as_str()) != op.expected_repository.as_deref() {
        return Err("Returned repository identity differs".into());
    }
    if !same_path(&path, &required(metadata, "checkout_path")?) {
        return Err("Returned worktree paths disagree".into());
    }
    if boolean(entry, "is_bare")?
        || boolean(entry, "is_linked_worktree")? != boolean(metadata, "is_linked_worktree")?
    {
        return Err("Returned worktree kind differs".into());
    }
    Ok(path)
}
/// Only observed, exact target absence can settle a lost close response. Creation,
/// focus and toggle cannot be attributed from later state and remain blocked.
pub fn reconcile(r: &impl Runner, op: &Operation) -> Outcome {
    let Some(c) = op.context.as_ref() else {
        return Outcome::Unknown("No safe reconciliation for this operation".into());
    };
    let target = match &op.command {
        Command::ClosePane => Some(("pane", "panes", "pane_id", &c.pane)),
        Command::CloseWorkspace => Some(("workspace", "workspaces", "workspace_id", &c.workspace)),
        _ => None,
    };
    let Some((kind, array_key, id_key, id)) = target else {
        return Outcome::Unknown(
            "Read-only reconciliation is inconclusive; operation remains blocked".into(),
        );
    };
    let result = (|| {
        let v = json(r, &["herdr", kind, "list"])?;
        let entries = array(&v["result"], array_key)?;
        let mut found = false;
        for entry in entries {
            found |= required(entry, id_key)? == *id;
        }
        Ok::<_, String>(found)
    })();
    match result {
        Ok(false) => Outcome::Success(format!("Confirmed {kind} {id} is closed")),
        Ok(true) => Outcome::Unknown(format!(
            "{kind} {id} still exists; original operation outcome remains unknown"
        )),
        Err(e) => Outcome::Unknown(format!("Reconciliation unavailable: {e}")),
    }
}
