use super::*;
use serde_json::json;
fn panes() -> Value {
    json!({"result":{"panes":[{"pane_id":"p","workspace_id":"w","tab_id":"t","focused":true,"foreground_cwd":"/foreground","cwd":"/cwd"}]}})
}
fn spaces() -> Value {
    json!({"result":{"workspaces":[{"workspace_id":"w","label":"Space","focused":true}]}})
}
#[test]
fn context_uses_focused_identity_and_absolute_foreground_directory() {
    let c = parse_context(&panes(), &spaces()).unwrap().unwrap();
    assert_eq!(c.pane, "p");
    assert_eq!(c.cwd.as_deref(), Some("/foreground"));
    assert_eq!(c.pane_label, "p");
    assert_eq!(c.workspace_label, "Space");
    let mut p = panes();
    p["result"]["panes"][0]["foreground_cwd"] = json!("relative");
    assert_eq!(
        parse_context(&p, &spaces())
            .unwrap()
            .unwrap()
            .cwd
            .as_deref(),
        Some("/cwd")
    );
    p["result"]["panes"][0]["cwd"] = json!("");
    assert_eq!(parse_context(&p, &spaces()).unwrap().unwrap().cwd, None);
}
#[test]
fn malformed_context_is_not_no_focus() {
    assert!(parse_context(&json!({}), &spaces()).is_err());
    for key in ["pane_id", "workspace_id", "tab_id", "focused"] {
        let mut p = panes();
        p["result"]["panes"][0][key] = json!(null);
        assert!(parse_context(&p, &spaces()).is_err(), "{key}");
    }
    let mut p = panes();
    p["result"]["panes"][0]["focused"] = json!(false);
    assert_eq!(parse_context(&p, &spaces()).unwrap(), None);
}
#[test]
fn unrelated_success_response_cannot_release_operation_slot() {
    assert!(success(
        &op(Command::ClosePane),
        r#"{"result":{"type":"workspace_list"}}"#
    )
    .is_err());
}
#[test]
fn rename_clear_and_literal_clear_are_distinct() {
    let context = parse_context(&panes(), &spaces()).unwrap();
    let mut op = Operation {
        id: RequestId {
            session: 1,
            serial: 1,
        },
        context,
        expected_repository: None,
        expected_repository_root: None,
        command: Command::RenamePane { label: None },
    };
    let clear = argv(&op).unwrap();
    op.command = Command::RenamePane {
        label: Some("--clear".into()),
    };
    assert!(argv(&op).is_err());
    assert!(clear.contains(&"--clear".into()));
}
struct Script {
    calls: std::cell::RefCell<Vec<Vec<String>>>,
    replies: std::cell::RefCell<std::collections::VecDeque<Result<String, String>>>,
}
impl Script {
    fn new(replies: Vec<Result<String, String>>) -> Self {
        Self {
            calls: Default::default(),
            replies: std::cell::RefCell::new(replies.into()),
        }
    }
}
impl Runner for Script {
    fn run(&self, args: &[&str]) -> Result<String, String> {
        self.calls
            .borrow_mut()
            .push(args.iter().map(|s| s.to_string()).collect());
        self.replies
            .borrow_mut()
            .pop_front()
            .expect("unexpected command")
    }
}
fn op(command: Command) -> Operation {
    Operation {
        id: RequestId {
            session: 1,
            serial: 1,
        },
        expected_repository: None,
        expected_repository_root: None,
        context: parse_context(&panes(), &spaces()).unwrap(),
        command,
    }
}
#[test]
fn exact_close_dispatch_and_changed_focus_definitive_cancel() {
    let r = Script::new(vec![
        Ok(spaces().to_string()),
        Ok(panes().to_string()),
        Ok(r#"{"result":{"type":"ok"}}"#.into()),
    ]);
    let done = execute(
        &r,
        &op(Command::ClosePane),
        || true,
        Path::new("/unused"),
        Path::new("/unused"),
    );
    assert!(matches!(done.outcome, Outcome::Success(_)));
    assert_eq!(r.calls.borrow()[2], vec!["herdr", "pane", "close", "p"]);
    let mut p = panes();
    p["result"]["panes"][0]["pane_id"] = json!("other");
    let r = Script::new(vec![Ok(spaces().to_string()), Ok(p.to_string())]);
    let done = execute(
        &r,
        &op(Command::ClosePane),
        || true,
        Path::new("/unused"),
        Path::new("/unused"),
    );
    assert!(matches!(done.outcome,Outcome::Failed(ref e) if e.contains("Cancelled")));
    assert_eq!(r.calls.borrow().len(), 2);
}
#[test]
fn dispatch_timeout_is_unknown_but_structured_server_refusal_is_failed() {
    for (error, unknown) in [
        ("timed out", true),
        (
            r#"{"error":{"code":"dirty_worktree_requires_force","message":"dirty"}}"#,
            false,
        ),
    ] {
        let r = Script::new(vec![
            Ok(spaces().to_string()),
            Ok(panes().to_string()),
            Err(error.into()),
        ]);
        let done = execute(
            &r,
            &op(Command::ClosePane),
            || true,
            Path::new("/unused"),
            Path::new("/unused"),
        );
        assert_eq!(matches!(done.outcome, Outcome::Unknown(_)), unknown);
        assert_eq!(r.calls.borrow().len(), 3);
    }
}
#[test]
fn cancellation_after_revalidation_never_dispatches() {
    let r = Script::new(vec![Ok(spaces().to_string()), Ok(panes().to_string())]);
    let count = std::cell::Cell::new(0);
    let done = execute(
        &r,
        &op(Command::ClosePane),
        || {
            let n = count.get();
            count.set(n + 1);
            n == 0
        },
        Path::new("/unused"),
        Path::new("/unused"),
    );
    assert!(matches!(done.outcome,Outcome::Failed(ref e) if e.contains("Cancelled")));
    assert_eq!(r.calls.borrow().len(), 2);
}
#[test]
fn reconciliation_requires_definitive_absence() {
    let r = Script::new(vec![Ok(json!({"result":{"panes":[]}}).to_string())]);
    assert!(matches!(
        reconcile(&r, &op(Command::ClosePane)),
        Outcome::Success(_)
    ));
    let r = Script::new(vec![Ok(panes().to_string())]);
    assert!(matches!(
        reconcile(&r, &op(Command::ClosePane)),
        Outcome::Unknown(_)
    ));
    let r = Script::new(vec![Ok(json!({}).to_string())]);
    assert!(matches!(
        reconcile(&r, &op(Command::ClosePane)),
        Outcome::Unknown(_)
    ));
    let r = Script::new(vec![]);
    assert!(matches!(
        reconcile(&r, &op(Command::ToggleZoom)),
        Outcome::Unknown(_)
    ));
}
fn repo_json() -> Value {
    json!({"result":{"source":{"repo_key":"key","repo_name":"repo","repo_root":"/repo","source_checkout_path":"/repo"},"worktrees":[{"path":"/repo","branch":"main","open_workspace_id":"w","is_linked_worktree":false,"is_bare":false},{"path":"/linked","branch":"feature","open_workspace_id":"linked","is_linked_worktree":true,"is_bare":false}]}})
}
const HASH: &str = "0123456789012345678901234567890123456789";
#[test]
fn repository_pins_commit_and_rejects_malformed_worktree_data() {
    let r = Script::new(vec![Ok(repo_json().to_string()), Ok(HASH.into())]);
    let repo = repository(&r, "w").unwrap();
    assert_eq!(repo.base_commit, HASH);
    assert!(repo.entries[0].primary);
    assert!(!repo.entries[1].primary);
    assert_eq!(
        r.calls.borrow()[0],
        vec!["herdr", "worktree", "list", "--workspace", "w"]
    );
    for key in ["path", "is_bare", "is_linked_worktree"] {
        let mut v = repo_json();
        v["result"]["worktrees"][1][key] = json!(null);
        let r = Script::new(vec![Ok(v.to_string())]);
        assert!(repository(&r, "w").is_err());
    }
}
#[test]
fn worktree_remove_revalidates_exact_association_and_never_uses_force() {
    let mut operation = op(Command::RemoveWorktree {
        path: "/linked".into(),
        workspace: "linked".into(),
    });
    operation.expected_repository = Some("key".into());
    operation.expected_repository_root = Some("/repo".into());
    let r = Script::new(vec![
        Ok(spaces().to_string()),
        Ok(panes().to_string()),
        Ok(repo_json().to_string()),
        Ok(HASH.into()),
    ]);
    assert_eq!(
        prepare(&r, &operation).unwrap(),
        vec!["herdr", "worktree", "remove", "--workspace", "linked"]
    );
    for changed in ["association", "key", "primary", "bare", "missing"] {
        let mut v = repo_json();
        match changed {
            "association" => v["result"]["worktrees"][1]["open_workspace_id"] = json!("moved"),
            "key" => v["result"]["source"]["repo_key"] = json!("moved"),
            "primary" => v["result"]["worktrees"][1]["is_linked_worktree"] = json!(false),
            "bare" => v["result"]["worktrees"][1]["is_bare"] = json!(true),
            _ => v["result"]["worktrees"][1]["path"] = json!("/moved"),
        }
        let r = Script::new(vec![
            Ok(spaces().to_string()),
            Ok(panes().to_string()),
            Ok(v.to_string()),
            Ok(HASH.into()),
        ]);
        assert!(prepare(&r, &operation).is_err(), "{changed}");
        assert_eq!(r.calls.borrow().len(), 4);
    }
}
#[test]
fn create_worktree_validates_literal_ref_and_pinned_commit_without_shell() {
    let mut operation = op(Command::CreateWorktree {
        branch: "feature/a".into(),
        base: HASH.into(),
        repository: "/repo".into(),
    });
    operation.expected_repository = Some("key".into());
    operation.expected_repository_root = Some("/repo".into());
    let r = Script::new(vec![
        Ok(spaces().to_string()),
        Ok(panes().to_string()),
        Ok(repo_json().to_string()),
        Ok(HASH.into()),
        Ok("".into()),
        Ok("refs/heads/main\n".into()),
        Ok(HASH.into()),
    ]);
    let args = prepare(&r, &operation).unwrap();
    assert_eq!(
        args,
        vec![
            "herdr",
            "worktree",
            "create",
            "--workspace",
            "w",
            "--branch",
            "feature/a",
            "--base",
            HASH,
            "--no-focus"
        ]
    );
    assert_eq!(
        r.calls.borrow()[4],
        vec![
            "git",
            "-C",
            "/repo",
            "check-ref-format",
            "refs/heads/feature/a"
        ]
    );
}
#[test]
fn internal_error_is_not_proof_of_no_side_effect() {
    let r = Script::new(vec![
        Ok(spaces().to_string()),
        Ok(panes().to_string()),
        Err(r#"{"error":{"code":"internal_error","message":"after dispatch"}}"#.into()),
    ]);
    assert!(matches!(
        execute(
            &r,
            &op(Command::ClosePane),
            || true,
            Path::new("/unused"),
            Path::new("/unused")
        )
        .outcome,
        Outcome::Unknown(_)
    ));
}
#[test]
fn contradictory_workspace_focus_is_refused() {
    let mut w = spaces();
    w["result"]["workspaces"][0]["focused"] = json!(false);
    assert!(parse_context(&panes(), &w).is_err());
}
#[test]
fn focus_change_during_repository_validation_cancels_before_dispatch() {
    let mut operation = op(Command::RemoveWorktree {
        path: "/linked".into(),
        workspace: "linked".into(),
    });
    operation.expected_repository = Some("key".into());
    operation.expected_repository_root = Some("/repo".into());
    let mut moved = panes();
    moved["result"]["panes"][0]["pane_id"] = json!("changed");
    let r = Script::new(vec![
        Ok(spaces().to_string()),
        Ok(panes().to_string()),
        Ok(repo_json().to_string()),
        Ok(HASH.into()),
        Ok(spaces().to_string()),
        Ok(moved.to_string()),
    ]);
    let done = execute(
        &r,
        &operation,
        || true,
        Path::new("/unused"),
        Path::new("/unused"),
    );
    assert!(matches!(done.outcome,Outcome::Failed(ref message) if message.contains("Cancelled")));
    assert_eq!(r.calls.borrow().len(), 6);
}
#[test]
fn duplicate_identities_and_multiple_focused_workspaces_are_refused() {
    let mut p = panes();
    let duplicate = p["result"]["panes"][0].clone();
    p["result"]["panes"].as_array_mut().unwrap().push(duplicate);
    assert!(parse_context(&p, &spaces()).is_err());
    let mut w = spaces();
    let mut other = w["result"]["workspaces"][0].clone();
    other["workspace_id"] = json!("other");
    w["result"]["workspaces"]
        .as_array_mut()
        .unwrap()
        .push(other);
    assert!(parse_context(&panes(), &w).is_err());
}

/// Explicitly invoked by the coordinator against its disposable named server.
/// An ordinary test invocation cannot touch any Herdr session.
#[test]
#[ignore = "requires explicitly named disposable Herdr server and repository"]
fn live_isolated_menu_adapter_smoke() {
    struct NamedRunner {
        session: String,
    }
    impl Runner for NamedRunner {
        fn run(&self, args: &[&str]) -> Result<String, String> {
            let result = if args.first() == Some(&"herdr") {
                let mut scoped = vec!["herdr", "--session", self.session.as_str()];
                scoped.extend_from_slice(&args[1..]);
                crate::herdr::run_with_timeout(&scoped, std::time::Duration::from_secs(20))
            } else {
                crate::herdr::run_with_timeout(args, std::time::Duration::from_secs(20))
            };
            if let Ok(output) = &result {
                use std::io::Write;
                let mut file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open("/tmp/gamepad-live-adapter-responses.log")
                    .unwrap();
                writeln!(file, "ARGV {:?}\n{}", args, output).unwrap();
            }
            result
        }
    }
    let session =
        std::env::var("GAMEPAD_TEST_SESSION").expect("explicit disposable session required");
    assert!(session.starts_with("gamepad-actions-probe-"));
    let repo = std::env::var("GAMEPAD_TEST_REPO").expect("explicit disposable repository required");
    let canonical = std::fs::canonicalize(&repo).unwrap();
    assert!(canonical.starts_with("/private/tmp") || canonical.starts_with("/tmp"));
    assert!(canonical
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("gamepad-actions-probe"));
    let r = NamedRunner { session };
    let original = context(&r).unwrap().expect("fixture has focused pane");
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let state = std::env::temp_dir().join(format!(
        "gamepad-adapter-live-{}-{stamp}",
        std::process::id()
    ));
    std::fs::create_dir_all(&state).unwrap();
    struct Cleanup<'a> {
        runner: &'a NamedRunner,
        spaces: Vec<String>,
        original: String,
        state: std::path::PathBuf,
    }
    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            for id in self.spaces.iter().rev() {
                if let Err(e) = self.runner.run(&["herdr", "workspace", "close", id]) {
                    eprintln!("Scratch cleanup for {id}: {e}");
                }
            }
            let _ = self
                .runner
                .run(&["herdr", "workspace", "focus", &self.original]);
            let _ = std::fs::remove_dir_all(&self.state);
        }
    }
    let mut cleanup = Cleanup {
        runner: &r,
        spaces: vec![],
        original: original.workspace.clone(),
        state: state.clone(),
    };
    let serial = std::cell::Cell::new(0);
    let stable_context = || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        let mut previous = None;
        let mut stable = 0;
        loop {
            let current = context(&r).unwrap();
            if current == previous {
                stable += 1;
            } else {
                stable = 0;
                previous = current.clone();
            }
            if stable >= 10 {
                return current;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "scratch focus did not stabilize"
            );
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    };
    let perform = |command: Command, key: Option<String>| {
        serial.set(serial.get() + 1);
        let operation = Operation {
            id: RequestId {
                session: 700,
                serial: serial.get(),
            },
            context: stable_context(),
            expected_repository_root: key
                .as_ref()
                .map(|_| canonical.to_string_lossy().to_string()),
            expected_repository: key,
            command,
        };
        execute(&r, &operation, || true, &state.join("config.toml"), &state)
    };
    let success = |completion: &Completion| {
        assert!(
            matches!(completion.outcome, Outcome::Success(_)),
            "{completion:?}"
        )
    };
    let created = perform(
        Command::CreateSpace {
            name: format!("Gamepad adapter {stamp}"),
            cwd: repo.clone(),
        },
        None,
    );
    success(&created);
    let owned = created.workspace.unwrap();
    cleanup.spaces.push(owned.clone());
    r.run(&["herdr", "workspace", "focus", &owned]).unwrap();
    success(&perform(
        Command::RenamePane {
            label: Some("Adapter smoke".into()),
        },
        None,
    ));
    success(&perform(Command::RenamePane { label: None }, None));
    success(&perform(Command::ToggleZoom, None));
    success(&perform(Command::ToggleZoom, None));
    let split = perform(
        Command::Split {
            direction: SplitDirection::Right,
            cwd: repo.clone(),
        },
        None,
    );
    success(&split);
    assert!(split.pane.is_some());
    let repository = repository(&r, &owned).unwrap();
    assert!(!repository.base_commit.is_empty());
    let branch = format!("gamepad-adapter-{stamp}");
    let created = perform(
        Command::CreateWorktree {
            branch: branch.clone(),
            base: repository.base_commit.clone(),
            repository: repository.root.clone(),
        },
        Some(repository.repo_key.clone()),
    );
    success(&created);
    let workspace = created.workspace.unwrap();
    cleanup.spaces.push(workspace.clone());
    let checkout = created.worktree.unwrap();
    // Close the created association, then verify exact-path opening through the adapter.
    r.run(&["herdr", "workspace", "focus", &workspace]).unwrap();
    success(&perform(Command::CloseWorkspace, None));
    r.run(&["herdr", "workspace", "focus", &owned]).unwrap();
    let opened = perform(
        Command::OpenWorktree {
            path: checkout.clone(),
            workspace: None,
        },
        Some(repository.repo_key.clone()),
    );
    success(&opened);
    let associated = opened.workspace.unwrap();
    cleanup.spaces.push(associated.clone());
    // An already associated checkout must use the guarded workspace-focus command.
    success(&perform(
        Command::OpenWorktree {
            path: checkout.clone(),
            workspace: Some(associated.clone()),
        },
        Some(repository.repo_key.clone()),
    ));
    r.run(&["herdr", "workspace", "focus", &owned]).unwrap();
    let dirty = Path::new(&checkout).join("gamepad-adapter-dirty-fixture");
    std::fs::write(&dirty, "owned disposable fixture").unwrap();
    let refused = perform(
        Command::RemoveWorktree {
            path: checkout.clone(),
            workspace: associated.clone(),
        },
        Some(repository.repo_key.clone()),
    );
    assert!(
        matches!(refused.outcome,Outcome::Failed(ref e) if e.contains("dirty_worktree_requires_force")),
        "{refused:?}"
    );
    assert!(dirty.exists());
    std::fs::remove_file(&dirty).unwrap();
    success(&perform(
        Command::RemoveWorktree {
            path: checkout,
            workspace: associated,
        },
        Some(repository.repo_key.clone()),
    ));
    r.run(&["git", "-C", &repo, "branch", "-D", &branch])
        .unwrap();
    let closed = perform(Command::CloseWorkspace, None);
    assert!(
        matches!(closed.outcome, Outcome::Success(_))
            || matches!(closed.outcome,Outcome::Failed(ref e) if e.contains("workspace_group_close_required")),
        "{closed:?}"
    );
    if let Outcome::Failed(message) = closed.outcome {
        eprintln!("Disposable root workspace {owned} remains for server cleanup: {message}");
    }
    println!("Live adapter smoke passed: create/split/rename/clear/zoom/worktree create/open/remove/dirty refusal");
}
#[test]
fn every_success_response_preserves_resources_and_rejects_wrong_type_or_identity() {
    let worktree = json!({"path":"/linked","open_workspace_id":"new","is_linked_worktree":true,"is_bare":false});
    let metadata = json!({"repo_key":"key","checkout_path":"/linked","is_linked_worktree":true});
    let cases = vec![
        (
            Command::CreateSpace {
                name: "New".into(),
                cwd: "/repo".into(),
            },
            json!({"type":"workspace_created","workspace":{"workspace_id":"new","label":"New"},"root_pane":{"pane_id":"new:p","workspace_id":"new"}}),
            vec![
                "/workspace/workspace_id",
                "/workspace/label",
                "/root_pane/pane_id",
                "/root_pane/workspace_id",
            ],
            Some("new"),
            Some("new:p"),
            None,
        ),
        (
            Command::CreateWorktree {
                branch: "b".into(),
                base: HASH.into(),
                repository: "/repo".into(),
            },
            json!({"type":"worktree_created","workspace":{"workspace_id":"new","worktree":metadata},"worktree":worktree,"root_pane":{"pane_id":"new:p","workspace_id":"new"}}),
            vec![
                "/workspace/workspace_id",
                "/workspace/worktree/repo_key",
                "/worktree/path",
                "/worktree/is_linked_worktree",
            ],
            Some("new"),
            Some("new:p"),
            Some("/linked"),
        ),
        (
            Command::OpenWorktree {
                path: "/linked".into(),
                workspace: None,
            },
            json!({"type":"worktree_opened","workspace":{"workspace_id":"new","worktree":metadata},"worktree":worktree,"already_open":false}),
            vec![
                "/workspace/workspace_id",
                "/workspace/worktree/repo_key",
                "/worktree/path",
                "/already_open",
            ],
            Some("new"),
            None,
            Some("/linked"),
        ),
        (
            Command::OpenWorktree {
                path: "/linked".into(),
                workspace: Some("linked".into()),
            },
            json!({"type":"workspace_info","workspace":{"workspace_id":"linked","focused":true}}),
            vec!["/workspace/workspace_id", "/workspace/focused"],
            None,
            None,
            None,
        ),
        (
            Command::RemoveWorktree {
                path: "/linked".into(),
                workspace: "linked".into(),
            },
            json!({"type":"worktree_removed","workspace_id":"linked","path":"/linked","forced":false}),
            vec!["/workspace_id", "/path", "/forced"],
            None,
            None,
            Some("/linked"),
        ),
        (
            Command::Split {
                direction: SplitDirection::Down,
                cwd: "/repo".into(),
            },
            json!({"type":"pane_info","pane":{"pane_id":"new:p","workspace_id":"w","tab_id":"t"}}),
            vec!["/pane/pane_id", "/pane/workspace_id", "/pane/tab_id"],
            None,
            Some("new:p"),
            None,
        ),
        (
            Command::RenamePane {
                label: Some("hello".into()),
            },
            json!({"type":"pane_info","pane":{"pane_id":"p","workspace_id":"w","label":"hello"}}),
            vec!["/pane/pane_id", "/pane/workspace_id", "/pane/label"],
            None,
            Some("p"),
            None,
        ),
        (
            Command::RenamePane { label: None },
            json!({"type":"pane_info","pane":{"pane_id":"p","workspace_id":"w"}}),
            vec!["/pane/pane_id", "/pane/workspace_id"],
            None,
            Some("p"),
            None,
        ),
        (
            Command::ToggleZoom,
            json!({"type":"pane_zoom","zoom":{"pane_id":"p","zoomed":true}}),
            vec!["/zoom/pane_id", "/zoom/zoomed"],
            None,
            Some("p"),
            None,
        ),
        (
            Command::ClosePane,
            json!({"type":"ok"}),
            vec![],
            None,
            None,
            None,
        ),
        (
            Command::CloseWorkspace,
            json!({"type":"ok"}),
            vec![],
            None,
            None,
            None,
        ),
    ];
    for (command, result, fields, workspace, pane, path) in cases {
        let mut operation = op(command);
        operation.expected_repository = Some("key".into());
        operation.expected_repository_root = Some("/repo".into());
        let parsed = success(&operation, &json!({"result":result}).to_string()).unwrap();
        assert_eq!(parsed.workspace.as_deref(), workspace);
        assert_eq!(parsed.pane.as_deref(), pane);
        assert_eq!(parsed.worktree.as_deref(), path);
        let mut wrong = result.clone();
        wrong["type"] = json!("unrelated");
        assert!(
            success(&operation, &json!({"result":wrong}).to_string()).is_err(),
            "{:?}",
            operation.command
        );
        for field in fields {
            let mut wrong = result.clone();
            *wrong.pointer_mut(field).unwrap() = Value::Null;
            assert!(
                success(&operation, &json!({"result":wrong}).to_string()).is_err(),
                "{:?} {field}",
                operation.command
            );
        }
    }
}
#[test]
fn wrong_success_values_are_unknown_not_accepted() {
    let mut operation = op(Command::CreateWorktree {
        branch: "b".into(),
        base: HASH.into(),
        repository: "/repo".into(),
    });
    operation.expected_repository = Some("key".into());
    operation.expected_repository_root = Some("/repo".into());
    let good = json!({"type":"worktree_created","workspace":{"workspace_id":"new","worktree":{"repo_key":"key","checkout_path":"/linked","is_linked_worktree":true}},"worktree":{"path":"/linked","open_workspace_id":"new","is_linked_worktree":true,"is_bare":false},"root_pane":{"pane_id":"new:p","workspace_id":"new"}});
    assert!(success(&operation, &json!({"result":good}).to_string()).is_ok());
    for (field, value) in [
        ("/workspace/worktree/repo_key", json!("wrong")),
        ("/worktree/is_linked_worktree", json!(false)),
        ("/worktree/path", json!("relative")),
        ("/worktree/open_workspace_id", json!("other")),
        ("/worktree/is_bare", json!(true)),
        ("/workspace/worktree/checkout_path", json!("/other")),
    ] {
        let mut bad = good.clone();
        *bad.pointer_mut(field).unwrap() = value;
        assert!(
            success(&operation, &json!({"result":bad}).to_string()).is_err(),
            "{field}"
        );
    }
    let split = op(Command::Split {
        direction: SplitDirection::Right,
        cwd: "/repo".into(),
    });
    assert!(success(&split,&json!({"result":{"type":"pane_info","pane":{"pane_id":"p","workspace_id":"w","tab_id":"t"}}}).to_string()).is_err());
    let remove = op(Command::RemoveWorktree {
        path: "/linked".into(),
        workspace: "linked".into(),
    });
    for (path, forced) in [("/linked", true), ("/other", false)] {
        assert!(success(&remove,&json!({"result":{"type":"worktree_removed","workspace_id":"linked","forced":forced,"path":path}}).to_string()).is_err());
    }
}
#[test]
fn source_paths_commit_hashes_and_canonical_identity_are_validated() {
    for (root, checkout) in [("relative", "/repo"), ("/repo", "relative")] {
        let mut v = repo_json();
        v["result"]["source"]["repo_root"] = json!(root);
        v["result"]["source"]["source_checkout_path"] = json!(checkout);
        let r = Script::new(vec![Ok(v.to_string())]);
        assert!(repository(&r, "w").is_err());
    }
    for hash in ["bad".to_string(), "z".repeat(40), "z".repeat(64)] {
        let r = Script::new(vec![Ok(repo_json().to_string()), Ok(hash)]);
        assert!(repository(&r, "w").is_err());
    }
    let r = Script::new(vec![Ok(repo_json().to_string()), Ok("a".repeat(64))]);
    assert_eq!(repository(&r, "w").unwrap().base_commit.len(), 64);
    let dir = std::env::temp_dir().join(format!("gamepad-canonical-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("one")).unwrap();
    std::fs::create_dir_all(dir.join("two")).unwrap();
    let link = dir.join("link");
    std::os::unix::fs::symlink(dir.join("one"), &link).unwrap();
    assert!(same_path(
        link.to_str().unwrap(),
        dir.join("one").to_str().unwrap()
    ));
    assert!(!same_path(
        link.to_str().unwrap(),
        dir.join("two").to_str().unwrap()
    ));
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn validated_worktree_dispatch_uses_separate_runner_and_preserves_committed_outcome() {
    let mut operation = op(Command::RemoveWorktree {
        path: "/linked".into(),
        workspace: "linked".into(),
    });
    operation.expected_repository = Some("key".into());
    operation.expected_repository_root = Some("/repo".into());
    let validation = Script::new(vec![
        Ok(spaces().to_string()),
        Ok(panes().to_string()),
        Ok(repo_json().to_string()),
        Ok(HASH.into()),
        Ok(spaces().to_string()),
        Ok(panes().to_string()),
    ]);
    let dispatch=Script::new(vec![Ok(json!({"result":{"type":"worktree_removed","workspace_id":"linked","forced":false,"path":"/linked"}}).to_string())]);
    let done = execute_using(
        &validation,
        &dispatch,
        &operation,
        || true,
        Path::new("/unused"),
        Path::new("/unused"),
    );
    assert!(matches!(done.outcome, Outcome::Success(_)));
    assert_eq!(validation.calls.borrow().len(), 6);
    assert_eq!(
        dispatch.calls.borrow()[0],
        vec!["herdr", "worktree", "remove", "--workspace", "linked"]
    );
    assert_eq!(done.worktree.as_deref(), Some("/linked"));
}
#[test]
fn argv_covers_each_scoped_action_and_rejects_invalid_directories() {
    let cases = vec![
        (
            Command::CreateSpace {
                name: "--clear".into(),
                cwd: "/repo".into(),
            },
            vec![
                "herdr",
                "workspace",
                "create",
                "--cwd",
                "/repo",
                "--label",
                "--clear",
                "--no-focus",
            ],
        ),
        (
            Command::Split {
                direction: SplitDirection::Right,
                cwd: "/repo".into(),
            },
            vec![
                "herdr",
                "pane",
                "split",
                "--pane",
                "p",
                "--direction",
                "right",
                "--cwd",
                "/repo",
                "--focus",
            ],
        ),
        (
            Command::Split {
                direction: SplitDirection::Down,
                cwd: "/repo".into(),
            },
            vec![
                "herdr",
                "pane",
                "split",
                "--pane",
                "p",
                "--direction",
                "down",
                "--cwd",
                "/repo",
                "--focus",
            ],
        ),
        (
            Command::ToggleZoom,
            vec!["herdr", "pane", "zoom", "--pane", "p", "--toggle"],
        ),
        (
            Command::RenamePane {
                label: Some("name with spaces".into()),
            },
            vec!["herdr", "pane", "rename", "p", "name with spaces"],
        ),
        (
            Command::RenamePane { label: None },
            vec!["herdr", "pane", "rename", "p", "--clear"],
        ),
        (
            Command::CloseWorkspace,
            vec!["herdr", "workspace", "close", "w"],
        ),
        (
            Command::OpenWorktree {
                path: "/linked".into(),
                workspace: Some("linked".into()),
            },
            vec!["herdr", "workspace", "focus", "linked"],
        ),
        (
            Command::OpenWorktree {
                path: "/linked".into(),
                workspace: None,
            },
            vec![
                "herdr",
                "worktree",
                "open",
                "--workspace",
                "w",
                "--path",
                "/linked",
                "--no-focus",
            ],
        ),
    ];
    for (command, expected) in cases {
        assert_eq!(argv(&op(command)).unwrap(), expected);
    }
    for command in [
        Command::CreateSpace {
            name: "n".into(),
            cwd: "relative".into(),
        },
        Command::Split {
            direction: SplitDirection::Right,
            cwd: "".into(),
        },
    ] {
        assert!(argv(&op(command)).is_err());
    }
}
#[test]
fn open_worktree_refuses_changed_repository_root_even_when_key_stays_same() {
    let mut operation = op(Command::OpenWorktree {
        path: "/linked".into(),
        workspace: Some("linked".into()),
    });
    operation.expected_repository = Some("key".into());
    operation.expected_repository_root = Some("/original".into());
    let r = Script::new(vec![
        Ok(spaces().to_string()),
        Ok(panes().to_string()),
        Ok(repo_json().to_string()),
        Ok(HASH.into()),
    ]);
    assert!(prepare(&r, &operation).is_err());
}
#[test]
fn create_rejects_duplicate_branch_and_invalid_or_missing_pinned_commit() {
    for base in ["bad".to_string(), "z".repeat(40), "z".repeat(64)] {
        let mut operation = op(Command::CreateWorktree {
            branch: "new".into(),
            base,
            repository: "/repo".into(),
        });
        operation.expected_repository = Some("key".into());
        operation.expected_repository_root = Some("/repo".into());
        let r = Script::new(vec![
            Ok(spaces().to_string()),
            Ok(panes().to_string()),
            Ok(repo_json().to_string()),
            Ok(HASH.into()),
            Ok("".into()),
            Ok("".into()),
        ]);
        assert!(prepare(&r, &operation).is_err());
        assert_eq!(r.calls.borrow().len(), 6);
    }
    let mut operation = op(Command::CreateWorktree {
        branch: "new".into(),
        base: HASH.into(),
        repository: "/repo".into(),
    });
    operation.expected_repository = Some("key".into());
    operation.expected_repository_root = Some("/repo".into());
    for (branches, verify) in [
        ("refs/heads/new\n", None),
        ("refs/heads/main\n", Some("different")),
    ] {
        let mut replies = vec![
            Ok(spaces().to_string()),
            Ok(panes().to_string()),
            Ok(repo_json().to_string()),
            Ok(HASH.into()),
            Ok("".into()),
            Ok(branches.into()),
        ];
        if let Some(value) = verify {
            replies.push(Ok(value.into()));
        }
        let r = Script::new(replies);
        assert!(prepare(&r, &operation).is_err());
    }
    let hash = "a".repeat(64);
    if let Command::CreateWorktree { base, .. } = &mut operation.command {
        *base = hash.clone();
    }
    let r = Script::new(vec![
        Ok(spaces().to_string()),
        Ok(panes().to_string()),
        Ok(repo_json().to_string()),
        Ok(HASH.into()),
        Ok("".into()),
        Ok("".into()),
        Ok(hash.clone()),
    ]);
    assert!(prepare(&r, &operation).unwrap().contains(&hash));
}
#[test]
fn open_revalidates_associated_and_unassociated_paths_and_workspace_reconciliation() {
    for associated in [true, false] {
        let mut operation = op(Command::OpenWorktree {
            path: "/linked".into(),
            workspace: associated.then(|| "linked".into()),
        });
        operation.expected_repository = Some("key".into());
        operation.expected_repository_root = Some("/repo".into());
        let mut repo = repo_json();
        if !associated {
            repo["result"]["worktrees"][1]["open_workspace_id"] = Value::Null;
        }
        let r = Script::new(vec![
            Ok(spaces().to_string()),
            Ok(panes().to_string()),
            Ok(repo.to_string()),
            Ok(HASH.into()),
        ]);
        let args = prepare(&r, &operation).unwrap();
        assert_eq!(args[1], if associated { "workspace" } else { "worktree" });
    }
    let r = Script::new(vec![Ok(json!({"result":{"workspaces":[]}}).to_string())]);
    assert!(matches!(
        reconcile(&r, &op(Command::CloseWorkspace)),
        Outcome::Success(_)
    ));
    assert_eq!(r.calls.borrow()[0], vec!["herdr", "workspace", "list"]);
}
#[test]
fn verified_not_found_refusals_release_slot_while_unknown_server_codes_stay_blocked() {
    for code in [
        "pane_not_found",
        "workspace_not_found",
        "worktree_not_found",
        "workspace_group_close_required",
        "internal_error",
        "unexpected_new_code",
    ] {
        let operation = op(Command::ClosePane);
        let r = Script::new(vec![
            Ok(spaces().to_string()),
            Ok(panes().to_string()),
            Err(json!({"error":{"code":code,"message":"refused"}}).to_string()),
        ]);
        let done = execute(
            &r,
            &operation,
            || true,
            Path::new("/unused"),
            Path::new("/unused"),
        );
        let mut slot = OperationSlot::default();
        assert!(slot.reserve(operation.clone()));
        assert!(slot.complete(operation.id, done.outcome));
        let unknown = code == "internal_error" || code == "unexpected_new_code";
        assert_eq!(slot.blocked(), unknown, "{code}");
        let mut next = operation;
        next.id.serial = 2;
        assert_eq!(slot.reserve(next), !unknown, "{code}");
    }
}
#[test]
fn focused_pane_is_the_last_context_read_before_mutation() {
    struct Routing {
        calls: std::cell::RefCell<Vec<String>>,
    }
    impl Runner for Routing {
        fn run(&self, args: &[&str]) -> Result<String, String> {
            self.calls.borrow_mut().push(args[1].into());
            Ok(if args[1] == "pane" { panes() } else { spaces() }.to_string())
        }
    }
    let r = Routing {
        calls: Default::default(),
    };
    assert_eq!(context(&r).unwrap().unwrap().pane, "p");
    assert_eq!(*r.calls.borrow(), vec!["workspace", "pane"]);
}
