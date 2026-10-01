//! Room framework on the app side: info card + control guide on entering a room, room camera
//! defaults, the teleport menu, and hot reload of room files.

use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use egui::RichText;
use pav_core::frame::RoomInfo;
use pav_core::params;
use pav_core::room::Device as RoomDevice;
use pav_view::{CameraParams, CameraRig};

use crate::input::{Device, guide};

/// One entry of the teleport menu.
#[derive(Clone, Debug)]
pub struct RoomEntry {
    pub key: String,
    pub name: String,
    pub wing: String,
    pub about: String,
}

pub enum TeleportTarget {
    Room(String),
    Hub,
    Wilderness,
}

#[derive(Default)]
pub struct RoomHud {
    pub current: Option<RoomInfo>,
    pub card_until: Option<Instant>,
    saved_camera: Option<CameraParams>,
    pub teleport_open: bool,
    pub entries: Vec<RoomEntry>,
    pub errors: Vec<String>,
}

impl RoomHud {
    /// Call every frame with the room from the latest simulation frame. Applies camera
    /// defaults on enter and restores the camera on exit.
    pub fn update(&mut self, room: &Option<RoomInfo>, rig: &mut CameraRig) {
        let now_id = room.as_ref().map(|r| (r.id, r.key.clone()));
        let prev_id = self.current.as_ref().map(|r| (r.id, r.key.clone()));
        if now_id == prev_id {
            if let (Some(c), Some(r)) = (&mut self.current, room) {
                c.def = r.def.clone(); // hot reload keeps the card fresh
            }
            return;
        }
        if prev_id.is_some() {
            if let Some(saved) = self.saved_camera.take() {
                rig.blend_from_current(0.6);
                rig.params = saved;
            }
        }
        if let Some(r) = room {
            if !r.def.camera.is_empty() {
                // Room cameras are authored for the room's own orientation: turn with it.
                self.saved_camera = Some(rig.params.clone());
                rig.blend_from_current(0.6);
                let mut cam = r.def.camera.clone();
                let yaw = cam.get("yaw").and_then(|v| v.as_f64()).unwrap_or(0.0) + r.quarters as f64 * 90.0;
                cam.insert("yaw".into(), params::ParamValue::Float((yaw + 540.0).rem_euclid(360.0) - 180.0));
                params::apply_map(&mut rig.params, &cam);
            }
            self.card_until = Some(Instant::now() + Duration::from_secs(14));
            log::info!("entered room '{}'", r.key);
        }
        self.current = room.clone();
    }

    pub fn card_visible(&self) -> bool {
        self.current.is_some() && self.card_until.is_some_and(|t| Instant::now() < t)
    }

    pub fn show_card(&mut self) {
        self.card_until = Some(Instant::now() + Duration::from_secs(30));
    }

    /// Info card + control guide for the room (top-left).
    pub fn card(&mut self, ctx: &egui::Context, last_device: Device) {
        let Some(room) = &self.current else { return };
        let def = &room.def;
        let mut open = true;
        egui::Window::new(if def.name.is_empty() { room.key.clone() } else { def.name.clone() })
            .id(egui::Id::new("room_card"))
            .open(&mut open)
            .anchor(egui::Align2::LEFT_TOP, [12.0, 12.0])
            .resizable(false)
            .collapsible(false)
            .default_width(380.0)
            .show(ctx, |ui| {
                if !def.about.is_empty() {
                    ui.label(&def.about);
                }
                if !def.try_list.is_empty() {
                    ui.add_space(4.0);
                    ui.label(RichText::new("Try").strong());
                    for t in &def.try_list {
                        ui.label(format!("• {t}"));
                    }
                }
                let primary = match def.primary_device {
                    RoomDevice::KeyboardMouse => Device::KeyboardMouse,
                    RoomDevice::Gamepad => Device::Gamepad,
                };
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Controls").strong());
                    ui.label(
                        RichText::new(format!(
                            "designed for {}",
                            if primary == Device::Gamepad { "gamepad" } else { "keyboard + mouse" }
                        ))
                        .small()
                        .weak(),
                    );
                });
                if primary != last_device {
                    ui.label(RichText::new("(you're using the other device: best-effort mapping)").small().weak());
                }
                egui::Grid::new("room_guide").show(ui, |ui| {
                    if def.controls.is_empty() {
                        for (a, k) in guide(last_device) {
                            ui.label(*a);
                            ui.label(RichText::new(*k).monospace());
                            ui.end_row();
                        }
                    } else {
                        for [a, k] in &def.controls {
                            ui.label(a);
                            ui.label(RichText::new(k).monospace());
                            ui.end_row();
                        }
                    }
                });
                if let Some(m) = def.movement_model {
                    ui.label(RichText::new(format!("Movement model: {}", params::ChoiceParam::name(m))).small());
                }
                ui.label(RichText::new("F2 rooms · F4 leave · F5 reset room").small().weak());
            });
        if !open {
            self.card_until = None;
        }
    }

    /// The teleport menu (F2). Returns where to go.
    pub fn teleport_menu(&mut self, ctx: &egui::Context) -> Option<TeleportTarget> {
        if !self.teleport_open {
            return None;
        }
        let mut target = None;
        let mut open = true;
        egui::Window::new("Go to")
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                if ui.button("Pavilion plaza").clicked() {
                    target = Some(TeleportTarget::Hub);
                }
                if ui.button("Wilderness (somewhere far)").clicked() {
                    target = Some(TeleportTarget::Wilderness);
                }
                ui.separator();
                let mut wing = String::new();
                for e in &self.entries {
                    if e.wing != wing {
                        wing = e.wing.clone();
                        ui.label(RichText::new(format!("{} wing", wing)).strong());
                    }
                    ui.horizontal(|ui| {
                        if ui.button(&e.name).clicked() {
                            target = Some(TeleportTarget::Room(e.key.clone()));
                        }
                        ui.label(RichText::new(&e.about).small());
                    });
                }
                if !self.errors.is_empty() {
                    ui.separator();
                    for e in &self.errors {
                        ui.label(RichText::new(e).color(egui::Color32::LIGHT_RED).small());
                    }
                }
            });
        if !open || target.is_some() {
            self.teleport_open = false;
        }
        target
    }

    /// Room-file errors (hot reload) stay on screen until fixed.
    pub fn error_panel(&self, ctx: &egui::Context) {
        if self.errors.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::new("room_errors")).anchor(egui::Align2::CENTER_BOTTOM, [0.0, -40.0]).show(ctx, |ui| {
            egui::Frame::popup(ui.style()).fill(egui::Color32::from_rgb(70, 20, 20)).show(ui, |ui| {
                ui.label(RichText::new("Room file errors (fix the file; it reloads automatically)").strong());
                for e in &self.errors {
                    ui.label(RichText::new(e).monospace().small());
                }
            });
        });
    }
}

/// Watches the rooms directory; `poll` returns true once changes have settled.
pub struct RoomWatcher {
    _watcher: notify::RecommendedWatcher,
    rx: Receiver<()>,
    dirty_since: Option<Instant>,
    pub dir: std::path::PathBuf,
}

impl RoomWatcher {
    pub fn start() -> Option<Self> {
        use notify::Watcher;
        let dir = pav_core::room::rooms_dir()?;
        let (tx, rx) = channel();
        let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(ev) = res {
                // Reading files (our own reload) produces access events: ignore those.
                if matches!(ev.kind, notify::EventKind::Access(_)) {
                    return;
                }
                if ev.paths.iter().any(|p| p.extension().is_some_and(|x| x == "toml")) {
                    let _ = tx.send(());
                }
            }
        })
        .map_err(|e| log::warn!("room hot reload unavailable: {e}"))
        .ok()?;
        w.watch(&dir, notify::RecursiveMode::NonRecursive).map_err(|e| log::warn!("cannot watch {}: {e}", dir.display())).ok()?;
        log::info!("hot reload: watching {}", dir.display());
        Some(Self { _watcher: w, rx, dirty_since: None, dir })
    }

    pub fn poll(&mut self) -> bool {
        while self.rx.try_recv().is_ok() {
            self.dirty_since = Some(Instant::now());
        }
        match self.dirty_since {
            Some(t) if t.elapsed() > Duration::from_millis(250) => {
                self.dirty_since = None;
                true
            }
            _ => false,
        }
    }
}
