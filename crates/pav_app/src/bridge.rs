//! The live agent bridge, game side: a TCP listener (`--bridge`, or `bridge = "ADDR"` in
//! `pavilion.toml`) whose requests run as agent tools on the simulation thread, against the
//! running game. Tools see the game's own camera and view settings, and changes they make come
//! back to it. Captures render on a separate headless device, so they work while you play.

use std::cell::RefCell;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use anyhow::Result;
use pav_core::Sim;
use pav_tools::Session;
use pav_tools::session::Gpu;
use pav_view::{CameraParams, CameraRig, ViewSettings};
use serde_json::{Value, json};

use crate::simhost::SimHost;

/// One request line and where its reply goes.
pub struct Request {
    pub msg: Value,
    pub reply: Sender<Value>,
}

/// Camera and view settings after a tool ran (the game adopts the ones that changed).
pub struct Changes {
    pub camera: Option<CameraParams>,
    pub view: Option<ViewSettings>,
}

pub struct Bridge {
    pub addr: String,
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
        Ok(Self { addr, requests, changes_tx, changes })
    }

    /// Adopts the camera/view changes earlier tools made, then hands waiting requests to the
    /// simulation thread. (A client waits for each reply, and changes are sent before replies,
    /// so the next request always sees the previous one's changes.)
    pub fn poll(&self, host: &SimHost, rig: &mut CameraRig, view: &mut ViewSettings) {
        for c in self.changes.try_iter() {
            if let Some(p) = c.camera {
                rig.params = p;
            }
            if let Some(v) = c.view {
                *view = v;
            }
        }
        while let Ok(req) = self.requests.try_recv() {
            let (rig, view, changes) = (rig.clone(), view.clone(), self.changes_tx.clone());
            host.exec(move |sim| {
                let live = std::mem::replace(sim, Sim::empty(1));
                let gpu = GPU.with(|g| g.borrow_mut().take());
                let (cam0, view0) = (rig.params.clone(), serde_json::to_value(&view).ok());
                let mut session = Session::from_live(live, rig, view, gpu);
                let reply = pav_tools::bridge::handle(&mut session, &req.msg);
                let (live, rig, view, gpu) = session.into_live();
                *sim = live;
                GPU.with(|g| *g.borrow_mut() = gpu);
                let _ = changes.send(Changes {
                    camera: (rig.params != cam0).then_some(rig.params),
                    view: (serde_json::to_value(&view).ok() != view0).then_some(view),
                });
                let _ = req.reply.send(reply);
            });
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
