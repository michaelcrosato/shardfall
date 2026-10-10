//! Runs the simulation at a fixed tick rate: on its own thread natively, and stepped from the
//! frame loop in the browser (no threads there). The renderer never waits for it: it reads the
//! two most recent frames and interpolates between them.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use pav_core::{InputFrame, RenderFrame, Sim, SimEvent};
use web_time::Instant;

#[cfg(not(target_arch = "wasm32"))]
type Job = Box<dyn FnOnce(&mut Sim) + Send>;

#[cfg(not(target_arch = "wasm32"))]
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
    /// Holding the rewind button.
    pub rewinding: bool,
    /// Ticks travelled back per tick while rewinding.
    pub rewind_speed: u32,
    /// Jump to this tick (timeline scrubbing), keeping the future until play resumes.
    pub scrub: Option<u64>,
}

impl Default for TimeControl {
    fn default() -> Self {
        Self { paused: false, speed: 1.0, step_requests: 0, rewinding: false, rewind_speed: 1, scrub: None }
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
    /// Rewind range available (ticks).
    pub oldest: u64,
    pub newest: u64,
    pub history_mb: f32,
    pub rewound: bool,
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
    /// Events for sound and effects, drained by the render thread.
    pub events: Mutex<Vec<SimEvent>>,
    /// When the input that is waiting for the next tick was pressed (latency measurement).
    pub input_stamp: Mutex<Option<Instant>>,
    /// (pressed, consumed by a tick, that tick's number) for the latest measured press.
    pub latency_probe: Mutex<Option<(Instant, Instant, u64)>>,
    /// Menu commands waiting for a tick (one rides along with each tick's input).
    pub cmds: Mutex<std::collections::VecDeque<pav_core::arpg::GameCmd>>,
}

impl Shared {
    /// Queues a game command for the next free tick.
    pub fn command(&self, c: pav_core::arpg::GameCmd) {
        let mut q = self.cmds.lock().unwrap();
        if q.len() < 64 {
            q.push_back(c);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub struct SimHost {
    tx: std::sync::mpsc::Sender<Cmd>,
    pub shared: Arc<Shared>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// In the browser the simulation lives on the main thread and `pump` advances it every frame.
#[cfg(target_arch = "wasm32")]
pub struct SimHost {
    sim: std::cell::RefCell<Sim>,
    clock: std::cell::RefCell<Clock>,
    pub shared: Arc<Shared>,
}

fn shared_for(sim: &mut Sim) -> Arc<Shared> {
    let f = Arc::new(sim.frame());
    Arc::new(Shared {
        frames: Mutex::new(FramePair { prev: f.clone(), curr: f, curr_at: Instant::now(), tick_wall: sim.dt() }),
        input: Mutex::new(InputFrame::default()),
        control: Mutex::new(TimeControl::default()),
        stats: Mutex::new(SimStats::default()),
        crashed: Mutex::new(None),
        events: Mutex::new(Vec::new()),
        input_stamp: Mutex::new(None),
        latency_probe: Mutex::new(None),
        cmds: Mutex::new(Default::default()),
    })
}

#[cfg(not(target_arch = "wasm32"))]
impl SimHost {
    pub fn start(mut sim: Sim) -> Self {
        let shared = shared_for(&mut sim);
        let (tx, rx) = std::sync::mpsc::channel();
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
        let (rtx, rrx) = std::sync::mpsc::channel();
        self.exec(move |sim| {
            let _ = rtx.send(f(sim));
        });
        rrx.recv_timeout(Duration::from_secs(5)).ok()
    }

    /// Advances the simulation (the thread does this by itself natively).
    pub fn pump(&self) {}
}

#[cfg(target_arch = "wasm32")]
impl SimHost {
    pub fn start(mut sim: Sim) -> Self {
        let shared = shared_for(&mut sim);
        Self { sim: std::cell::RefCell::new(sim), clock: std::cell::RefCell::new(Clock::new()), shared }
    }

    pub fn exec(&self, f: impl FnOnce(&mut Sim) + 'static) {
        let mut sim = self.sim.borrow_mut();
        f(&mut sim);
        let dt = sim.dt();
        publish(&mut sim, &self.shared, dt, Instant::now());
    }

    pub fn query<R: 'static>(&self, f: impl FnOnce(&mut Sim) -> R + 'static) -> Option<R> {
        let mut sim = self.sim.borrow_mut();
        let r = f(&mut sim);
        let dt = sim.dt();
        publish(&mut sim, &self.shared, dt, Instant::now());
        Some(r)
    }

    /// Runs the ticks that are due (called once per frame).
    pub fn pump(&self) {
        self.clock.borrow_mut().advance(&mut self.sim.borrow_mut(), &self.shared);
    }
}

impl SimHost {
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

#[cfg(not(target_arch = "wasm32"))]
impl Drop for SimHost {
    fn drop(&mut self) {
        let _ = self.tx.send(Cmd::Quit);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn publish(sim: &mut Sim, sh: &Shared, tick_wall: f32, at: Instant) {
    let events = sim.drain_events();
    if !events.is_empty() {
        let mut q = sh.events.lock().unwrap();
        q.extend(events);
        let n = q.len();
        if n > 512 {
            q.drain(..n - 512);
        }
    }
    let frame = Arc::new(sim.frame());
    let mut f = sh.frames.lock().unwrap();
    f.prev = std::mem::replace(&mut f.curr, frame);
    f.curr_at = at;
    f.tick_wall = tick_wall;
}

/// Fixed-rate ticking with time controls (pause, single steps, speed, rewind, scrubbing).
struct Clock {
    next: Instant,
    window_start: Instant,
    window_ticks: u32,
    busy: Duration,
    rewound: bool,
    previewing: bool,
}

impl Clock {
    fn new() -> Self {
        Self {
            next: Instant::now(),
            window_start: Instant::now(),
            window_ticks: 0,
            busy: Duration::ZERO,
            rewound: false,
            previewing: false,
        }
    }

    /// Runs whatever is due now; returns how long until the next tick.
    fn advance(&mut self, sim: &mut Sim, sh: &Shared) -> Duration {
        let previewing = sim.state.animation_preview.is_some();
        if self.previewing != previewing {
            self.previewing = previewing;
            self.next = Instant::now();
        }
        let ctl = *sh.control.lock().unwrap();
        // The animation stage has its own play, pause and speed controls. The world's time
        // controls stay untouched, including a paused or rewound game behind the stage.
        let tick_wall = if previewing { sim.dt() } else { sim.dt() / ctl.speed.clamp(0.01, 16.0) };
        let (busy, window_ticks) = (&mut self.busy, &mut self.window_ticks);
        let mut tick_once = |sim: &mut Sim, at: Instant| {
            let input = if previewing {
                InputFrame::default()
            } else {
                let mut i = sh.input.lock().unwrap();
                let mut frame = *i;
                i.pressed = 0;
                frame.cmd = sh.cmds.lock().unwrap().pop_front();
                frame
            };
            let stamp = if previewing { None } else { sh.input_stamp.lock().unwrap().take() };
            let t = Instant::now();
            sim.step(&input);
            *busy += t.elapsed();
            if let Some(s) = stamp {
                *sh.latency_probe.lock().unwrap() = Some((s, Instant::now(), sim.state.tick));
            }
            *window_ticks += 1;
            publish(sim, sh, tick_wall, at);
        };

        let now = Instant::now();
        if previewing {
            let mut n = 0;
            while now >= self.next && n < 8 {
                self.next += Duration::from_secs_f32(tick_wall);
                tick_once(sim, self.next);
                n += 1;
            }
            if n == 8 && now > self.next {
                self.next = now;
            }
        } else if ctl.rewinding {
            if now >= self.next {
                self.next = now + Duration::from_secs_f32(sim.dt());
                let oldest = sim.history.oldest_tick().unwrap_or(sim.state.tick);
                let target = sim.state.tick.saturating_sub(ctl.rewind_speed.max(1) as u64).max(oldest);
                if target < sim.state.tick && sim.rewind_to(target) {
                    self.rewound = true;
                }
                let dt = sim.dt();
                publish(sim, sh, dt, now);
            }
        } else if let Some(t) = ctl.scrub {
            sh.control.lock().unwrap().scrub = None;
            if sim.rewind_to(t) {
                self.rewound = true;
            }
            let dt = sim.dt();
            publish(sim, sh, dt, now);
            self.next = now;
        } else if ctl.paused {
            if ctl.step_requests > 0 {
                sh.control.lock().unwrap().step_requests -= 1;
                if self.rewound {
                    sim.commit_rewind();
                    self.rewound = false;
                }
                tick_once(sim, now);
            }
            self.next = now;
        } else {
            if self.rewound {
                // Acting after a rewind starts a new timeline.
                sim.commit_rewind();
                self.rewound = false;
                self.next = now;
            }
            let mut n = 0;
            while now >= self.next && n < 8 {
                self.next += Duration::from_secs_f32(tick_wall);
                tick_once(sim, self.next);
                n += 1;
            }
            if n == 8 && now > self.next {
                self.next = now; // too far behind: drop time instead of spiralling
            }
        }

        let elapsed = self.window_start.elapsed();
        if elapsed >= Duration::from_millis(500) {
            let mut s = sh.stats.lock().unwrap();
            s.tps = self.window_ticks as f32 / elapsed.as_secs_f32();
            s.tick_ms = if self.window_ticks > 0 { self.busy.as_secs_f32() * 1000.0 / self.window_ticks as f32 } else { 0.0 };
            s.tick = sim.state.tick;
            s.entities = sim.state.entities.len();
            s.oldest = sim.history.oldest_tick().unwrap_or(sim.state.tick);
            s.newest = sim.history.newest_tick().unwrap_or(sim.state.tick).max(sim.state.tick);
            s.history_mb = sim.history.approx_bytes() as f32 / 1.0e6;
            s.rewound = self.rewound;
            self.window_start = Instant::now();
            self.window_ticks = 0;
            self.busy = Duration::ZERO;
        }
        if ctl.paused && !previewing { Duration::from_millis(4) } else { self.next.saturating_duration_since(Instant::now()) }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn run(mut sim: Sim, rx: std::sync::mpsc::Receiver<Cmd>, sh: Arc<Shared>) {
    use std::sync::mpsc::{RecvTimeoutError, TryRecvError};
    let mut clock = Clock::new();
    loop {
        // Commands first.
        loop {
            match rx.try_recv() {
                Ok(Cmd::Run(job)) => {
                    job(&mut sim);
                    let dt = sim.dt();
                    publish(&mut sim, &sh, dt, Instant::now());
                }
                Ok(Cmd::Quit) => return,
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => return,
            }
        }
        let wait = clock.advance(&mut sim, &sh);
        match rx.recv_timeout(wait) {
            Ok(Cmd::Run(job)) => {
                job(&mut sim);
                let dt = sim.dt();
                publish(&mut sim, &sh, dt, Instant::now());
            }
            Ok(Cmd::Quit) => return,
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_runs_over_a_paused_world_without_changing_its_time_controls() {
        let mut sim = Sim::empty(1);
        let shared = shared_for(&mut sim);
        {
            let mut control = shared.control.lock().unwrap();
            control.paused = true;
            control.speed = 8.0;
        }
        let mut clock = Clock::new();
        clock.rewound = true;
        sim.preview_open(Some("QUATERNIUS/Idle_Loop"), None).unwrap();
        clock.advance(&mut sim, &shared);
        let time = sim.preview_info().unwrap().time;
        assert!(time >= sim.dt() && time < sim.dt() * 2.0);
        assert!(clock.rewound, "opening a preview must not commit the world's rewind");
        assert_eq!(sim.state.tick, 0);
        let control = *shared.control.lock().unwrap();
        assert!(control.paused);
        assert_eq!(control.speed, 8.0);
        sim.preview_close();
        clock.advance(&mut sim, &shared);
        assert_eq!(sim.state.tick, 0, "the paused world stays paused after closing");
        assert!(clock.rewound);
    }
}
