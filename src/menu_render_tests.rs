use crate::menu::*;
use crate::menu_render::*;
fn view() -> View {
    View {
        voice_preview: None,
        revision: 4,
        scope: "Project [ws-1] / Shell [pane-1]".into(),
        breadcrumb: "Main / Close…".into(),
        rows: vec![
            Row {
                label: "Cancel".into(),
                disabled: None,
            },
            Row {
                label: "Confirm 猫 close".into(),
                disabled: None,
            },
        ],
        selected: 1,
        details: vec!["target /tmp/猫/very-long-name\x1b[2J\r\nnext".into()],
        status: "Ready".into(),
        scroll: 0,
        expanded: false,
        labels: LabelStyle::Both,
        editor: None,
        menu_button: "Options/Menu".into(),
    }
}
#[test]
fn render_structure_selection_and_terminal_safety() {
    let out = render(&view(), 64, 12);
    assert!(out.contains("Main / Close…"));
    assert!(out.contains("> Confirm 猫 close"));
    assert!(out.contains("Cross"));
    assert!(out.contains("Circle"));
    assert!(!out.contains('\x1b'));
    assert!(!out.contains('\r'));
    assert!(out.contains("target"));
}
#[test]
fn width_clipping_and_small_viewports() {
    assert_eq!(clip("猫ab", 3), "猫a");
    assert_eq!(clip("e\u{301}猫", 1), "e\u{301}");
    for width in [1, 4, 12, 40] {
        let out = render(&view(), width, 8);
        assert!(out
            .lines()
            .all(|l| unicode_width::UnicodeWidthStr::width(l) <= width));
        assert!(out.lines().count() <= 8);
    }
    let out = render(&view(), 24, 8);
    assert!(out.contains("> Confirm"));
}
#[test]
fn expanded_details_show_full_target_over_scroll() {
    let mut v = view();
    v.expanded = true;
    v.details = vec!["/tmp/long/猫/target/with/a/name".into()];
    let first = render(&v, 16, 8);
    assert!(first.contains("/tmp/long/猫"));
    v.scroll = 1;
    let next = render(&v, 16, 8);
    assert_ne!(first, next);
    assert!(next.contains("with/a/name"));
    assert!(first
        .lines()
        .skip(2)
        .take(5)
        .collect::<String>()
        .contains("/tmp/long/猫/target/with/a/name"));
    v.scroll = 999;
    assert!(render(&v, 16, 8).contains("with/a/name"));
}
#[test]
fn external_controls_are_visible_text_and_legends_follow_preference() {
    assert_eq!(sanitize("a\x1b\n\t\u{7f}b"), "a�  �b");
    let mut v = view();
    v.labels = LabelStyle::Xbox;
    let out = render(&v, 80, 12);
    assert!(out.contains("A Select"));
    assert!(!out.contains("Cross"));
    v.labels = LabelStyle::PlayStation;
    assert!(render(&v, 80, 12).contains("✕ Cross"));
}
#[test]
fn narrow_status_preserves_selected_action_and_voice_menu_footer() {
    let mut v = view();
    v.status = "Pending operation".into();
    v.menu_button = "L3".into();
    let out = render(&v, 80, 5);
    assert!(out.contains("> Confirm"));
    assert!(out.contains("L3/Left stick Close"));
}
#[test]
fn editor_renders_limit_tail_and_selected_cell_in_narrow_grid() {
    let mut v = view();
    let mut editor = TextEditor::new(Field::Name, format!("{}TAIL", "a".repeat(60)));
    editor.selected = 8;
    v.editor = Some(editor);
    let out = render(&v, 24, 8);
    assert!(out.contains("TAIL"));
    assert!(out.contains("64/80"));
    assert!(out.contains(">i"));
}
#[test]
fn renderer_handles_empty_dimensions_exact_width_and_all_grid_positions() {
    let mut v = view();
    assert_eq!(render(&v, 0, 8), "");
    assert_eq!(render(&v, 10, 0), "");
    v.expanded = true;
    v.details = vec!["a猫b".into()];
    assert_eq!(detail_lines(&v, 1), vec!["a", "�", "b"]);
    assert_eq!(detail_lines(&v, 2), vec!["a", "猫", "b"]);
    assert!(detail_lines(&v, 0).is_empty());
    v.expanded = false;
    v.rows = vec![];
    v.details = vec![];
    v.status.clear();
    for width in [1, 7, 14, 24, 64] {
        for selected in [0, 1, 7, 8, 15, 38, 39] {
            let mut e = TextEditor::new(Field::Name, "猫ab".into());
            e.selected = selected;
            v.editor = Some(e);
            for height in 1..10 {
                let out = render(&v, width, height);
                assert_eq!(out.split('\n').count(), height);
                assert!(out
                    .lines()
                    .all(|line| unicode_width::UnicodeWidthStr::width(line) <= width));
            }
        }
    }
    v.editor = Some(TextEditor::new(Field::Name, "猫ab".into()));
    assert_eq!(render(&v, 4, 8).lines().nth(2), Some("猫ab"));
    assert_eq!(render(&v, 3, 8).lines().nth(2), Some("ab"));
}
#[test]
fn list_layout_reserves_status_detail_footer_and_keeps_selection() {
    let mut v = view();
    v.rows = (0..10)
        .map(|i| Row {
            label: format!("row{i}"),
            disabled: None,
        })
        .collect();
    v.selected = 5;
    v.details = vec!["full target details".into()];
    v.status = "Pending".into();
    let out = render(&v, 80, 8);
    let lines = out.lines().collect::<Vec<_>>();
    assert_eq!(lines[2], "  row3");
    assert_eq!(lines[4], "> row5");
    assert_eq!(lines[5], "full target details");
    assert_eq!(lines[6], "Pending");
    assert!(lines[7].contains("Close"));
    let out = render(&v, 80, 4);
    assert_eq!(out.lines().nth(2), Some("> row5"));
    assert!(!out.contains("Pending"));
    v.status.clear();
    v.details.clear();
    let out = render(&v, 80, 8);
    assert_eq!(out.lines().nth(2), Some("  row1"));
    assert_eq!(out.lines().nth(6), Some("> row5"));
    v.rows[5].disabled = Some("No target".into());
    assert!(render(&v, 80, 8).contains("> row5 (unavailable)"));
}
#[test]
fn log_and_controls_scrolling_editor_errors_and_legend_pages() {
    let mut v = view();
    v.breadcrumb = "Main / Logs".into();
    v.rows = vec![Row {
        label: "Refresh".into(),
        disabled: None,
    }];
    v.details = vec!["first".into(), "second".into(), "third".into()];
    v.status.clear();
    v.scroll = 1;
    let out = render(&v, 80, 8);
    assert_eq!(out.lines().nth(2), Some("> Refresh"));
    assert_eq!(out.lines().nth(3), Some("second"));
    assert!(!out.contains("first"));
    v.rows.clear();
    v.breadcrumb = "Main / Controls".into();
    assert_eq!(render(&v, 80, 8).lines().nth(2), Some("second"));
    v.scroll = 999;
    assert_eq!(render(&v, 80, 8).lines().nth(2), Some("third"));
    let mut editor = TextEditor::new(Field::Branch, "b".into());
    editor.error = "Invalid branch".into();
    editor.selected = 10;
    v.editor = Some(editor);
    v.status = "do not show status over editor".into();
    for label in [LabelStyle::Both, LabelStyle::PlayStation, LabelStyle::Xbox] {
        v.labels = label;
        let out = render(&v, 120, 10);
        assert!(out.contains("1/200 chars"));
        assert!(out.contains("Invalid branch"));
        assert!(out.contains("Delete"));
        assert!(out.contains("Page"));
        assert!(!out.contains("do not show"));
    }
    v.expanded = true;
    v.editor = None;
    assert!(!render(&v, 80, 8).contains("do not show"));
}
#[test]
fn emoji_sequences_and_bidi_controls_cannot_overflow_or_reorder_targets() {
    let mut v = view();
    v.scope = "❤️❤️❤️".into();
    v.details = vec!["👩‍💻❤️🇨🇦".into()];
    for width in 1..8 {
        let out = render(&v, width, 8);
        assert!(out
            .lines()
            .all(|l| unicode_width::UnicodeWidthStr::width(l) <= width));
    }
    assert_eq!(
        sanitize("safe\u{202e}evil\u{2066}x\u{2069}"),
        "safe�evil�x�"
    );
    assert_eq!(sanitize("👩‍💻"), "👩‍💻");
}
#[test]
fn standard_popup_legends_always_show_cancel_and_close_without_replacement_artifacts() {
    let mut v = view();
    v.status.clear();
    for style in [LabelStyle::Both, LabelStyle::PlayStation, LabelStyle::Xbox] {
        v.labels = style;
        let out = render(&v, 64, 12);
        let footer = out.lines().last().unwrap();
        assert!(footer.contains("Close"), "{footer}");
        assert!(footer.contains("Back"), "{footer}");
        assert!(!footer.contains("Options/Options"));
        v.editor = Some(TextEditor::new(Field::Name, "name".into()));
        let out = render(&v, 64, 12);
        assert!(out.lines().last().unwrap().contains("Cancel"));
        v.editor = None;
    }
}
#[test]
fn every_grid_selection_stays_visible_in_its_fixed_eight_column_row() {
    let mut v = view();
    v.status.clear();
    for width in [7, 14, 24, 64] {
        for selected in 0..40 {
            let mut e = TextEditor::new(Field::Name, "Name".into());
            e.selected = selected;
            let cell = e.cells()[selected].clone();
            v.editor = Some(e);
            let out = render(&v, width, 10);
            let row = out.lines().nth(4).unwrap();
            let label = if cell == " " { "Space" } else { &cell };
            assert!(
                row.contains(&format!(">{label}")),
                "width={width} cell={selected} row={row:?}"
            );
        }
    }
}
#[test]
fn small_rows_status_and_detail_slots_are_stable() {
    let mut v = view();
    v.rows = vec![Row {
        label: "Only".into(),
        disabled: None,
    }];
    v.selected = 0;
    v.status.clear();
    v.details = vec!["target".into()];
    let out = render(&v, 80, 5);
    assert_eq!(out.lines().nth(2), Some("> Only"));
    assert_eq!(out.lines().nth(3), Some(""));
    v.status = "status".into();
    let out = render(&v, 80, 5);
    assert_eq!(out.lines().nth(3), Some("status"));
    v.details.clear();
    let out = render(&v, 80, 5);
    assert_eq!(out.lines().nth(3), Some("status"));
}
#[test]
fn grid_uses_available_columns_and_long_logs_reserve_status() {
    let mut v = view();
    v.status.clear();
    v.editor = Some(TextEditor::new(Field::Name, "Name".into()));
    let out = render(&v, 64, 10);
    let row = out.lines().nth(4).unwrap();
    assert!(row.contains(" h"));
    let mut e = v.editor.take().unwrap();
    e.selected = 7;
    v.editor = Some(e);
    let out = render(&v, 14, 10);
    let row = out.lines().nth(4).unwrap();
    assert!(row.contains(" g"));
    assert!(row.contains(">h"));
    v.editor = None;
    v.breadcrumb = "Main / Logs".into();
    v.rows = vec![Row {
        label: "Refresh".into(),
        disabled: None,
    }];
    v.details = (0..20).map(|i| format!("line{i}")).collect();
    v.status = "Loading…".into();
    let out = render(&v, 64, 8);
    assert_eq!(out.lines().nth(6), Some("Loading…"));
    assert!(out.lines().last().unwrap().contains("Close"));
}

#[test]
fn xbox_editor_keeps_all_actions_on_one_footer_line() {
    let mut v = view();
    v.labels = LabelStyle::Xbox;
    v.editor = Some(TextEditor::new(Field::Name, "Draft".into()));
    let rendered = render(&v, 64, 5);
    let footer = rendered.lines().last().unwrap();
    for action in ["A Insert/Done", "X Delete", "Y Page", "B Cancel"] {
        assert!(footer.contains(action), "missing {action}: {footer}");
    }
}

#[test]
fn voice_preview_shows_scrollable_unicode_draft_and_style_controls() {
    let mut v = view();
    v.voice_preview = Some(crate::voice_preview::PreviewView {
        text: "猫".repeat(90),
        destination: "Agent [w1:p2]".into(),
        status: "Ready to send".into(),
        voice_button: "R3".into(),
    });
    v.labels = LabelStyle::PlayStation;
    let first = render(&v, 40, 12);
    assert!(first.contains("Voice message"));
    assert!(first.contains("Cross Send"));
    assert!(first.contains("Square Delete"));
    assert!(first.contains("Circle Back"));
    assert!(first.contains("Hold R3"));
    assert!(first.contains("w1:p2"));
    assert!(!first.contains("Select"));
    assert!(first
        .lines()
        .all(|l| unicode_width::UnicodeWidthStr::width(l) <= 40));
    assert!(first.lines().count() <= 12);
    v.voice_preview.as_mut().unwrap().text = "head ".repeat(50) + "TAIL";
    let first = render(&v, 40, 12);
    v.scroll = 6;
    let last = render(&v, 40, 12);
    assert_ne!(first, last);
    assert!(last.contains("TAIL"));
    v.labels = LabelStyle::Xbox;
    assert!(render(&v, 40, 12).contains("A Send"));
}
