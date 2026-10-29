use crate::engine::Btn;
use crate::menu::*;
fn context() -> Context {
    Context {
        workspace: "ws-1".into(),
        tab: "tab-1".into(),
        pane: "pane-1".into(),
        workspace_label: "Project".into(),
        pane_label: "Shell".into(),
        cwd: Some("/tmp/project".into()),
    }
}
fn settings() -> SettingsSnapshot {
    SettingsSnapshot {
        labels: LabelStyle::Both,
        voice_button: Btn::RStick,
        max_seconds: 47,
        preview_before_send: false,
        model: "/models/base.bin".into(),
        language: "en".into(),
        config_path: "/config.toml".into(),
        version: "v1".into(),
    }
}
pub(crate) fn menu() -> Menu {
    Menu::new(
        7,
        Some(context()),
        settings(),
        "Space 123".into(),
        "gamepad/123".into(),
    )
}
fn press(m: &mut Menu, input: Input) -> Vec<Effect> {
    {
        if let Some(key) = input.action_key() {
            m.input(Input::Release(key), m.revision());
        }
        m.input(input, m.revision())
    }
}
fn enter_label(m: &mut Menu, label: &str) -> Vec<Effect> {
    let index = m.view().rows.iter().position(|r| r.label == label).expect("menu row");
    enter(m, index)
}
fn select(m: &mut Menu, index: usize) {
    for _ in 0..30 {
        press(m, Input::Up);
    }
    for _ in 0..index {
        press(m, Input::Down);
    }
}
fn enter(m: &mut Menu, index: usize) -> Vec<Effect> {
    select(m, index);
    press(m, Input::Activate)
}
fn read(e: Vec<Effect>) -> ReadRequest {
    match e.into_iter().next().unwrap() {
        Effect::Read(r) => r,
        other => panic!("{other:?}"),
    }
}
fn operation(e: Vec<Effect>) -> Operation {
    match e.into_iter().next().unwrap() {
        Effect::Mutate(r) => r,
        other => panic!("{other:?}"),
    }
}
pub(crate) fn repository() -> Repository {
    Repository {
        repo_key: "/tmp/project/.git".into(),
        name: "project".into(),
        root: "/tmp/project".into(),
        source_checkout: "/tmp/project".into(),
        base_commit: "abc123".into(),
        entries: vec![
            Worktree {
                path: "/tmp/project".into(),
                branch: Some("main".into()),
                workspace: Some("ws-1".into()),
                primary: true,
                bare: false,
            },
            Worktree {
                path: "/tmp/linked".into(),
                branch: Some("feature".into()),
                workspace: Some("ws-2".into()),
                primary: false,
                bare: false,
            },
            Worktree {
                path: "/tmp/closed".into(),
                branch: None,
                workspace: None,
                primary: false,
                bare: false,
            },
            Worktree {
                path: "/tmp/bare".into(),
                branch: None,
                workspace: Some("ws-3".into()),
                primary: false,
                bare: true,
            },
        ],
    }
}
fn load_repo(m: &mut Menu) {
    let r = read(enter(m, 1));
    assert!(m.receive(Response::Read {
        id: r.id,
        result: Ok(ReadData::Repository(repository()))
    }));
}
#[test]
fn root_navigation_and_scope() {
    let mut m = menu();
    assert_eq!(
        m.view()
            .rows
            .iter()
            .map(|r| r.label.as_str())
            .collect::<Vec<_>>(),
        vec![
            "New space",
            "Worktrees",
            "Pane actions",
            "Close…",
            "Settings",
            "Logs",
            "Controls",
            "Review voice draft"
        ]
    );
    select(&mut m, 99);
    assert_eq!(m.view().selected, 7);
    press(&mut m, Input::Up);
    assert_eq!(m.view().selected, 6);
    press(&mut m, Input::Right);
    assert_eq!(m.view().breadcrumb, "Main / Controls");
    press(&mut m, Input::Back);
    assert_eq!(m.view().selected, 6);
    press(&mut m, Input::Left);
    assert!(!m.is_open());
}
#[test]
fn optional_scope_and_changed_identity() {
    let mut m = Menu::new(1, None, settings(), "Space".into(), "branch".into());
    assert!(m.view().rows[0].disabled.is_some());
    enter(&mut m, 4);
    assert_eq!(m.view().scope, "Global");
    assert!(m.observe_context(Ok(Some(context()))));
    assert!(!m.is_open());
    let mut m = menu();
    let mut c = context();
    c.pane = "different".into();
    assert!(m.observe_context(Ok(Some(c))));
    assert!(!m.is_open());
    let mut m = menu();
    assert!(m.observe_context(Err("snapshot failed".into())));
    assert!(!m.is_open());
}
#[test]
fn ack_and_right_never_execute_leaf() {
    let mut m = menu();
    enter(&mut m, 2);
    let rev = m.revision();
    press(&mut m, Input::Down);
    assert!(m.revision() > rev);
    assert!(m.input(Input::Activate, rev).is_empty());
    assert_eq!(m.view().breadcrumb, "Main / Pane actions");
    assert!(press(&mut m, Input::Right).is_empty());
    assert!(m.pending().is_none());
    let op = operation(press(&mut m, Input::Activate));
    assert_eq!(
        op.command,
        Command::Split {
            direction: SplitDirection::Down,
            cwd: "/tmp/project".into()
        }
    );
    assert_eq!(op.context, Some(context()));
    assert!(press(&mut m, Input::Activate).is_empty());
}
#[test]
fn close_cancel_then_exact_once() {
    let mut m = menu();
    enter(&mut m, 3);
    enter(&mut m, 0);
    assert_eq!(m.view().selected, 0);
    assert!(m.view().details.join(" ").contains("pane-1"));
    assert!(press(&mut m, Input::Activate).is_empty());
    assert_eq!(m.view().breadcrumb, "Main / Close…");
    enter(&mut m, 1);
    press(&mut m, Input::Down);
    let op = operation(press(&mut m, Input::Activate));
    assert_eq!(op.command, Command::CloseWorkspace);
    assert!(press(&mut m, Input::Activate).is_empty());
    assert!(m.pending().is_some());
}
#[test]
fn reads_discard_after_back_and_wrong_identity() {
    let mut m = menu();
    let r = read(enter(&mut m, 1));
    assert!(m.view().status.contains("Loading"));
    press(&mut m, Input::Back);
    assert!(!m.receive(Response::Read {
        id: r.id,
        result: Ok(ReadData::Repository(repository()))
    }));
    assert_eq!(m.view().breadcrumb, "Main");
    let r = read(enter(&mut m, 1));
    let wrong = RequestId {
        session: 99,
        serial: r.id.serial,
    };
    assert!(!m.receive(Response::Read {
        id: wrong,
        result: Ok(ReadData::Repository(repository()))
    }));
    assert!(m.receive(Response::Read {
        id: r.id,
        result: Ok(ReadData::Repository(repository()))
    }));
    assert!(m.view().details.join(" ").contains("/tmp/project"));
}
#[test]
fn worktree_commands_use_paths_and_pinned_base() {
    let mut m = menu();
    load_repo(&mut m);
    let r = read(enter(&mut m, 0));
    assert!(m.receive(Response::Read {
        id: r.id,
        result: Ok(ReadData::Repository(repository()))
    }));
    let op = operation(enter(&mut m, 1));
    assert_eq!(
        op.expected_repository_root,
        Some(repository().root),
        "Create must carry the captured repository root"
    );
    assert_eq!(
        op.command,
        Command::CreateWorktree {
            branch: "gamepad/123".into(),
            base: "abc123".into(),
            repository: "/tmp/project".into()
        }
    );
    let mut m = menu();
    load_repo(&mut m);
    enter(&mut m, 1);
    let op = operation(enter(&mut m, 1));
    assert_eq!(
        op.expected_repository_root,
        Some(repository().root),
        "Open must carry the captured repository root"
    );
    assert_eq!(
        op.command,
        Command::OpenWorktree {
            path: "/tmp/linked".into(),
            workspace: Some("ws-2".into())
        }
    );
    let mut m = menu();
    load_repo(&mut m);
    enter(&mut m, 2);
    assert!(m.view().rows[0].disabled.is_some());
    assert!(m.view().rows[2].disabled.is_some());
    assert!(m.view().rows[3].disabled.is_some());
    enter(&mut m, 1);
    assert_eq!(m.view().selected, 0);
    assert!(m.view().details.join(" ").contains("/tmp/linked"));
    let op = operation(enter(&mut m, 1));
    assert_eq!(
        op.expected_repository_root,
        Some(repository().root),
        "Remove must carry the captured repository root"
    );
    assert_eq!(
        op.command,
        Command::RemoveWorktree {
            path: "/tmp/linked".into(),
            workspace: "ws-2".into()
        }
    );
}
#[test]
fn empty_repository_and_read_failure_are_actionable() {
    let mut m = menu();
    let r = read(enter(&mut m, 1));
    assert!(m.receive(Response::Read {
        id: r.id,
        result: Err("No repository".into())
    }));
    assert!(m.view().status.contains("No repository"));
    assert!(m.view().rows.iter().all(|r| r.disabled.is_some()));
    let mut m = menu();
    let r = read(enter(&mut m, 1));
    let mut repo = repository();
    repo.entries.clear();
    assert!(m.receive(Response::Read {
        id: r.id,
        result: Ok(ReadData::Repository(repo))
    }));
    enter(&mut m, 1);
    assert_eq!(m.view().rows[0].label, "No worktrees");
    assert!(m.view().rows[0].disabled.is_some());
}
#[test]
fn text_grid_edits_unicode_cancels_and_validates() {
    let mut e = TextEditor::new(Field::Label, "猫é".into());
    e.input(Input::Delete);
    assert_eq!(e.buffer, "猫");
    e.input(Input::Activate);
    assert_eq!(e.buffer, "猫a");
    e.input(Input::Page);
    e.input(Input::Activate);
    assert_eq!(e.buffer, "猫aA");
    e.input(Input::Page);
    assert!(e.cells().contains(&"/".to_string()));
    assert!(e.cells().contains(&"_".to_string()));
    assert!(e.cells().contains(&"-".to_string()));
    assert_eq!(e.input(Input::Back), EditResult::Cancelled);
    assert!(validate(Field::Name, "").is_err());
    assert!(validate(Field::Name, &"a".repeat(81)).is_err());
    assert!(validate(Field::Branch, &"a".repeat(201)).is_err());
    assert!(validate(Field::Label, "bad\x1b").is_err());
    assert!(validate(Field::Branch, "feature/good-name_123").is_ok());
}
#[test]
fn editing_forms_and_cwd_validation() {
    let mut m = menu();
    enter(&mut m, 0);
    enter(&mut m, 0);
    press(&mut m, Input::Delete);
    press(&mut m, Input::Back);
    assert!(m.view().rows[0].label.contains("Space 123"));
    let op = operation(enter(&mut m, 1));
    assert_eq!(
        op.command,
        Command::CreateSpace {
            name: "Space 123".into(),
            cwd: "/tmp/project".into()
        }
    );
    let mut c = context();
    c.cwd = Some("relative".into());
    let m = Menu::new(1, Some(c), settings(), "Space".into(), "b".into());
    assert!(m.view().rows[0].disabled.is_some());
}
#[test]
fn settings_preserve_custom_and_save_explicitly() {
    let mut m = menu();
    enter(&mut m, 4);
    assert!(m.view().rows[2].label.contains("47"));
    press(&mut m, Input::Activate);
    assert!(m.view().rows[0].label.contains("PlayStation"));
    enter(&mut m, 1);
    assert!(m.view().rows[1].label.contains("L3"));
    assert!(m.pending().is_none());
    let op = operation(enter_label(&mut m, "Save"));
    match op.command {
        Command::SaveSettings { original, updated } => {
            assert_eq!(original.version, "v1");
            assert_eq!(updated.labels, LabelStyle::PlayStation);
            assert_eq!(updated.max_seconds, 47);
        }
        other => panic!("{other:?}"),
    };
    let mut m = menu();
    enter(&mut m, 4);
    press(&mut m, Input::Activate);
    enter_label(&mut m, "Discard");
    enter(&mut m, 4);
    assert!(m.view().rows[0].label.contains("Both"));
}
#[test]
fn logs_refresh_and_controls_scroll() {
    let mut m = menu();
    let r = read(enter(&mut m, 5));
    assert_eq!(r.kind, ReadKind::Logs);
    assert!(m.receive(Response::Read {
        id: r.id,
        result: Ok(ReadData::Logs(vec![
            "one".into(),
            "two".into(),
            "three".into()
        ]))
    }));
    press(&mut m, Input::Down);
    assert_eq!(m.view().scroll, 1);
    let r = read(press(&mut m, Input::Activate));
    assert_eq!(r.kind, ReadKind::Logs);
    press(&mut m, Input::Back);
    enter(&mut m, 6);
    press(&mut m, Input::Down);
    assert_eq!(m.view().scroll, 1);
    assert!(m.view().details.join(" ").contains("R3"));
}
#[test]
fn global_slot_retains_unknown_and_never_retries() {
    let mut m = menu();
    enter(&mut m, 2);
    let op = operation(press(&mut m, Input::Activate));
    let mut slot = OperationSlot::default();
    assert!(slot.reserve(op.clone()));
    assert!(!slot.reserve(op.clone()));
    assert!(slot.complete(op.id, Outcome::Unknown("timed out".into())));
    assert_eq!(
        slot.state(),
        Some(&OperationState::Unknown("timed out".into()))
    );
    assert!(!slot.reserve(op.clone()));
    press(&mut m, Input::Menu);
    assert!(!m.is_open());
    assert!(slot.reconcile(op.id, Outcome::Success("created".into())));
    assert_eq!(
        slot.state(),
        Some(&OperationState::Finished(Outcome::Success(
            "created".into()
        )))
    );
    assert!(!slot.reserve(op));
}
#[test]
fn mutation_result_requires_exact_id_and_unknown_stays_blocked() {
    let mut m = menu();
    enter(&mut m, 2);
    let op = operation(press(&mut m, Input::Activate));
    assert!(!m.receive(Response::Mutation {
        id: RequestId {
            session: 8,
            serial: op.id.serial
        },
        outcome: Outcome::Success("wrong".into())
    }));
    assert!(m.receive(Response::Mutation {
        id: op.id,
        outcome: Outcome::Unknown("timeout".into())
    }));
    assert!(m.view().status.contains("Outcome unknown"));
    assert!(press(&mut m, Input::Activate).is_empty());
}
#[test]
fn unrelated_release_does_not_repeat_held_activation() {
    let mut m = menu();
    enter(&mut m, 2);
    m.input(Input::Release(ActionKey::Activate), m.revision());
    let op = operation(m.input(Input::Activate, m.revision()));
    assert!(m.receive(Response::Mutation {
        id: op.id,
        outcome: Outcome::Success("split".into())
    }));
    m.input(Input::Page, m.revision());
    m.input(Input::Release(ActionKey::Page), m.revision());
    assert!(
        m.input(Input::Activate, m.revision()).is_empty(),
        "held A must remain latched after another key release"
    );
}
#[test]
fn actual_saved_snapshot_and_conflict_replace_stale_versions() {
    let mut m = menu();
    enter(&mut m, 4);
    let op = operation(enter_label(&mut m, "Save"));
    let mut saved = settings();
    saved.version = "v2".into();
    saved.labels = LabelStyle::Xbox;
    assert!(m.settings_saved(op.id, saved.clone()));
    assert_eq!(m.settings(), &saved);
    let op = operation(enter_label(&mut m, "Save"));
    let mut latest = saved.clone();
    latest.version = "v3".into();
    latest.max_seconds = 99;
    assert!(m.settings_conflict(op.id, latest.clone()));
    assert_eq!(m.settings(), &latest);
    assert!(m.view().status.contains("reloaded"));
}

#[test]
fn context_compares_all_ids_but_retains_captured_labels() {
    let c = context();
    assert!(c.same_identity(&c));
    for key in 0..3 {
        let mut other = c.clone();
        match key {
            0 => other.workspace = "other".into(),
            1 => other.tab = "other".into(),
            _ => other.pane = "other".into(),
        };
        assert!(!c.same_identity(&other));
    }
    let mut m = menu();
    let revision = m.revision();
    let mut other = c.clone();
    other.pane_label = "Renamed externally".into();
    other.cwd = Some("/elsewhere".into());
    assert!(!m.observe_context(Ok(Some(other))));
    assert_eq!(m.context(), Some(&c));
    assert_eq!(m.revision(), revision);
    let mut m = Menu::new(1, None, settings(), "Space".into(), "branch".into());
    assert!(!m.observe_context(Ok(None)));
    assert!(m.is_open());
    let mut missing = context();
    missing.cwd = None;
    assert_eq!(missing.directory(), None);
    missing.cwd = Some("".into());
    assert_eq!(missing.directory(), None);
    missing.cwd = Some("relative".into());
    assert_eq!(missing.directory(), None);
    missing.cwd = Some("/absolute".into());
    assert_eq!(missing.directory(), Some("/absolute"));
}
#[test]
fn slot_exact_completion_reconciliation_and_new_operations() {
    let mut m = menu();
    enter(&mut m, 2);
    let op = operation(press(&mut m, Input::Activate));
    let mut slot = OperationSlot::default();
    assert_eq!(slot.operation(), None);
    assert!(!slot.blocked());
    assert!(slot.reserve(op.clone()));
    assert_eq!(slot.operation(), Some(&op));
    assert!(slot.blocked());
    let wrong = RequestId {
        session: op.id.session + 1,
        serial: op.id.serial,
    };
    assert!(!slot.complete(wrong, Outcome::Success("wrong".into())));
    assert!(!slot.reconcile(op.id, Outcome::Success("early".into())));
    assert!(slot.complete(op.id, Outcome::Unknown("timeout".into())));
    assert!(!slot.complete(op.id, Outcome::Success("late duplicate".into())));
    assert!(!slot.reconcile(wrong, Outcome::Success("wrong".into())));
    assert!(!slot.reconcile(op.id, Outcome::Unknown("still unknown".into())));
    assert!(slot.blocked());
    assert!(slot.reconcile(op.id, Outcome::Failed("not dispatched".into())));
    assert!(!slot.blocked());
    let mut next = op.clone();
    next.id.serial += 1;
    assert!(slot.reserve(next.clone()));
    assert!(slot.complete(next.id, Outcome::Failed("refused".into())));
    assert_eq!(
        slot.state(),
        Some(&OperationState::Finished(Outcome::Failed("refused".into())))
    );
}
#[test]
fn validation_boundaries_and_branch_syntax() {
    for field in [Field::Name, Field::Label, Field::Branch] {
        let limit = if field == Field::Branch { 200 } else { 80 };
        assert!(validate(field, &"猫".repeat(limit)).is_ok());
        assert!(validate(field, &"猫".repeat(limit + 1)).is_err());
        assert!(validate(field, "  ").is_err());
        assert!(validate(field, "abc\n").is_err());
    }
    for bad in [
        "-bad", "a b", "a..b", "a@{b", "a/", "a.", "/a", "a//b", "a~b", "a^b", "a:b", "a?b", "a*b",
        "a[b", "a\\b",
    ] {
        assert!(validate(Field::Branch, bad).is_err(), "{bad}");
    }
    assert!(validate(Field::Label, "-a b..@").is_ok());
}
#[test]
fn text_editor_boundaries_done_errors_and_three_pages() {
    let mut e = TextEditor::new(Field::Name, "".into());
    e.input(Input::Up);
    e.input(Input::Left);
    assert_eq!(e.selected, 0);
    e.input(Input::Down);
    assert_eq!(e.selected, 8);
    e.input(Input::Right);
    assert_eq!(e.selected, 9);
    e.input(Input::Left);
    assert_eq!(e.selected, 8);
    e.input(Input::Up);
    assert_eq!(e.selected, 0);
    for _ in 0..100 {
        e.input(Input::Down);
    }
    assert_eq!(e.selected, e.cells().len() - 1);
    assert_eq!(e.input(Input::Activate), EditResult::Continue);
    assert!(e.error.contains("nonempty"));
    e.buffer = "Good".into();
    assert_eq!(e.input(Input::Activate), EditResult::Done("Good".into()));
    e.input(Input::Page);
    assert_eq!(e.page, 1);
    e.input(Input::Page);
    assert_eq!(e.page, 2);
    assert_eq!(e.selected, e.cells().len() - 1);
    e.input(Input::Page);
    assert_eq!(e.page, 0);
    e.selected = 0;
    e.buffer = "猫".repeat(80);
    e.input(Input::Activate);
    assert_eq!(e.buffer.chars().count(), 80);
    assert!(e.error.contains("80"));
    e.input(Input::Delete);
    e.input(Input::Activate);
    assert!(e.error.is_empty());
    assert!(e.buffer.ends_with('a'));
    let mut branch = TextEditor::new(Field::Branch, "a".repeat(199));
    branch.input(Input::Activate);
    assert_eq!(branch.buffer.len(), 200);
    branch.input(Input::Activate);
    assert_eq!(branch.buffer.len(), 200);
    assert!(branch.error.contains("200"));
}
#[test]
fn busy_slot_disabled_rows_ack_and_closed_inputs_are_consumed() {
    let mut m = menu();
    m.set_external_busy(true);
    let rev = m.revision();
    m.set_external_busy(true);
    assert_eq!(m.revision(), rev);
    enter(&mut m, 2);
    assert!(press(&mut m, Input::Activate).is_empty());
    assert!(m.view().status.contains("pending"));
    m.set_external_busy(false);
    let op = operation(press(&mut m, Input::Activate));
    assert!(m.receive(Response::Mutation {
        id: op.id,
        outcome: Outcome::Failed("refused".into())
    }));
    assert!(m.pending().is_none());
    assert!(m.view().status.contains("refused"));
    press(&mut m, Input::View);
    assert!(!m.is_open());
    assert!(press(&mut m, Input::Activate).is_empty());
    assert!(!m.observe_context(Err("again".into())));
    let mut m = Menu::new(1, None, settings(), "Space".into(), "branch".into());
    assert!(press(&mut m, Input::Activate).is_empty());
    assert!(m.view().status.contains("directory"));
    assert_eq!(m.view().breadcrumb, "Main");
}
#[test]
fn settings_cycles_and_voice_details_are_read_only() {
    let mut m = menu();
    enter(&mut m, 4);
    for expected in [LabelStyle::PlayStation, LabelStyle::Xbox, LabelStyle::Both] {
        press(&mut m, Input::Activate);
        assert!(m.view().rows[0].label.contains(&format!("{expected:?}")));
    }
    for expected in ["L3", "Options/Menu", "R3"] {
        enter(&mut m, 1);
        assert!(m.view().rows[1].label.contains(expected));
    }
    for seconds in [15, 30, 60, 120, 300, 15] {
        enter(&mut m, 2);
        assert!(m.view().rows[2]
            .label
            .contains(&format!("{seconds} seconds")));
    }
    enter(&mut m, 3);
    let details = m.view().details.join(" ");
    assert!(details.contains("/models/base.bin"));
    assert!(details.contains("Language: en"));
    assert!(details.contains("/config.toml"));
    assert!(m.view().rows.is_empty());
    assert!(press(&mut m, Input::Activate).is_empty());
}
#[test]
fn repository_duplicate_branch_empty_base_and_response_kind_guards() {
    let mut m = menu();
    let req = read(enter(&mut m, 1));
    assert!(!m.receive(Response::Read {
        id: req.id,
        result: Ok(ReadData::Logs(vec!["wrong".into()]))
    }));
    let mut r = repository();
    r.entries[0].branch = Some("gamepad/123".into());
    assert!(m.receive(Response::Read {
        id: req.id,
        result: Ok(ReadData::Repository(r.clone()))
    }));
    let req = read(enter(&mut m, 0));
    assert!(m.receive(Response::Read {
        id: req.id,
        result: Ok(ReadData::Repository(r))
    }));
    assert!(m.view().rows[1]
        .disabled
        .as_ref()
        .unwrap()
        .contains("exists"));
    let mut m = menu();
    let req = read(enter(&mut m, 1));
    let mut r = repository();
    r.base_commit.clear();
    assert!(m.receive(Response::Read {
        id: req.id,
        result: Ok(ReadData::Repository(r.clone()))
    }));
    let req = read(enter(&mut m, 0));
    assert!(m.receive(Response::Read {
        id: req.id,
        result: Ok(ReadData::Repository(r))
    }));
    assert!(m.view().rows[1].disabled.as_ref().unwrap().contains("HEAD"));
}
#[test]
fn create_form_requests_fresh_pinned_head_and_disables_until_loaded() {
    let mut m = menu();
    load_repo(&mut m);
    let req = read(enter(&mut m, 0));
    assert!(m.view().rows[1].disabled.is_some());
    let mut fresh = repository();
    fresh.base_commit = "new-head".into();
    assert!(m.receive(Response::Read {
        id: req.id,
        result: Ok(ReadData::Repository(fresh))
    }));
    let op = operation(enter(&mut m, 1));
    assert!(matches!(op.command,Command::CreateWorktree{base,..} if base=="new-head"));
}
#[test]
fn expanded_details_scroll_is_bounded_by_rendered_lines_and_resizes() {
    let mut m = menu();
    m.set_detail_width(20);
    enter(&mut m, 3);
    enter(&mut m, 0);
    press(&mut m, Input::Details);
    let count = crate::menu_render::detail_lines(&m.view(), 20).len();
    assert!(count > 2);
    for _ in 0..1000 {
        press(&mut m, Input::Down);
    }
    assert_eq!(m.view().scroll, count - 1);
    press(&mut m, Input::Up);
    assert_eq!(m.view().scroll, count - 2);
    m.set_detail_width(200);
    assert!(m.view().scroll < 2);
    press(&mut m, Input::Back);
    assert!(!m.view().expanded);
    assert!(m.view().breadcrumb.ends_with("Confirm"));
    press(&mut m, Input::Delete);
    assert!(m.view().expanded);
    press(&mut m, Input::Delete);
    assert!(!m.view().expanded);
}

#[test]
fn every_action_key_requires_matching_release_and_current_ack() {
    for (input, key) in [
        (Input::Right, ActionKey::Right),
        (Input::Delete, ActionKey::Delete),
        (Input::Page, ActionKey::Page),
        (Input::Activate, ActionKey::Activate),
    ] {
        assert_eq!(input.action_key(), Some(key));
    }
    let mut m = menu();
    enter(&mut m, 0);
    enter(&mut m, 0);
    let old = m.revision();
    press(&mut m, Input::Activate);
    assert_eq!(m.view().editor.as_ref().unwrap().buffer, "Space 123a");
    assert!(m.revision() > old);
    for input in [Input::Delete, Input::Page, Input::Right] {
        let before = m.view();
        m.input(input, old);
        assert_eq!(m.view(), before);
    }
    m.input(Input::Delete, m.revision());
    assert_eq!(m.view().editor.as_ref().unwrap().buffer, "Space 123");
    m.input(Input::Page, m.revision());
    assert_eq!(m.view().editor.as_ref().unwrap().page, 1);
    m.input(Input::Delete, m.revision());
    assert_eq!(m.view().editor.as_ref().unwrap().buffer, "Space 123");
    m.input(Input::Release(ActionKey::Delete), m.revision());
    m.input(Input::Delete, m.revision());
    assert_eq!(m.view().editor.as_ref().unwrap().buffer, "Space 12");
}
#[test]
fn pane_leaf_commands_and_editor_commit_are_frozen() {
    for (index, expected) in [
        (2, Command::ToggleZoom),
        (4, Command::RenamePane { label: None }),
    ] {
        let mut m = menu();
        enter(&mut m, 2);
        let op = operation(enter(&mut m, index));
        assert_eq!(op.command, expected);
        assert_eq!(op.context, Some(context()));
    }
    let mut m = menu();
    enter(&mut m, 2);
    select(&mut m, 3);
    press(&mut m, Input::Right);
    assert!(m.view().breadcrumb.ends_with("Text entry"));
    assert!(m.view().details.join(" ").contains("80"));
    press(&mut m, Input::Activate);
    for _ in 0..20 {
        press(&mut m, Input::Down);
    }
    press(&mut m, Input::Activate);
    assert!(m.view().rows[3].label.contains("Shella"));
    let op = operation(enter(&mut m, 6));
    assert_eq!(
        op.command,
        Command::RenamePane {
            label: Some("Shella".into())
        }
    );
    let mut m = menu();
    enter(&mut m, 2);
    select(&mut m, 5);
    press(&mut m, Input::Right);
    assert!(m.view().breadcrumb.ends_with("Confirm"));
    let op = operation(enter(&mut m, 1));
    assert_eq!(op.command, Command::ClosePane);
}
#[test]
fn semantic_revisions_invalidate_ack_for_context_and_async_responses() {
    let mut m = menu();
    let initial = m.revision();
    m.set_external_busy(true);
    assert_eq!(m.revision(), initial + 1);
    let old = m.revision();
    assert!(m.observe_context(Ok(None)));
    assert_eq!(m.revision(), old + 1);
    let mut m = menu();
    let req = read(enter(&mut m, 5));
    let rev = m.revision();
    assert!(!m.receive(Response::Read {
        id: req.id,
        result: Ok(ReadData::Repository(repository()))
    }));
    assert!(m.receive(Response::Read {
        id: req.id,
        result: Ok(ReadData::Logs(vec!["log".into()]))
    }));
    assert_eq!(m.revision(), rev + 1);
    let next = read(press(&mut m, Input::Activate));
    assert_eq!(next.id.serial, req.id.serial + 1);
    let mut m = menu();
    enter(&mut m, 0);
    assert!(m.view().details.join(" ").contains("/tmp/project"));
}
#[test]
fn settings_commit_requires_matching_save_operation_and_updates_revision() {
    let mut m = menu();
    enter(&mut m, 2);
    let op = operation(press(&mut m, Input::Activate));
    assert!(!m.settings_saved(op.id, settings()));
    assert!(!m.settings_conflict(op.id, settings()));
    let mut m = menu();
    enter(&mut m, 4);
    let op = operation(enter_label(&mut m, "Save"));
    let wrong = RequestId {
        session: op.id.session + 1,
        serial: op.id.serial,
    };
    assert!(!m.settings_saved(wrong, settings()));
    assert!(!m.settings_conflict(wrong, settings()));
    let rev = m.revision();
    let mut saved = settings();
    saved.voice_button = Btn::Menu;
    assert!(m.settings_saved(op.id, saved));
    assert_eq!(m.revision(), rev + 1);
    assert_eq!(m.view().menu_button, "Options/Menu");
    let op = operation(enter_label(&mut m, "Save"));
    let rev = m.revision();
    assert!(m.settings_conflict(op.id, settings()));
    assert_eq!(m.revision(), rev + 1);
}
#[test]
fn right_opens_only_submenu_and_discard_exits_reset_draft() {
    let mut m = menu();
    enter(&mut m, 4);
    select(&mut m, 3);
    press(&mut m, Input::Right);
    assert!(m.view().breadcrumb.ends_with("Voice configuration"));
    press(&mut m, Input::Back);
    select(&mut m, 0);
    let before = m.view();
    press(&mut m, Input::Right);
    assert_eq!(m.view(), before);
    press(&mut m, Input::Activate);
    press(&mut m, Input::Left);
    enter(&mut m, 4);
    assert!(m.view().rows[0].label.ends_with("Both"));
    let mut m = menu();
    enter(&mut m, 3);
    enter(&mut m, 0);
    press(&mut m, Input::Details);
    let before = m.view();
    assert!(press(&mut m, Input::Activate).is_empty());
    assert_eq!(m.view(), before);
}
#[test]
fn refreshed_repository_cannot_retarget_session() {
    let mut m = menu();
    load_repo(&mut m);
    let req = read(enter(&mut m, 0));
    let mut other = repository();
    other.repo_key = "/different/.git".into();
    assert!(m.receive(Response::Read {
        id: req.id,
        result: Ok(ReadData::Repository(other))
    }));
    assert!(m.view().rows[1].disabled.is_some());
    assert!(m.view().status.contains("Repository changed"));
    assert!(enter(&mut m, 1).is_empty());
    let mut m = menu();
    load_repo(&mut m);
    enter(&mut m, 1);
    let op = operation(enter(&mut m, 1));
    assert_eq!(op.expected_repository, Some("/tmp/project/.git".into()));
}
#[test]
fn text_right_boundary_and_viewport_revision_are_exact() {
    let mut e = TextEditor::new(Field::Name, "name".into());
    for _ in 0..100 {
        e.input(Input::Right);
    }
    assert_eq!(e.selected, e.cells().len() - 1);
    let mut m = menu();
    let rev = m.revision();
    m.set_detail_width(20);
    assert_eq!(m.revision(), rev + 1);
    m.set_detail_width(20);
    assert_eq!(m.revision(), rev + 1);
    m.set_detail_width(0);
    assert_eq!(m.revision(), rev + 2);
    m.set_detail_width(1);
    assert_eq!(m.revision(), rev + 2);
}
#[test]
fn settings_success_requires_committed_version_not_generic_success() {
    let mut m = menu();
    enter(&mut m, 4);
    let op = operation(enter_label(&mut m, "Save"));
    assert!(!m.receive(Response::Mutation {
        id: op.id,
        outcome: Outcome::Success("saved".into())
    }));
    assert!(m.pending().is_some());
    let mut saved = settings();
    saved.version = "committed-v2".into();
    assert!(m.settings_saved(op.id, saved));
    assert_eq!(m.settings().version, "committed-v2");
}
#[test]
fn successful_structural_action_consumes_confirmation_and_session() {
    let mut m = menu();
    enter(&mut m, 3);
    enter(&mut m, 0);
    let op = operation(enter(&mut m, 1));
    assert!(m.receive(Response::Mutation {
        id: op.id,
        outcome: Outcome::Success("closed".into())
    }));
    assert!(!m.is_open());
    assert!(press(&mut m, Input::Activate).is_empty());
    let mut m = menu();
    enter(&mut m, 0);
    let op = operation(enter(&mut m, 1));
    assert!(m.receive(Response::Mutation {
        id: op.id,
        outcome: Outcome::Success("created".into())
    }));
    assert!(!m.is_open());
}
#[test]
fn successful_settings_save_defers_display_bindings_until_session_exit() {
    let mut m = menu();
    enter(&mut m, 4);
    let op = operation(enter_label(&mut m, "Save"));
    let mut saved = settings();
    saved.version = "v2".into();
    saved.labels = LabelStyle::Xbox;
    saved.voice_button = Btn::Menu;
    assert!(m.settings_saved(op.id, saved.clone()));
    assert_eq!(m.view().labels, LabelStyle::Both);
    assert_eq!(m.view().menu_button, "Options/Menu");
    press(&mut m, Input::Back);
    enter(&mut m, 6);
    assert!(m.view().details[0].contains("Voice: R3"));
    let m = Menu::new(8, Some(context()), saved, "Space".into(), "branch".into());
    assert_eq!(m.view().labels, LabelStyle::Xbox);
    assert_eq!(m.view().menu_button, "L3");
}
#[test]
fn reopened_menu_displays_global_unknown_target_and_waits_for_reconciliation() {
    let mut first = menu();
    enter(&mut first, 2);
    let op = operation(press(&mut first, Input::Activate));
    let mut slot = OperationSlot::default();
    assert!(slot.reserve(op.clone()));
    assert!(slot.complete(op.id, Outcome::Unknown("timeout".into())));
    let mut reopened = Menu::new(8, None, settings(), "Space".into(), "branch".into());
    reopened.sync_operation_slot(&slot);
    assert!(reopened.view().status.contains("Outcome unknown"));
    assert!(reopened.view().details.join(" ").contains("pane-1"));
    let rev = reopened.revision();
    reopened.sync_operation_slot(&slot);
    assert_eq!(reopened.revision(), rev);
    enter(&mut reopened, 4);
    assert!(enter_label(&mut reopened, "Save").is_empty());
    assert!(slot.reconcile(op.id, Outcome::Failed("not dispatched".into())));
    reopened.sync_operation_slot(&slot);
    let op = operation(press(&mut reopened, Input::Activate));
    assert!(matches!(op.command, Command::SaveSettings { .. }));
}
#[test]
fn nonstructural_success_keeps_menu_open_and_structural_failure_is_recoverable() {
    let mut m = menu();
    enter(&mut m, 2);
    let op = operation(press(&mut m, Input::Activate));
    assert!(m.receive(Response::Mutation {
        id: op.id,
        outcome: Outcome::Success("split".into())
    }));
    assert!(m.is_open());
    let second = operation(press(&mut m, Input::Activate));
    assert_ne!(op.id, second.id);
    let mut m = menu();
    enter(&mut m, 3);
    enter(&mut m, 0);
    let op = operation(enter(&mut m, 1));
    assert!(m.receive(Response::Mutation {
        id: op.id,
        outcome: Outcome::Failed("refused".into())
    }));
    assert!(m.is_open());
    assert!(m.view().status.contains("refused"));
}
#[test]
fn pending_to_unknown_global_slot_updates_view_and_semantic_revision() {
    let mut first = menu();
    enter(&mut first, 2);
    let op = operation(press(&mut first, Input::Activate));
    let mut slot = OperationSlot::default();
    assert!(slot.reserve(op.clone()));
    let mut reopened = menu();
    let rev = reopened.revision();
    reopened.sync_operation_slot(&slot);
    assert_eq!(reopened.revision(), rev + 1);
    assert!(reopened.view().status.starts_with("Pending"));
    assert!(slot.complete(op.id, Outcome::Unknown("timeout".into())));
    reopened.sync_operation_slot(&slot);
    assert_eq!(reopened.revision(), rev + 2);
    assert!(reopened.view().status.starts_with("Outcome unknown"));
}

#[test]
fn pending_settings_never_render_configuration_version_contents() {
    let mut settings = settings();
    settings.version = "secret-token-that-must-not-render".into();
    let mut m = Menu::new(901, None, settings, "n".into(), "b".into());
    for _ in 0..4 {
        let r = m.revision();
        m.input(Input::Down, r);
    }
    let r = m.revision();
    m.input(Input::Activate, r);
    m.input(Input::Release(ActionKey::Activate), 0);
    for _ in 0..5 {
        let r = m.revision();
        m.input(Input::Down, r);
    }
    let r = m.revision();
    let effects = m.input(Input::Activate, r);
    assert!(matches!(effects.first(), Some(Effect::Mutate(_))));
    assert!(!m
        .view()
        .details
        .join("\n")
        .contains("secret-token-that-must-not-render"));
}
#[test]
fn settings_form_uses_disk_baseline_while_legends_use_active_controls() {
    let disk = settings();
    let mut active = disk.clone();
    active.voice_button = Btn::Menu;
    active.labels = LabelStyle::Xbox;
    let mut m =
        Menu::new(999, None, disk.clone(), "n".into(), "b".into()).with_active_settings(active);
    assert_eq!(m.settings(), &disk);
    assert_eq!(m.view().menu_button, "L3");
    assert_eq!(m.view().labels, LabelStyle::Xbox);
    for _ in 0..4 {
        let rev = m.revision();
        m.input(Input::Down, rev);
    }
    let rev = m.revision();
    m.input(Input::Activate, rev);
    assert!(m.view().rows[1].label.contains("R3"));
}
#[test]
fn reopening_shows_actual_returned_checkout_path_without_rearming_action() {
    let mut m = Menu::new(888, None, settings(), "n".into(), "b".into());
    let mut slot = OperationSlot::default();
    let operation = Operation {
        id: RequestId {
            session: 887,
            serial: 1,
        },
        context: None,
        expected_repository: None,
        expected_repository_root: None,
        command: Command::CreateWorktree {
            branch: "b".into(),
            base: "commit".into(),
            repository: "/repo".into(),
        },
    };
    assert!(slot.reserve(operation.clone()));
    assert!(slot.complete(
        operation.id,
        Outcome::Success("Created checkout /actual/server-chosen/path".into())
    ));
    m.sync_operation_slot(&slot);
    assert!(m.view().status.contains("/actual/server-chosen/path"));
    assert!(!slot.blocked());
    assert!(m.pending().is_none());
}
#[test]
fn completed_long_checkout_path_is_available_in_scrollable_details() {
    let path = format!("/actual/{}/checkout", "long-directory/".repeat(30));
    let mut m = Menu::new(888, None, settings(), "n".into(), "b".into());
    let mut slot = OperationSlot::default();
    let operation = Operation {
        id: RequestId {
            session: 887,
            serial: 1,
        },
        context: None,
        expected_repository: None,
        expected_repository_root: None,
        command: Command::ToggleZoom,
    };
    assert!(slot.reserve(operation.clone()));
    assert!(slot.complete(operation.id, Outcome::Success(path.clone())));
    m.sync_operation_slot(&slot);
    assert!(m.view().details.iter().any(|s| s.contains(&path)));
    let rev = m.revision();
    m.input(Input::Details, rev);
    m.set_detail_width(30);
    let initial = crate::menu_render::render(&m.view(), 30, 10);
    for _ in 0..30 {
        let rev = m.revision();
        m.input(Input::Down, rev);
    }
    let scrolled = crate::menu_render::render(&m.view(), 30, 10);
    assert_ne!(initial, scrolled);
    assert!(scrolled.contains("checkout"));
}
#[test]
fn retained_failure_is_visible_without_rearming_and_settings_operation_is_named() {
    let mut m = Menu::new(888, None, settings(), "n".into(), "b".into());
    let mut slot = OperationSlot::default();
    let operation = Operation {
        id: RequestId {
            session: 887,
            serial: 1,
        },
        context: None,
        expected_repository: None,
        expected_repository_root: None,
        command: Command::SaveSettings {
            original: settings(),
            updated: settings(),
        },
    };
    assert!(slot.reserve(operation.clone()));
    assert!(slot.complete(operation.id, Outcome::Failed("disk full".into())));
    m.sync_operation_slot(&slot);
    assert_eq!(m.view().status, "Last failure: disk full");
    assert!(m.view().details.iter().any(|s| s.contains("Save settings")));
    assert!(!slot.blocked());
    assert!(m.pending().is_none());
    let rev = m.revision();
    assert!(m.input(Input::Activate, rev).is_empty());
}

#[test]
fn controls_body_uses_selected_controller_labels_and_voice_binding() {
    for (style, voice, expected) in [
        (
            LabelStyle::PlayStation,
            Btn::Menu,
            vec![
                "Voice: Options; Menu: L3",
                "Left/○ Circle: back",
                "✕ Cross: activate once",
                "L3/Create/Share: close",
                "□ Square: details",
                "△ Triangle: text page",
            ],
        ),
        (
            LabelStyle::Xbox,
            Btn::RStick,
            vec![
                "Voice: Right stick; Menu: Menu",
                "Left/B: back",
                "A: activate once",
                "Menu/View: close",
                "X: details",
                "Y: text page",
            ],
        ),
        (
            LabelStyle::Both,
            Btn::LStick,
            vec![
                "Voice: L3/Left stick; Menu: Options/Menu",
                "Left/○ Circle/B: back",
                "✕ Cross/A: activate once",
                "Options/Menu/Create/Share/View: close",
                "□ Square/X: details",
                "△ Triangle/Y: text page",
            ],
        ),
    ] {
        let mut cfg = settings();
        cfg.labels = style;
        cfg.voice_button = voice;
        let mut m = Menu::new(9, Some(context()), cfg, "n".into(), "b".into());
        enter(&mut m, 6);
        let body = m.view().details.join("\n");
        for label in expected {
            assert!(body.contains(label), "missing {label:?} in {body:?}");
        }
    }
}

#[test]
fn pane_row_labels_drive_their_corresponding_actions() {
    for (label, expected) in [
        (
            "Split right",
            Command::Split {
                direction: SplitDirection::Right,
                cwd: "/tmp/project".into(),
            },
        ),
        (
            "Split down",
            Command::Split {
                direction: SplitDirection::Down,
                cwd: "/tmp/project".into(),
            },
        ),
        ("Toggle zoom", Command::ToggleZoom),
        ("Clear label", Command::RenamePane { label: None }),
    ] {
        let mut m = menu();
        enter(&mut m, 2);
        let selected = m.view().rows.iter().position(|r| r.label == label).unwrap();
        select(&mut m, selected);
        let op = operation(press(&mut m, Input::Activate));
        assert_eq!(op.command, expected, "row {label}");
    }
}

#[test]
fn controls_names_all_voice_bindings_in_the_selected_style() {
    for (style, voices, menus) in [
        (
            LabelStyle::PlayStation,
            ["R3", "L3", "Options"],
            ["Options", "Options", "L3"],
        ),
        (
            LabelStyle::Xbox,
            ["Right stick", "Left stick", "Menu"],
            ["Menu", "Menu", "Left stick"],
        ),
        (
            LabelStyle::Both,
            ["R3/Right stick", "L3/Left stick", "Options/Menu"],
            ["Options/Menu", "Options/Menu", "L3/Left stick"],
        ),
    ] {
        for (i, voice) in [Btn::RStick, Btn::LStick, Btn::Menu]
            .into_iter()
            .enumerate()
        {
            let mut cfg = settings();
            cfg.labels = style;
            cfg.voice_button = voice;
            let mut m = Menu::new(19, None, cfg, "n".into(), "b".into());
            enter(&mut m, 6);
            assert_eq!(
                m.view().details[0],
                format!("Voice: {}; Menu: {}", voices[i], menus[i])
            );
        }
    }
}

#[test]
fn preview_setting_toggles_in_draft_and_saves_explicitly() {
    let mut m = menu();
    enter(&mut m, 4);
    let index = m.view().rows.iter().position(|r| r.label == "Preview dictation before sending: Off").expect("preview preference row");
    let save = m.view().rows.iter().position(|r| r.label == "Save").unwrap();
    let discard = m.view().rows.iter().position(|r| r.label == "Discard").unwrap();
    assert!(index < save && save < discard, "preferences must appear before commit/discard actions");
    enter(&mut m, index);
    assert_eq!(m.view().rows[index].label, "Preview dictation before sending: On");
    enter(&mut m, index);
    assert_eq!(m.view().rows[index].label, "Preview dictation before sending: Off");
    enter(&mut m, index);
    assert!(!m.settings().preview_before_send);
    let op = operation(enter_label(&mut m, "Save"));
    match op.command {
        Command::SaveSettings { original, updated } => {
            assert!(!original.preview_before_send);
            assert!(updated.preview_before_send);
        }
        other => panic!("{other:?}"),
    }
    let mut m = menu();
    enter(&mut m, 4);
    enter(&mut m, index);
    enter_label(&mut m, "Discard");
    enter(&mut m, 4);
    assert_eq!(m.view().rows[index].label, "Preview dictation before sending: Off");
}

#[test]
fn retained_voice_draft_can_be_reopened_from_actions() {
    let mut m=menu(); m.set_voice_draft_available(true);
    let index=m.view().rows.iter().position(|r|r.label=="Review voice draft").expect("draft review row");
    assert_eq!(enter(&mut m,index),vec![Effect::ReviewVoiceDraft]);
    let mut m=menu();
    let index=m.view().rows.iter().position(|r|r.label=="Review voice draft").unwrap();
    assert!(m.view().rows[index].disabled.is_some());
    assert!(enter(&mut m,index).is_empty());
}

#[test]
fn draft_availability_changes_revision_only_when_the_row_changes() {
    let mut m=menu(); let before=m.revision();
    m.set_voice_draft_available(true); assert!(m.revision()>before);
    let ready=m.revision(); m.set_voice_draft_available(true); assert_eq!(m.revision(),ready);
    m.set_voice_draft_available(false); assert!(m.revision()>ready);
}
