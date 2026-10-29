//! Plain-text terminal frames. Only the viewer adds terminal control sequences.
use crate::{
    engine::Btn,
    menu::{LabelStyle, View},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub fn sanitize(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\n' | '\r' | '\t' => ' ',
            '\u{061c}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2066}'..='\u{2069}' => '�',
            c if c.is_control() => '�',
            c => c,
        })
        .collect()
}
pub fn clip(text: &str, width: usize) -> String {
    let clean = sanitize(text);
    let mut result = String::new();
    for grapheme in clean.graphemes(true) {
        if UnicodeWidthStr::width(format!("{result}{grapheme}").as_str()) > width {
            break;
        }
        result.push_str(grapheme);
    }
    result
}
fn wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![];
    }
    let clean = sanitize(text);
    let mut lines = vec![];
    let mut line = String::new();
    for grapheme in clean.graphemes(true) {
        let grapheme = if UnicodeWidthStr::width(grapheme) > width {
            "�"
        } else {
            grapheme
        };
        if UnicodeWidthStr::width(format!("{line}{grapheme}").as_str()) > width {
            lines.push(std::mem::take(&mut line));
        }
        line.push_str(grapheme);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}
fn legend(style: LabelStyle, editing: bool, menu_button: &str) -> Vec<String> {
    let select = style.button(Btn::A);
    let back = style.button(Btn::B);
    let details = style.button(Btn::X);
    let page = style.button(Btn::Y);
    let close = style.button(if menu_button == "L3" {
        Btn::LStick
    } else {
        Btn::Menu
    });
    if editing {
        if style == LabelStyle::Xbox {
            vec![format!(
                "{select} Insert/Done · {details} Delete · {page} Page · {back} Cancel"
            )]
        } else {
            vec![
                format!("{select} Insert/Done · {details} Delete"),
                format!("{page} Page · {back} Cancel"),
            ]
        }
    } else {
        match style {
            LabelStyle::Both => vec![format!(
                "{select} OK|{back} Back|{details} Info|{close} Close"
            )],
            LabelStyle::PlayStation => vec![format!(
                "{select} Select|{back} Back|{details} Details|{close} Close"
            )],
            LabelStyle::Xbox => vec![format!(
                "{select} Select · {back} Back · {details} Details · {close}/{} Close",
                style.button(Btn::View)
            )],
        }
    }
}
pub fn detail_lines(view: &View, width: usize) -> Vec<String> {
    view.details.iter().flat_map(|s| wrap(s, width)).collect()
}

fn tail(text: &str, width: usize) -> String {
    let clean = sanitize(text);
    let mut result = String::new();
    for grapheme in clean.graphemes(true).rev() {
        let next = format!("{grapheme}{result}");
        if UnicodeWidthStr::width(next.as_str()) > width {
            break;
        }
        result = next;
    }
    result
}

pub fn render(view: &View, width: usize, height: usize) -> String {
    if width == 0 || height == 0 {
        return String::new();
    }
    let legends = legend(view.labels, view.editor.is_some(), &view.menu_button);
    let footer = legends.len().min(height.saturating_sub(1));
    let mut lines = vec![];
    if height >= footer + 3 {
        lines.push(clip(&view.scope, width));
        lines.push(clip(&view.breadcrumb, width));
    }
    let available = height.saturating_sub(lines.len() + footer);
    let mut content = vec![];
    if let Some(editor) = &view.editor {
        let limit = if editor.field == crate::menu::Field::Branch {
            200
        } else {
            80
        };
        content.push(tail(&editor.buffer, width));
        content.push(format!("{}/{} chars", editor.buffer.chars().count(), limit));
        let cells = editor.cells();
        // The logical grid always has eight columns, matching TextEditor::input.
        // Narrow windows show a horizontal slice containing the selected cell.
        let cell_width = 7.min(width).max(1);
        let visible_columns = (width / cell_width).clamp(1, 8);
        let row_start = editor.selected / 8 * 8;
        let col_start = (editor.selected % 8).saturating_sub(visible_columns - 1);
        let mut row = String::new();
        for index in row_start + col_start
            ..(row_start + col_start + visible_columns)
                .min(cells.len())
                .min(row_start + 8)
        {
            let cell = if cells[index] == " " {
                "Space"
            } else {
                &cells[index]
            };
            let value = format!(
                "{}{}",
                if index == editor.selected { ">" } else { " " },
                cell
            );
            let clipped = clip(&value, cell_width);
            row.push_str(&clipped);
            row.push_str(
                &" ".repeat(cell_width.saturating_sub(UnicodeWidthStr::width(clipped.as_str()))),
            );
        }
        content.push(row);
        content.push(format!(
            "Selected: {} (8-column grid)",
            cells[editor.selected]
        ));
        content.push(editor.error.clone());
    } else if view.expanded {
        let wrapped = detail_lines(view, width);
        let start = view.scroll.min(wrapped.len().saturating_sub(1));
        content.extend(wrapped.into_iter().skip(start).take(available));
    } else if (view.rows.len() <= 1
        && !view.details.is_empty()
        && view.breadcrumb.ends_with("Logs"))
        || view.rows.is_empty()
    {
        if let Some(row) = view.rows.first() {
            content.push(format!("> {}", row.label));
        }
        let start = view.scroll.min(view.details.len().saturating_sub(1));
        content.extend(view.details.iter().skip(start).cloned());
    } else {
        let status_slots = usize::from(!view.status.is_empty() && available >= 2);
        let detail_slots = usize::from(!view.details.is_empty() && available >= 3);
        let row_height = available.saturating_sub(detail_slots + status_slots).max(1);
        let start = view.selected.saturating_sub(row_height.saturating_sub(1));
        for (index, row) in view.rows.iter().enumerate().skip(start).take(row_height) {
            content.push(format!(
                "{} {}{}",
                if index == view.selected { ">" } else { " " },
                row.label,
                if row.disabled.is_some() {
                    " (unavailable)"
                } else {
                    ""
                }
            ));
        }
        if detail_slots > 0 {
            content.push(view.details.join(" · "));
        }
    }
    if !view.status.is_empty() && !view.expanded && view.editor.is_none() && available >= 2 {
        content.truncate(available - 1);
        content.push(view.status.clone());
    }
    lines.extend(
        content
            .into_iter()
            .take(available)
            .map(|line| clip(&line, width)),
    );
    while lines.len() < height.saturating_sub(footer) {
        lines.push(String::new());
    }
    // If the viewport is too short for both editor lines, keep Cancel visible.
    lines.extend(
        legends
            .into_iter()
            .rev()
            .take(footer)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .map(|line| clip(&line, width)),
    );
    debug_assert!(lines
        .iter()
        .all(|line| UnicodeWidthStr::width(line.as_str()) <= width));
    lines.join("\n")
}
