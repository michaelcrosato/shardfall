//! The live agent bridge, game side: a TCP listener (`--bridge`, or `bridge = "ADDR"` in
//! `shardfall.toml`) whose requests run as agent tools on the simulation thread, against the
//! running game. Tools see the game's own camera and view settings, and changes they make come
//! back to it. Captures render on a separate headless device, so they work while you play.

use std::cell::{Cell, RefCell};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use anyhow::{Result, bail};
use pav_core::Sim;
use pav_tools::Session;
use pav_tools::session::Gpu;
use pav_view::look::Look;
use pav_view::{CameraParams, CameraRig, ViewSettings};
use serde_json::{Value, json};

use crate::simhost::SimHost;

/// One request line and where its reply goes.
pub struct Request {
    pub msg: Value,
    pub reply: Sender<Value>,
}

/// Camera, view settings and look after a tool ran (the game adopts the ones that changed).
pub struct Changes {
    pub camera: Option<CameraParams>,
    pub view: Option<ViewSettings>,
    pub look: Option<Look>,
    pub ticket: u64,
}

pub struct Bridge {
    pub addr: String,
    pub feedback: pav_tools::live_feedback::SharedFeedback,
    adopted_ticket: AtomicU64,
    in_flight: Cell<bool>,
    requests: Receiver<Request>,
    changes_tx: Sender<Changes>,
    changes: Receiver<Changes>,
}

thread_local! {
    /// The headless device for captures, kept on the simulation thread between requests.
    static GPU: RefCell<Option<Gpu>> = const { RefCell::new(None) };
}

impl Bridge {
    pub fn start(addr: &str) -> Result<Self> {
        let listener = TcpListener::bind(addr)?;
        let addr = listener.local_addr().map(|a| a.to_string()).unwrap_or_else(|_| addr.to_string());
        let (tx, requests) = channel();
        std::thread::Builder::new().name("bridge".into()).spawn(move || {
            for stream in listener.incoming().flatten() {
                let tx = tx.clone();
                let _ = std::thread::Builder::new().name("bridge client".into()).spawn(move || serve(stream, tx));
            }
        })?;
        let (changes_tx, changes) = channel();
        log::info!("live agent bridge listening on {addr}");
        Ok(Self {
            addr,
            feedback: pav_tools::live_feedback::LiveFeedback::shared(),
            adopted_ticket: AtomicU64::new(0),
            in_flight: Cell::new(false),
            requests,
            changes_tx,
            changes,
        })
    }

    /// Adopts the previous tool's camera/view changes before dispatching another request.
    /// Only one request can be in flight, including when several clients have queued work.
    /// Returns the look when a tool changed it.
    pub fn poll(&self, host: &SimHost, rig: &mut CameraRig, view: &mut ViewSettings, look: &Look) -> Option<Look> {
        let new_look = self.adopt_changes(rig, view);
        if self.in_flight.get() {
            return new_look;
        }
        if let Ok(req) = self.requests.try_recv() {
            self.in_flight.set(true);
            let (rig, view, changes) = (rig.clone(), view.clone(), self.changes_tx.clone());
            let look = new_look.clone().unwrap_or_else(|| look.clone());
            let feedback = self.feedback.clone();
            host.exec(move |sim| {
                let live = std::mem::replace(sim, Sim::empty(1));
                let gpu = GPU.with(|g| g.borrow_mut().take());
                let (cam0, view0) = (rig.params.clone(), serde_json::to_value(&view).ok());
                let look0 = serde_json::to_value(&look).ok();
                let mut session = Session::from_live(live, rig, view, gpu);
                session.look = look;
                session.feedback = Some(feedback);
                let reply = pav_tools::bridge::handle(&mut session, &req.msg);
                let look = std::mem::take(&mut session.look);
                let (live, rig, view, gpu) = session.into_live();
                *sim = live;
                GPU.with(|g| *g.borrow_mut() = gpu);
                let _ = changes.send(Changes {
                    camera: (rig.params != cam0).then_some(rig.params),
                    view: (serde_json::to_value(&view).ok() != view0).then_some(view),
                    look: (serde_json::to_value(&look).ok() != look0).then_some(look),
                    ticket: sim.live_edit_ticket,
                });
                let _ = req.reply.send(reply);
            });
        }
        new_look
    }

    /// Before a direct UI tool captures its camera/view, finish adopting an earlier bridge
    /// request. Queued requests stay queued until the next poll. The caller must skip its
    /// mutation on error: the earlier request may still be running on the simulation thread.
    pub fn synchronize(&self, host: &SimHost, rig: &mut CameraRig, view: &mut ViewSettings) -> Result<Option<Look>> {
        let mut new_look = self.adopt_changes(rig, view);
        if self.in_flight.get() {
            // A barrier is needed only when a dispatched request has not returned its changes.
            // It does not wait for a window acknowledgement, so the simulation cannot deadlock on us.
            let _ = host.query(|_| ());
            if let Some(look) = self.adopt_changes(rig, view) {
                new_look = Some(look);
            }
            if self.in_flight.get() {
                bail!("the previous bridge request has not finished; try the studio command again");
            }
        }
        Ok(new_look)
    }

    fn adopt_changes(&self, rig: &mut CameraRig, view: &mut ViewSettings) -> Option<Look> {
        let mut new_look = None;
        for c in self.changes.try_iter() {
            if let Some(p) = c.camera {
                rig.params = p;
            }
            if let Some(v) = c.view {
                *view = v;
            }
            if let Some(l) = c.look {
                new_look = Some(l);
            }
            self.adopted(c.ticket);
            self.in_flight.set(false);
        }
        new_look
    }

    /// The camera changes for this edit have reached the window too.
    pub fn adopted(&self, ticket: u64) {
        self.adopted_ticket.fetch_max(ticket, Ordering::Relaxed);
    }

    pub fn submitted(&self, ticket: u64) {
        // A sim frame can arrive between draining camera changes and drawing. Wait for
        // a frame whose matching camera was adopted, rather than confirming too early.
        if ticket == self.adopted_ticket.load(Ordering::Relaxed) {
            if let Ok(mut feedback) = self.feedback.lock() {
                feedback.submitted(ticket);
            }
        }
    }
}

/// One connection: a request per line, answered in order.
fn serve(stream: TcpStream, tx: Sender<Request>) {
    let Ok(mut out) = stream.try_clone() else { return };
    for line in BufReader::new(stream).lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let reply = match serde_json::from_str::<Value>(&line) {
            Ok(msg) => {
                let (rtx, rrx) = channel();
                if tx.send(Request { msg, reply: rtx }).is_err() {
                    break;
                }
                rrx.recv_timeout(Duration::from_secs(120)).unwrap_or_else(|_| json!({ "error": "the game did not answer" }))
            }
            Err(e) => json!({ "error": format!("bad request: {e}") }),
        };
        if writeln!(out, "{reply}").and_then(|_| out.flush()).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn connected() -> (Bridge, Sender<Request>) {
        let (tx, requests) = channel();
        let (changes_tx, changes) = channel();
        (
            Bridge {
                addr: "test".into(),
                feedback: pav_tools::live_feedback::LiveFeedback::shared(),
                adopted_ticket: AtomicU64::new(0),
                in_flight: Cell::new(false),
                requests,
                changes_tx,
                changes,
            },
            tx,
        )
    }

    fn request(tx: &Sender<Request>, tool: &str, args: Value) -> Receiver<Value> {
        let (reply, rx) = channel();
        tx.send(Request { msg: json!({"tool":tool,"args":args}), reply }).unwrap();
        rx
    }

    #[test]
    fn queued_requests_use_the_preceding_adopted_camera() {
        let host = SimHost::start(Sim::empty(1));
        host.set_control(|c| c.paused = true);
        let (bridge, tx) = connected();
        let (mut rig, mut view, look) = (CameraRig::default(), ViewSettings::default(), Look::default());
        let first = request(&tx, "set", json!({"path":"camera.yaw","value":47}));
        let second = request(&tx, "set", json!({"path":"camera.distance","value":27}));
        bridge.poll(&host, &mut rig, &mut view, &look);
        host.query(|_| ()).unwrap();
        assert!(first.recv_timeout(Duration::from_secs(5)).unwrap().get("error").is_none());
        assert!(matches!(second.try_recv(), Err(std::sync::mpsc::TryRecvError::Empty)));
        assert_eq!(rig.params.yaw, 0.0, "the window has not adopted the first result yet");

        bridge.poll(&host, &mut rig, &mut view, &look);
        assert_eq!(rig.params.yaw, 47.0);
        bridge.synchronize(&host, &mut rig, &mut view).unwrap();
        assert!(second.recv_timeout(Duration::from_secs(5)).unwrap().get("error").is_none());
        assert_eq!(rig.params.yaw, 47.0, "the second request must preserve the first request's yaw");
        assert_eq!(rig.params.distance, 27.0);
        assert!(!bridge.in_flight.get());
    }

    #[test]
    fn synchronized_manual_switch_cannot_be_overwritten_by_an_earlier_bridge_edit() {
        let host = SimHost::start(Sim::empty(1));
        host.set_control(|c| c.paused = true);
        let (bridge, tx) = connected();
        let (mut rig, mut view, look) = (CameraRig::default(), ViewSettings::default(), Look::default());
        let world_camera = rig.params.clone();
        let opened = request(&tx, "asset_preview", json!({"name":"BUILTIN/bench","playing":false}));
        bridge.poll(&host, &mut rig, &mut view, &look);
        bridge.synchronize(&host, &mut rig, &mut view).unwrap();
        let reply = opened.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(reply.get("error").is_none(), "{reply}");
        let first_ticket = reply["result"]["feedback"]["ticket"].as_u64().unwrap();
        let prop_camera = rig.params.clone();
        assert_ne!(prop_camera, world_camera);

        // The app clones its session inputs only after synchronize has adopted the bridge result.
        let (manual_rig, manual_view, feedback) = (rig.clone(), view.clone(), bridge.feedback.clone());
        let (manual_rig, manual_view, ticket, saved_prop) = host
            .query(move |sim| {
                let live = std::mem::replace(sim, Sim::empty(1));
                let mut session = Session::from_live(live, manual_rig, manual_view, None);
                session.feedback = Some(feedback);
                let reply = pav_tools::bridge::handle(&mut session, &json!({"tool":"asset_preview","args":{"action":"close"}}));
                assert!(reply.get("error").is_none(), "{reply}");
                let (live, rig, view, _) = session.into_live();
                let saved_prop = live.state.studio_cameras.prop.as_ref().unwrap().params.clone();
                let ticket = live.live_edit_ticket;
                *sim = live;
                (rig, view, ticket, saved_prop)
            })
            .unwrap();
        rig = manual_rig;
        view = manual_view;
        bridge.adopted(ticket);
        assert!(ticket > first_ticket);
        assert_eq!(saved_prop, serde_json::to_value(prop_camera).unwrap());
        bridge.poll(&host, &mut rig, &mut view, &look);
        assert_eq!(rig.params, world_camera, "no delayed prop camera may overwrite the manual close");
        assert_eq!(bridge.adopted_ticket.load(Ordering::Relaxed), ticket);

        bridge.submitted(ticket);
        let status = request(&tx, "studio_status", json!({"ticket":ticket}));
        bridge.poll(&host, &mut rig, &mut view, &look);
        bridge.synchronize(&host, &mut rig, &mut view).unwrap();
        let status = status.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(status["result"]["feedback"]["state"], "submitted");
    }
}
