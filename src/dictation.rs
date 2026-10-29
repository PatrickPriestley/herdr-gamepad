//! A retained voice draft and its delivery state. No I/O.
use crate::engine::{Pane, Snapshot, Status};
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeliveryState { Ready, Sending, Unknown(String) }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Draft {
    pub id: u64,
    pub text: String,
    pub target: Option<Pane>,
    pub state: DeliveryState,
    pub error: String,
}
impl Draft {
    pub fn validate_target(&self, snap: &Snapshot) -> Result<(), String> {
        let target = self.target.as_ref().ok_or("No agent was selected when recorded")?;
        let focused = snap.focused.as_ref().ok_or("Return to the original agent pane to send")?;
        if target.pane_id != focused.pane_id || target.workspace_id != focused.workspace_id
            || target.tab_id != focused.tab_id || target.agent.is_none() || target.agent != focused.agent {
            return Err("Return to the original agent pane to send".into());
        }
        if !matches!(focused.status, Some(Status::Idle | Status::Done | Status::Working)) {
            return Err("Agent unavailable or waiting at a dialog; draft kept".into());
        }
        Ok(())
    }
}
