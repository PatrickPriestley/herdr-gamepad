//! Native composer input preparation and original-target validation. No I/O.
use crate::engine::{Pane, Snapshot, Status};

pub fn validate_target(target: &Pane, snap: &Snapshot) -> Result<(), String> {
    if target.agent.is_none() {return Err("No agent was selected when recorded".into());}
    let focused = snap.focused.as_ref().ok_or("Return to the original agent pane")?;
    if target.pane_id != focused.pane_id || target.workspace_id != focused.workspace_id
        || target.tab_id != focused.tab_id || target.agent != focused.agent {
        return Err("Return to the original agent pane".into());
    }
    if !matches!(focused.status, Some(Status::Idle | Status::Done | Status::Working)) {
        return Err("Agent unavailable or waiting at a dialog".into());
    }
    Ok(())
}

/// `pane send-text` writes literal bytes: no line breaks or terminal control bytes may reach it.
pub fn native_text(text: &str) -> String {
    let safe: String = text.chars().filter_map(|c| {
        if c.is_whitespace() { Some(' ') } else if c.is_control() { None } else { Some(c) }
    }).collect();
    safe.split_whitespace().collect::<Vec<_>>().join(" ")
}
