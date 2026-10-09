//! Window events -> egui input. Natively this is egui-winit; in the browser (where egui-winit
//! does not build) a small adapter covers what the panels use: pointer, wheel, keys and text.

use winit::event::WindowEvent;
use winit::window::Window;

#[cfg(not(target_arch = "wasm32"))]
pub struct UiInput(egui_winit::State);

#[cfg(not(target_arch = "wasm32"))]
impl UiInput {
    pub fn new(ctx: &egui::Context, window: &Window, max_texture: usize) -> Self {
        Self(egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            None,
            Some(max_texture),
        ))
    }

    /// Feeds an event to egui; true when egui used it (the game should ignore it).
    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        self.0.on_window_event(window, event).consumed
    }

    pub fn take(&mut self, window: &Window) -> egui::RawInput {
        self.0.take_egui_input(window)
    }

    pub fn platform_output(&mut self, window: &Window, out: egui::PlatformOutput) {
        self.0.handle_platform_output(window, out);
    }
}

#[cfg(target_arch = "wasm32")]
pub struct UiInput {
    ctx: egui::Context,
    raw: egui::RawInput,
    pointer: egui::Pos2,
    modifiers: egui::Modifiers,
    start: web_time::Instant,
    max_texture: usize,
    cursor: egui::CursorIcon,
    /// The finger that is egui's pointer (the first one down).
    touch_pointer: Option<u64>,
}

#[cfg(target_arch = "wasm32")]
impl UiInput {
    pub fn new(ctx: &egui::Context, _window: &Window, max_texture: usize) -> Self {
        Self {
            ctx: ctx.clone(),
            raw: egui::RawInput::default(),
            pointer: egui::Pos2::ZERO,
            modifiers: egui::Modifiers::default(),
            start: web_time::Instant::now(),
            max_texture,
            cursor: egui::CursorIcon::Default,
            touch_pointer: None,
        }
    }

    fn ppp(&self, window: &Window) -> f32 {
        self.ctx.zoom_factor() * window.scale_factor() as f32
    }

    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        use egui::Event;
        use winit::event::{MouseButton, MouseScrollDelta};
        let ppp = self.ppp(window);
        let over_ui = |c: &egui::Context| c.egui_wants_pointer_input() || c.is_pointer_over_egui();
        let ev = &mut self.raw.events;
        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.pointer = egui::pos2(position.x as f32 / ppp, position.y as f32 / ppp);
                ev.push(Event::PointerMoved(self.pointer));
                over_ui(&self.ctx)
            }
            WindowEvent::CursorLeft { .. } => {
                ev.push(Event::PointerGone);
                false
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    MouseButton::Left => egui::PointerButton::Primary,
                    MouseButton::Right => egui::PointerButton::Secondary,
                    MouseButton::Middle => egui::PointerButton::Middle,
                    MouseButton::Back => egui::PointerButton::Extra1,
                    MouseButton::Forward => egui::PointerButton::Extra2,
                    MouseButton::Other(_) => return false,
                };
                ev.push(Event::PointerButton {
                    pos: self.pointer,
                    button,
                    pressed: state.is_pressed(),
                    modifiers: self.modifiers,
                });
                over_ui(&self.ctx)
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (unit, delta) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (egui::MouseWheelUnit::Line, egui::vec2(*x, *y)),
                    MouseScrollDelta::PixelDelta(p) => (egui::MouseWheelUnit::Point, egui::vec2(p.x as f32, p.y as f32) / ppp),
                };
                ev.push(Event::MouseWheel { unit, delta, phase: egui::TouchPhase::Move, modifiers: self.modifiers });
                over_ui(&self.ctx)
            }
            WindowEvent::ModifiersChanged(m) => {
                let s = m.state();
                self.modifiers = egui::Modifiers {
                    alt: s.alt_key(),
                    ctrl: s.control_key(),
                    shift: s.shift_key(),
                    mac_cmd: false,
                    command: s.control_key(),
                };
                false
            }
            WindowEvent::KeyboardInput { event, .. } => {
                use winit::keyboard::Key as K;
                let pressed = event.state.is_pressed();
                let key = match &event.logical_key {
                    K::Named(n) => egui::Key::from_name(&format!("{n:?}")),
                    K::Character(c) => egui::Key::from_name(c.as_str()),
                    _ => None,
                };
                if let Some(key) = key {
                    ev.push(Event::Key { key, physical_key: None, pressed, repeat: event.repeat, modifiers: self.modifiers });
                }
                if pressed && !self.modifiers.ctrl {
                    if let Some(t) = &event.text {
                        let t: String = t.chars().filter(|c| !c.is_control()).collect();
                        if !t.is_empty() {
                            ev.push(Event::Text(t));
                        }
                    }
                }
                self.ctx.egui_wants_keyboard_input()
            }
            WindowEvent::Touch(t) => {
                // Like egui-winit: every finger as a touch (pinches), the first also as the pointer.
                use winit::event::TouchPhase as P;
                let pos = egui::pos2(t.location.x as f32 / ppp, t.location.y as f32 / ppp);
                let phase = match t.phase {
                    P::Started => egui::TouchPhase::Start,
                    P::Moved => egui::TouchPhase::Move,
                    P::Ended => egui::TouchPhase::End,
                    P::Cancelled => egui::TouchPhase::Cancel,
                };
                ev.push(Event::Touch {
                    device_id: egui::TouchDeviceId(0),
                    id: egui::TouchId::from(t.id),
                    phase,
                    pos,
                    force: None,
                });
                if self.touch_pointer.is_none_or(|id| id == t.id) {
                    let button = |pressed| Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::default(),
                    };
                    match t.phase {
                        P::Started => {
                            self.touch_pointer = Some(t.id);
                            self.pointer = pos;
                            ev.push(Event::PointerMoved(pos));
                            ev.push(button(true));
                        }
                        P::Moved => {
                            self.pointer = pos;
                            ev.push(Event::PointerMoved(pos));
                        }
                        P::Ended => {
                            self.touch_pointer = None;
                            ev.push(button(false));
                            ev.push(Event::PointerGone);
                        }
                        P::Cancelled => {
                            self.touch_pointer = None;
                            ev.push(Event::PointerGone);
                        }
                    }
                }
                true
            }
            WindowEvent::Focused(f) => {
                ev.push(Event::WindowFocused(*f));
                self.raw.focused = *f;
                false
            }
            _ => false,
        }
    }

    pub fn take(&mut self, window: &Window) -> egui::RawInput {
        let ppp = self.ppp(window);
        let size = window.inner_size();
        let points = egui::vec2(size.width as f32, size.height as f32) / ppp;
        self.raw.time = Some(self.start.elapsed().as_secs_f64());
        self.raw.screen_rect = (points.x > 0.0 && points.y > 0.0).then(|| egui::Rect::from_min_size(egui::Pos2::ZERO, points));
        self.raw.viewport_id = egui::ViewportId::ROOT;
        self.raw.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point =
            Some(window.scale_factor() as f32);
        self.raw.max_texture_side = Some(self.max_texture);
        self.raw.take()
    }

    pub fn platform_output(&mut self, window: &Window, out: egui::PlatformOutput) {
        use egui::CursorIcon as E;
        use winit::window::CursorIcon as W;
        if out.cursor_icon != self.cursor {
            self.cursor = out.cursor_icon;
            window.set_cursor(match out.cursor_icon {
                E::PointingHand => W::Pointer,
                E::Text => W::Text,
                E::Grab => W::Grab,
                E::Grabbing => W::Grabbing,
                E::ResizeHorizontal | E::ResizeColumn => W::EwResize,
                E::ResizeVertical | E::ResizeRow => W::NsResize,
                E::NotAllowed => W::NotAllowed,
                _ => W::Default,
            });
        }
    }
}
