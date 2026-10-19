//! The game UI without a window (doctrine 2, 3): the tools draw the HUD, the open windows and
//! the touch controls with the same code the window runs, on a headless egui context. A frame
//! gives a reading of what is on screen (every text with where it is, grouped by window, and the
//! touch controls) and egui's output, which `Overlay` paints over a capture.

use egui::{Pos2, Rect, Shape};
use glam::Mat4;
use pav_core::arpg::{GameCmd, GameFrame};
use pav_core::frame::HudFrame;
use serde::Serialize;

use crate::Device;
use crate::arpg_items::GameUi;
use crate::arpg_ui::{Projector, hud};
use crate::touch::{Touch, View};

/// Passes per frame: windows size themselves from the pass before.
const PASSES: usize = 3;

/// The UI's state between frames: which windows are open, the device, the touch controls.
#[derive(Default)]
pub struct Headless {
    pub game: GameUi,
    pub touch: Touch,
    pub device: Device,
}

/// What a frame drew.
pub struct Drawn {
    /// What `Overlay::paint` draws.
    pub paint: Paint,
    pub reading: Reading,
    /// What the player gave by clicking (`Input::click`).
    pub commands: Vec<GameCmd>,
}

/// What is on screen, as text.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Reading {
    pub screen: [f32; 2],
    pub device: &'static str,
    /// The windows the UI has open (`inventory`, `tree`, ...).
    pub open: Vec<&'static str>,
    /// Text outside any window: the HUD, loot labels, banners.
    pub hud: Vec<Text>,
    /// Each window or panel on screen: its title (its first line) and its text.
    pub windows: Vec<Window>,
    /// Touch: the controls a finger can press.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub controls: Vec<Control>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Text {
    pub text: String,
    /// Top-left corner, in points (pixels: the tools draw at one point per pixel).
    pub at: [f32; 2],
}

#[derive(Clone, Debug, Serialize)]
pub struct Window {
    pub title: String,
    /// `[x0, y0, x1, y1]`.
    pub rect: [f32; 4],
    pub text: Vec<Text>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Control {
    pub control: String,
    pub rect: [f32; 4],
}

/// A frame's triangles and the textures they need.
pub struct Paint {
    pub primitives: Vec<egui::ClippedPrimitive>,
    pub textures: egui::TexturesDelta,
    pub pixels_per_point: f32,
}

impl Drop for Paint {
    /// A reading that is never painted drops its textures unapplied (egui checks).
    fn drop(&mut self) {
        self.textures.clear();
    }
}

/// Pointer input for a frame: hover (tooltips) or a click at a point.
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub hover: Option<Pos2>,
    pub click: Option<Pos2>,
}

impl Headless {
    /// The window names `open` accepts, and `GameUi`'s flag for each.
    pub const WINDOWS: [&'static str; 5] = ["inventory", "character", "skills", "tree", "map"];

    fn flag(&mut self, name: &str) -> Option<&mut bool> {
        Some(match name {
            "inventory" => &mut self.game.inventory,
            "character" => &mut self.game.character,
            "skills" => &mut self.game.skills,
            "tree" => &mut self.game.tree.open,
            "map" => &mut self.game.map,
            _ => return None,
        })
    }

    /// Opens or closes a window by name; false if there is no such window.
    pub fn set_open(&mut self, name: &str, open: bool) -> bool {
        self.flag(name).map(|f| *f = open).is_some()
    }

    /// Draws one frame of the UI over a `width` x `height` view whose camera is `vp`: Shardfall's
    /// (`game`), or a `pav_core::games` game's HUD lines (`hud`).
    pub fn frame(
        &mut self,
        game: Option<&GameFrame>,
        hud_lines: &HudFrame,
        vp: Mat4,
        width: u32,
        height: u32,
        input: Input,
    ) -> Drawn {
        let ctx = egui::Context::default();
        let touch = self.device == Device::Touch;
        crate::touch_style(&ctx, touch);
        ctx.all_styles_mut(|s| {
            s.animation_time = 0.0;
            s.interaction.tooltip_delay = 0.0;
        });
        self.game.touch = touch;
        self.game.tree.touch = touch;
        self.game.pad = self.device == Device::Gamepad;
        self.game.tree.pad = self.game.pad;
        let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(width as f32, height as f32));
        let mut textures = egui::TexturesDelta::default();
        let mut commands = Vec::new();
        let mut output = None;
        let mut layers = Vec::new();
        for pass in 0..PASSES + input.click.is_some() as usize {
            let mut events = Vec::new();
            if let Some(p) = input.click.or(input.hover) {
                events.push(egui::Event::PointerMoved(p));
            }
            // The click lands in the last pass, once the windows are laid out.
            if let Some(p) = input.click.filter(|_| pass == PASSES) {
                for pressed in [true, false] {
                    events.push(egui::Event::PointerButton {
                        pos: p,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    });
                }
            }
            let raw = egui::RawInput {
                screen_rect: Some(screen),
                time: Some(pass as f64 / 60.0),
                predicted_dt: 1.0 / 60.0,
                events,
                ..Default::default()
            };
            self.touch.swipes.begin_frame();
            let last = pass + 1 == PASSES + input.click.is_some() as usize;
            let out = ctx.run_ui(raw, |ui| {
                let root = ui.layer_id();
                let ctx = ui.ctx().clone();
                if let Some(g) = game {
                    let proj = Projector { vp, size: screen.size() };
                    hud(&ctx, g, &proj, self.device, self.game.map, &mut self.touch.swipes);
                    let given = self.game.ui(&ctx, g, &proj);
                    if pass == PASSES {
                        commands = given;
                    }
                }
                crate::game_lines(&ctx, hud_lines);
                if touch && game.is_some() {
                    let map_hidden = self.touch.swipes.is_gone(egui::Id::new("minimap"), 0);
                    let points = game.and_then(|g| g.inv.as_ref()).map(|i| i.points).unwrap_or(0);
                    let covered = self.game.any_open();
                    self.touch.draw(&ctx, &View { game, auto_attack: true, map_hidden, points, covered });
                }
                if last {
                    layers = window_layers(&ctx, root, screen);
                }
            });
            let mut out = out;
            textures.append(std::mem::take(&mut out.textures_delta));
            output = Some(out);
        }
        let output = output.unwrap();
        let reading = self.read(&output, screen, layers);
        let pixels_per_point = output.pixels_per_point;
        let primitives = ctx.tessellate(output.shapes, pixels_per_point);
        Drawn { paint: Paint { primitives, textures, pixels_per_point }, reading, commands }
    }

    /// What `out` shows: the windows' text (`layers`, from `window_layers`), and the rest as HUD.
    fn read(&self, out: &egui::FullOutput, screen: Rect, layers: Vec<(Rect, Vec<(Rect, String)>)>) -> Reading {
        let mut all = Vec::new();
        for clipped in &out.shapes {
            collect(&clipped.shape, clipped.clip_rect.intersect(screen), &mut all);
        }
        let in_window = |t: &(Rect, String)| layers.iter().any(|(_, texts)| texts.contains(t));
        let mut hud: Vec<Text> = dedupe(all.into_iter().filter(|t| !in_window(t)).collect());
        let mut windows: Vec<(Rect, Vec<Text>)> =
            layers.iter().map(|(r, texts)| (*r, dedupe(texts.clone()))).filter(|(_, t)| !t.is_empty()).collect();
        // Top to bottom, then left to right (egui keeps no public stacking order).
        windows.sort_by(|a, b| a.0.min.y.total_cmp(&b.0.min.y).then(a.0.min.x.total_cmp(&b.0.min.x)));
        let windows = windows
            .into_iter()
            .map(|(r, mut text)| {
                sort(&mut text);
                let title = text.first().map(|t| t.text.clone()).unwrap_or_default();
                Window { title, rect: rect4(r), text }
            })
            .collect();
        sort(&mut hud);
        let controls = if self.device == Device::Touch {
            self.touch.placed().iter().map(|p| Control { control: format!("{:?}", p.control), rect: rect4(p.rect) }).collect()
        } else {
            Vec::new()
        };
        Reading {
            screen: [screen.width(), screen.height()],
            device: match self.device {
                Device::KeyboardMouse => "mouse",
                Device::Gamepad => "pad",
                Device::Touch => "touch",
            },
            open: Self::WINDOWS.into_iter().filter(|w| self.is_open(w)).collect(),
            hud,
            windows,
            controls,
        }
    }

    fn is_open(&self, name: &str) -> bool {
        match name {
            "inventory" => self.game.inventory,
            "character" => self.game.character,
            "skills" => self.game.skills,
            "tree" => self.game.tree.open,
            "map" => self.game.map,
            _ => false,
        }
    }
}

/// Each window (an egui area other than the screen-wide `root`) on screen this pass: its rect
/// and the text on its layer. Read before the pass ends, while egui still holds shapes by layer.
fn window_layers(ctx: &egui::Context, root: egui::LayerId, screen: Rect) -> Vec<(Rect, Vec<(Rect, String)>)> {
    let areas: Vec<(egui::LayerId, Rect)> = ctx.memory(|m| {
        let mut ids: Vec<egui::LayerId> = m.areas().visible_layer_ids().into_iter().filter(|l| *l != root).collect();
        ids.sort_by_key(|l| (l.order, l.id.value()));
        ids.into_iter().filter_map(|l| m.area_rect(l.id).map(|r| (l, r))).collect()
    });
    areas
        .into_iter()
        .map(|(layer, rect)| {
            let mut texts = Vec::new();
            ctx.graphics(|g| {
                for clipped in g.get(layer).map(|list| list.all_entries().collect::<Vec<_>>()).unwrap_or_default() {
                    collect(&clipped.shape, clipped.clip_rect.intersect(screen), &mut texts);
                }
            });
            (rect, texts)
        })
        .collect()
}

/// Texts with where they are; a drop shadow (the same text a point or two off) is kept once.
fn dedupe(texts: Vec<(Rect, String)>) -> Vec<Text> {
    let mut kept: Vec<(Rect, String)> = Vec::new();
    for (rect, text) in texts {
        if !kept.iter().any(|(r, t)| *t == text && r.min.distance(rect.min) < 3.0) {
            kept.push((rect, text));
        }
    }
    kept.into_iter().map(|(r, text)| Text { text, at: [r.min.x.round(), r.min.y.round()] }).collect()
}

/// Every visible text in `shape` with its bounds.
fn collect(shape: &Shape, clip: Rect, out: &mut Vec<(Rect, String)>) {
    match shape {
        Shape::Vec(shapes) => {
            for s in shapes {
                collect(s, clip, out);
            }
        }
        Shape::Text(t) => {
            let rect = t.visual_bounding_rect();
            let text = t.galley.text().trim();
            if !text.is_empty() && t.opacity_factor > 0.0 && clip.intersects(rect) {
                out.push((rect, text.to_string()));
            }
        }
        _ => {}
    }
}

/// Reading order: rows top to bottom (within 4 points is one row), then left to right.
fn sort(texts: &mut [Text]) {
    texts.sort_by(|a, b| {
        let row = |t: &Text| (t.at[1] / 4.0).round();
        row(a).total_cmp(&row(b)).then(a.at[0].total_cmp(&b.at[0]))
    });
}

fn rect4(r: Rect) -> [f32; 4] {
    [r.min.x.round(), r.min.y.round(), r.max.x.round(), r.max.y.round()]
}

/// Paints egui output over a texture (a capture), on one device.
pub struct Overlay {
    renderer: egui_wgpu::Renderer,
    format: wgpu::TextureFormat,
}

impl Overlay {
    pub fn new(device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        Self { renderer: egui_wgpu::Renderer::new(device, format, egui_wgpu::RendererOptions::default()), format }
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Records the UI's draw over `view` (loaded, not cleared) into `encoder`. Its textures
    /// are uploaded now and freed after.
    pub fn paint(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        size: (u32, u32),
        paint: &Paint,
    ) {
        let prims = &paint.primitives;
        for (id, delta) in &paint.textures.set {
            for d in delta {
                self.renderer.update_texture(device, queue, *id, d);
            }
        }
        let sd = egui_wgpu::ScreenDescriptor { size_in_pixels: [size.0, size.1], pixels_per_point: paint.pixels_per_point };
        let cmds = self.renderer.update_buffers(device, queue, encoder, prims, &sd);
        queue.submit(cmds);
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("ui overlay"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        self.renderer.render(&mut pass.forget_lifetime(), prims, &sd);
        for id in &paint.textures.free {
            self.renderer.free_texture(id);
        }
    }
}
