//! Runs the simulation on its own thread at a fixed tick rate. The renderer never waits for
//! it: it reads the two most recent frames and interpolates between them.

use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use pav_core::{InputFrame, RenderFrame, Sim};

type Job = Box<dyn FnOnce(&mut Sim) + Send>;

enum Cmd {
    Run(Job),
    Quit,
}

#[derive(Clone, Copy, Debug)]
pub struct TimeControl {
    pub paused: bool,
    /// Simulation speed multiplier (0.05 .. 8).
    pub speed: f32,
    /// Ticks to advance while paused.
    pub step_requests: u32,
}

impl Default for TimeControl {
    fn default() -> Self {
        Self { paused: false, speed: 1.0, step_requests: 0 }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SimStats {
    /// Measured ticks per second of wall time.
    pub tps: f32,
    /// Average CPU time per tick (ms).
    pub tick_ms: f32,
    pub tick: u64,
    pub entities: usize,
}

pub struct FramePair {
    pub prev: Arc<RenderFrame>,
    pub curr: Arc<RenderFrame>,
    /// When `curr` became current (wall clock).
    pub curr_at: Instant,
    /// Wall-clock length of one tick at the current speed.
    pub tick_wall: f32,
}

pub struct Shared {
    pub frames: Mutex<FramePair>,
    pub input: Mutex<InputFrame>,
    pub control: Mutex<TimeControl>,
    pub stats: Mutex<SimStats>,
    pub crashed: Mutex<Option<String>>,
}

pub struct SimHost {
    tx: Sender<Cmd>,
    pub shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl SimHost {
    pub fn start(sim: Sim) -> Self {
        let f = Arc::new(sim.frame());
        let shared = Arc::new(Shared {
            frames: Mutex::new(FramePair { prev: f.clone(), curr: f, curr_at: Instant::now(), tick_wall: sim.dt() }),
            input: Mutex::new(InputFrame::default()),
            control: Mutex::new(TimeControl::default()),
            stats: Mutex::new(SimStats::default()),
            crashed: Mutex::new(None),
        });
        let (tx, rx) = channel();
        let sh = shared.clone();
        let thread = std::thread::Builder::new()
            .name("simulation".into())
            .spawn(move || {
                let sh2 = sh.clone();
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || run(sim, rx, sh2)));
                if let Err(e) = r {
                    let msg = e
                        .downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .or_else(|| e.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "unknown panic".into());
                    *sh.crashed.lock().unwrap() = Some(msg);
                }
            })
            .expect("spawn simulation thread");
        Self { tx, shared, thread: Some(thread) }
    }

    /// Runs `f` on the simulation thread (fire and forget).
    pub fn exec(&self, f: impl FnOnce(&mut Sim) + Send + 'static) {
        let _ = self.tx.send(Cmd::Run(Box::new(f)));
    }

    /// Runs `f` on the simulation thread and waits for its result.
    pub fn query<R: Send + 'static>(&self, f: impl FnOnce(&mut Sim) -> R + Send + 'static) -> Option<R> {
        let (rtx, rrx) = channel();
        self.exec(move |sim| {
            let _ = rtx.send(f(sim));
        });
        rrx.recv_timeout(Duration::from_secs(5)).ok()
    }

    pub fn frames(&self) -> (Arc<RenderFrame>, Arc<RenderFrame>, Instant, f32) {
        let f = self.shared.frames.lock().unwrap();
        (f.prev.clone(), f.curr.clone(), f.curr_at, f.tick_wall)
    }

    pub fn control(&self) -> TimeControl {
        *self.shared.control.lock().unwrap()
    }

    pub fn set_control(&self, f: impl FnOnce(&mut TimeControl)) {
        f(&mut self.shared.control.lock().unwrap());
    }
}

impl Drop for SimHost {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn publish(sim: &Sim, sh: &Shared, tick_wall: f32, at: Instant) {
    let frame = Arc::new(sim.frame());
    let mut f = sh.frames.lock().unwrap();
    f.prev = std::mem::replace(&mut f.curr, frame);
    f.curr_at = at;
    f.tick_wall = tick_wall;
}

fn run(mut sim: Sim, rx: Receiver<Cmd>, sh: Arc<Shared>) {
    let mut next = Instant::now();
    let mut window_start = Instant::now();
    let mut window_ticks = 0u32;
    let mut busy = Duration::ZERO;
    loop {
        // Commands first.
        loop {
            match rx.try_recv() {
                Ok(Cmd::Run(job)) => {
                    job(&mut sim);
                    publish(&sim, &sh, sim.dt(), Instant::now());
                }
                Ok(Cmd::Quit) => return,
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return,
            }
        }

        let ctl = *sh.control.lock().unwrap();
        let tick_wall = sim.dt() / ctl.speed.clamp(0.01, 16.0);
        let mut tick_once = |sim: &mut Sim, at: Instant| {
            let input = {
                let mut i = sh.input.lock().unwrap();
                let frame = *i;
                i.pressed = 0;
                frame
            };
            let t = Instant::now();
            sim.step(&input);
            busy += t.elapsed();
            window_ticks += 1;
            publish(sim, &sh, tick_wall, at);
            sim.drain_events();
        };

        let now = Instant::now();
        if ctl.paused {
            if ctl.step_requests > 0 {
                sh.control.lock().unwrap().step_requests -= 1;
                tick_once(&mut sim, now);
            }
            next = now;
        } else {
            let mut n = 0;
            while now >= next && n < 8 {
                next += Duration::from_secs_f32(tick_wall);
                tick_once(&mut sim, next);
                n += 1;
            }
            if n == 8 && now > next {
                next = now; // too far behind: drop time instead of spiralling
            }
        }

        let elapsed = window_start.elapsed();
        if elapsed >= Duration::from_millis(500) {
            let mut s = sh.stats.lock().unwrap();
            s.tps = window_ticks as f32 / elapsed.as_secs_f32();
            s.tick_ms = if window_ticks > 0 { busy.as_secs_f32() * 1000.0 / window_ticks as f32 } else { 0.0 };
            s.tick = sim.state.tick;
            s.entities = sim.state.entities.len();
            window_start = Instant::now();
            window_ticks = 0;
            busy = Duration::ZERO;
        }

        let wait = if ctl.paused { Duration::from_millis(4) } else { next.saturating_duration_since(Instant::now()) };
        match rx.recv_timeout(wait) {
            Ok(Cmd::Run(job)) => {
                job(&mut sim);
                publish(&sim, &sh, sim.dt(), Instant::now());
            }
            Ok(Cmd::Quit) => return,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}
