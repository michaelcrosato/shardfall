//! Teaching in the world demo: the station guide (H, or the room card's button) explains the
//! room you are in (what you see, how it works, the pads, live settings, where games use it,
//! how to ask for it, what it costs, the engine's own code, the words); the field guide has
//! every word and every room's "ask for it" phrases; and stepping on a pad with a note shows
//! the note for a while.

use egui::{Color32, RichText, Ui};
use pav_core::frame::{HudFrame, RoomInfo};
use pav_core::guide;
use pav_core::params::{self, Tunable};

use crate::rooms::RoomEntry;

/// What the guides want the app to do.
pub enum GuideAction {
    /// Teleport to a room.
    Go(String),
    /// Something to say in a toast.
    Toast(String),
}

#[derive(Default)]
pub struct GuideUi {
    pub station: bool,
    pub field: bool,
    filter: String,
}

const ACCENT: Color32 = Color32::from_rgb(255, 214, 140);

fn heading(icon: &str, text: &str) -> egui::CollapsingHeader {
    egui::CollapsingHeader::new(RichText::new(format!("{icon}  {text}")).strong().color(ACCENT))
}

/// "Title: text" as a bold title and its text.
fn titled(ui: &mut Ui, s: &str) {
    match s.split_once(':') {
        Some((t, rest)) if t.len() < 40 => {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(RichText::new(t).strong());
                ui.label(rest.trim());
            });
        }
        _ => {
            ui.label(format!("• {s}"));
        }
    }
}

impl GuideUi {
    /// The station guide for `room`. `root` holds every setting (view, camera, simulation):
    /// the guide's knobs edit it like the F1 panel does.
    pub fn station(&mut self, ctx: &egui::Context, room: Option<&RoomInfo>, root: &mut dyn Tunable) -> Option<GuideAction> {
        if !self.station {
            return None;
        }
        let Some(room) = room else {
            // Outside the rooms: the field guide is the guide.
            self.station = false;
            self.field = true;
            return None;
        };
        let def = &room.def;
        let l = &def.learn;
        let name = if def.name.is_empty() { room.key.clone() } else { def.name.clone() };
        let mut out = None;
        let mut open = true;
        let screen = ctx.content_rect();
        egui::Window::new(format!("{name} · how it works"))
            .id(egui::Id::new("station_guide"))
            .open(&mut open)
            .default_pos([screen.right() - 500.0, 60.0])
            .default_width(470.0)
            .default_height((screen.height() - 140.0).min(760.0))
            .max_height(screen.height() - 90.0)
            .vscroll(true)
            .show(ctx, |ui| {
                if !def.about.is_empty() {
                    ui.label(RichText::new(&def.about).italics());
                }
                if l.is_empty() {
                    ui.label(
                        RichText::new("No station guide for this room yet: the room card and the F1 panel have its settings.")
                            .weak(),
                    );
                }
                if !l.what.is_empty() {
                    heading("👁", "What you're seeing").default_open(true).show(ui, |ui| {
                        ui.label(&l.what);
                    });
                }
                if !l.how.is_empty() {
                    heading("⚙", "How it works").default_open(true).show(ui, |ui| {
                        for (i, step) in l.how.iter().enumerate() {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(format!("{}.", i + 1)).strong().color(ACCENT));
                                ui.label(step);
                            });
                        }
                    });
                }
                let pads = def.pads();
                if pads.iter().any(|(_, n)| !n.is_empty()) {
                    heading("▣", "The pads").default_open(false).show(ui, |ui| {
                        for (label, note) in pads.iter().filter(|(_, n)| !n.is_empty()) {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(*label).strong().monospace());
                                ui.label(*note);
                            });
                        }
                    });
                }
                if !l.knobs.is_empty() {
                    heading("🔧", "Tweak it live").default_open(true).show(ui, |ui| {
                        ui.label(
                            RichText::new("The same settings as the F1 panel; pads and leaving the room set them back.")
                                .small()
                                .weak(),
                        );
                        knobs(ui, &l.knobs, root);
                    });
                }
                if !def.try_list.is_empty() {
                    heading("👉", "Try this").default_open(false).show(ui, |ui| {
                        for t in &def.try_list {
                            ui.label(format!("• {t}"));
                        }
                    });
                }
                if !l.uses.is_empty() {
                    heading("🎮", "Where games use it").default_open(false).show(ui, |ui| {
                        for u in &l.uses {
                            titled(ui, u);
                        }
                    });
                }
                if !l.ask.is_empty() {
                    heading("💬", "Ask for it like…").default_open(false).show(ui, |ui| {
                        ui.label(
                            RichText::new("Words to use when you ask for this in your own game. Click one to copy it.")
                                .small()
                                .weak(),
                        );
                        ui.horizontal_wrapped(|ui| {
                            for a in &l.ask {
                                if ui.button(format!("“{a}”")).clicked() {
                                    ui.ctx().copy_text(a.clone());
                                    out = Some(GuideAction::Toast(format!("copied: {a}")));
                                }
                            }
                        });
                    });
                }
                if !l.cost.is_empty() {
                    heading("⏱", "What it costs").default_open(false).show(ui, |ui| {
                        ui.label(&l.cost);
                    });
                }
                if !l.code.is_empty() {
                    heading("⌨", "The engine's code").default_open(false).show(ui, |ui| {
                        for (i, c) in l.code.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new(&c.title).strong());
                                if ui.small_button("copy").clicked() {
                                    ui.ctx().copy_text(c.src.clone());
                                    out = Some(GuideAction::Toast("code copied".into()));
                                }
                            });
                            if !c.file.is_empty() {
                                ui.label(RichText::new(&c.file).small().monospace().weak());
                            }
                            egui::Frame::new().fill(Color32::from_black_alpha(90)).inner_margin(6.0).corner_radius(4.0).show(
                                ui,
                                |ui| {
                                    egui::ScrollArea::horizontal().id_salt(("code", i)).show(ui, |ui| {
                                        ui.label(RichText::new(c.src.trim_end()).monospace().size(11.5));
                                    });
                                },
                            );
                            ui.add_space(4.0);
                        }
                    });
                }
                let words: Vec<&guide::Term> = l.terms.iter().filter_map(|t| guide::term(t)).collect();
                if !words.is_empty() {
                    heading("📖", "Words").default_open(false).show(ui, |ui| {
                        for t in words {
                            term_row(ui, t);
                        }
                    });
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Field guide…").on_hover_text("Every word, and what to ask for in every room").clicked() {
                        self.field = true;
                    }
                    ui.label(RichText::new("H closes · F1 has every setting").small().weak());
                });
            });
        if !open {
            self.station = false;
        }
        out
    }

    /// The field guide: every word, and every room with its ask-for-it phrases.
    pub fn field_guide(&mut self, ctx: &egui::Context, rooms: &[RoomEntry]) -> Option<GuideAction> {
        if !self.field {
            return None;
        }
        let mut out = None;
        let mut open = true;
        let screen = ctx.content_rect();
        egui::Window::new("Field guide")
            .id(egui::Id::new("field_guide"))
            .open(&mut open)
            .default_pos([screen.center().x - 260.0, 70.0])
            .default_width(520.0)
            .default_height((screen.height() - 140.0).min(720.0))
            .max_height(screen.height() - 90.0)
            .vscroll(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Find");
                    ui.text_edit_singleline(&mut self.filter);
                    if !self.filter.is_empty() && ui.small_button("clear").clicked() {
                        self.filter.clear();
                    }
                });
                let q = self.filter.to_lowercase();
                let words: Vec<&guide::Term> = if q.is_empty() { guide::terms().iter().collect() } else { guide::search(&q) };
                heading("📖", &format!("Words ({})", words.len())).default_open(true).show(ui, |ui| {
                    for t in words {
                        term_row(ui, t);
                    }
                });
                heading("💬", "What to ask for, room by room").default_open(!q.is_empty()).show(ui, |ui| {
                    ui.label(RichText::new("Click a phrase to copy it; the room name takes you there.").small().weak());
                    let mut wing = String::new();
                    for r in rooms {
                        let hit = q.is_empty()
                            || r.name.to_lowercase().contains(&q)
                            || r.about.to_lowercase().contains(&q)
                            || r.ask.iter().any(|a| a.to_lowercase().contains(&q));
                        if !hit {
                            continue;
                        }
                        if r.wing != wing {
                            wing = r.wing.clone();
                            ui.add_space(4.0);
                            ui.label(RichText::new(format!("{wing} wing")).strong());
                        }
                        ui.horizontal_wrapped(|ui| {
                            if ui.link(RichText::new(&r.name).strong()).on_hover_text(&r.about).clicked() {
                                out = Some(GuideAction::Go(r.key.clone()));
                            }
                            for a in &r.ask {
                                if ui.small_button(format!("“{a}”")).clicked() {
                                    ui.ctx().copy_text(a.clone());
                                    out = Some(GuideAction::Toast(format!("copied: {a}")));
                                }
                            }
                        });
                    }
                });
            });
        if !open {
            self.field = false;
        }
        out
    }
}

fn term_row(ui: &mut Ui, t: &guide::Term) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(RichText::new(&t.name).strong());
        ui.label(&t.text);
        if !t.see.is_empty() {
            ui.label(RichText::new(format!("(see {})", t.see.join(", "))).small().weak());
        }
    });
    ui.add_space(2.0);
}

/// Controls for the given setting paths, edited in `root`.
fn knobs(ui: &mut Ui, paths: &[String], root: &mut dyn Tunable) {
    let list = params::list(root);
    let mut changes = Vec::new();
    for path in paths {
        match list.iter().find(|p| &p.path == path) {
            Some(p) => crate::panel::param_widget(ui, p, &mut changes),
            None => {
                ui.label(RichText::new(format!("(unknown setting {path})")).small().weak());
            }
        }
    }
    for (path, v) in changes {
        let _ = params::set(root, &path, v);
    }
}

/// The note of the last pad stepped on, for a while after (bottom centre).
pub fn pad_note(ctx: &egui::Context, hud: &HudFrame, tick: u64, dt: f32) {
    let Some((label, note, at)) = &hud.pad_note else { return };
    let age = tick.saturating_sub(*at) as f32 * dt.max(1e-3);
    if age > 14.0 {
        return;
    }
    let fade = (1.0 - (age - 12.0).max(0.0) / 2.0).clamp(0.0, 1.0);
    egui::Area::new(egui::Id::new("pad_note")).anchor(egui::Align2::CENTER_BOTTOM, [0.0, -64.0]).interactable(false).show(
        ctx,
        |ui| {
            ui.set_max_width(560.0);
            ui.multiply_opacity(fade);
            egui::Frame::popup(ui.style()).fill(Color32::from_rgba_unmultiplied(18, 20, 28, 225)).show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(label).strong().color(ACCENT));
                    ui.label(note);
                });
            });
        },
    );
}
