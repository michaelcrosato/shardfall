//! Small acknowledgements for live studio edits. A reply means the edit was accepted;
//! submission is recorded later by the window, never waited for on the simulation thread.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Result, anyhow, bail};
use serde_json::{Value, json};

use crate::Session;
use crate::tools::{Args, Output};

pub type SharedFeedback = Arc<Mutex<LiveFeedback>>;

struct Edit {
    ticket: u64,
    tool: String,
    started: Instant,
    apply_ms: f64,
    submitted_ms: Option<f64>,
    superseded_by: Option<u64>,
}

#[derive(Default)]
pub struct LiveFeedback {
    next: u64,
    submitted: u64,
    edits: VecDeque<Edit>,
}

impl LiveFeedback {
    /// This state belongs to the live connection, outside snapshots and rewinds.
    pub fn shared() -> SharedFeedback {
        Arc::new(Mutex::new(Self::default()))
    }

    pub fn accepted(&mut self, tool: &str, started: Instant) -> Value {
        self.next += 1;
        let apply_ms = started.elapsed().as_secs_f64() * 1000.0;
        self.edits.push_back(Edit {
            ticket: self.next,
            tool: tool.into(),
            started,
            apply_ms,
            submitted_ms: None,
            superseded_by: None,
        });
        while self.edits.len() > 64 {
            self.edits.pop_front();
        }
        json!({"ticket":self.next,"state":"pending","apply_ms":apply_ms,"status_tool":"studio_status"})
    }

    /// Called only after a successful surface submission. Earlier edits that did not
    /// reach a submitted frame are reported as superseded, not as separately displayed.
    pub fn submitted(&mut self, ticket: u64) {
        if ticket == 0 || ticket <= self.submitted {
            return;
        }
        self.submitted = ticket;
        for edit in &mut self.edits {
            if edit.submitted_ms.is_some() || edit.superseded_by.is_some() {
                continue;
            }
            if edit.ticket == ticket {
                edit.submitted_ms = Some(edit.started.elapsed().as_secs_f64() * 1000.0);
            } else if edit.ticket < ticket {
                edit.superseded_by = Some(ticket);
            }
        }
    }

    fn status(&self, ticket: Option<u64>) -> Value {
        let ticket = ticket.unwrap_or(self.next);
        let edit = self.edits.iter().find(|e| e.ticket == ticket);
        json!({
            "available":true,"latest_ticket":self.next,"submitted_ticket":self.submitted,
            "ticket":ticket,
            "state": match edit {
                Some(e) if e.submitted_ms.is_some() => "submitted",
                Some(e) if e.superseded_by.is_some() => "superseded",
                Some(_) => "pending",
                None if ticket == 0 => "idle",
                None => "unknown",
            },
            "tool":edit.map(|e| &e.tool),
            "apply_ms":edit.map(|e| e.apply_ms),
            "submitted_ms":edit.and_then(|e| e.submitted_ms),
            "superseded_by":edit.and_then(|e| e.superseded_by),
            "measurement":"tool start to frame submission; excludes transport and physical display latency"
        })
    }
}

pub(crate) fn changes_studio(name: &str, args: &Args) -> bool {
    let action = args.get("action").and_then(Value::as_str).unwrap_or("");
    match name {
        "anim_edit" | "asset_edit" => !matches!(action, "" | "inspect"),
        "anim_preview" | "asset_preview" => {
            !matches!(action, "" | "status" | "pose")
                || args.iter().any(|(key, value)| {
                    if key == "close" {
                        value == true || value == "true"
                    } else {
                        !matches!(key.as_str(), "action" | "scene" | "seed" | "ticks")
                    }
                })
        }
        "asset_spawn" => matches!(action, "add" | "update" | "remove"),
        _ => false,
    }
}

pub fn t_studio_status(s: &mut Session, a: &Args) -> Result<Output> {
    for key in a.keys() {
        if !matches!(key.as_str(), "ticket" | "scene" | "seed" | "ticks") {
            bail!("unknown studio_status argument '{key}' (use ticket)");
        }
    }
    let ticket =
        a.get("ticket").map(|v| v.as_u64().ok_or_else(|| anyhow!("ticket must be a non-negative integer"))).transpose()?;
    let feedback = match &s.feedback {
        Some(shared) => shared.lock().map_err(|_| anyhow!("live feedback is unavailable"))?.status(ticket),
        None => json!({"available":false,"state":"headless","connect":"pav mcp --live"}),
    };
    let frame = s.sim.frame();
    Ok(Output::Json(json!({
        "mode":frame.studio_mode(),"world_tick":s.sim.state.tick,"feedback":feedback,
        "animation":frame.animation_preview,"asset":frame.prop_preview
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acknowledgements_follow_frames_not_repeated_content_or_paused_ticks() {
        let mut f = LiveFeedback::default();
        assert_eq!(f.accepted("asset_edit", Instant::now())["ticket"], 1);
        assert_eq!(f.accepted("asset_edit", Instant::now())["ticket"], 2);
        f.submitted(2);
        assert_eq!(f.status(Some(1))["state"], "superseded");
        assert_eq!(f.status(Some(2))["state"], "submitted");
        assert_eq!(f.accepted("asset_edit undo", Instant::now())["ticket"], 3);
        assert_eq!(f.status(Some(3))["state"], "pending");
        f.submitted(2);
        assert_eq!(f.status(Some(3))["state"], "pending");
        f.submitted(3);
        assert_eq!(f.status(Some(3))["state"], "submitted");
    }
}
