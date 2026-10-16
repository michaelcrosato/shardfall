//! The Clip Browser room's stage: every motion clip in the library, one at a time, played by a
//! cast side by side (the stock puppet and the imported figures), so a clip can be seen on
//! several bodies at once. Pads send signals: the next or previous clip, the first clip of a
//! set, and auto-play, which steps on by itself. The clip on show and the cast's performances
//! are simulation state, so rewind, snapshots and replays keep them.

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

use crate::Sim;
use crate::color::Color;
use crate::statics::RegionKey;
use crate::zones::{Label, LabelMode};

/// Steps a cast through the clip library. In a room file: `behavior = { type = "clip_browser" }`
/// on an object with `body = "none"`; the performers are `[[npc]]`s whose names start with `cast`,
/// and the clip's card lies on the floor at the object (its name), then 1.45 m toward +z.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ClipBrowserDef {
    /// Signals: the next clip, the one before, auto-play on or off.
    pub next: String,
    pub back: String,
    pub auto: String,
    /// `signal = "SET"` pairs: a pad sending `signal` shows the set's first clip.
    pub sets: Vec<[String; 2]>,
    /// The performers: the room's characters whose names start with this.
    pub cast: String,
    /// Seconds a looping clip plays before auto-play steps on (a one-shot plays through twice).
    pub hold: f32,
    /// The first clip shown (`SET/Clip`); empty: the library's first.
    pub start: String,
    /// The clip on show (an index into `ids()`; -1: none yet), whether auto-play is on, and the
    /// seconds until it steps.
    pub index: i32,
    pub playing: bool,
    pub timer: f32,
    pub signs: Vec<Label>,
}

impl Default for ClipBrowserDef {
    fn default() -> Self {
        Self {
            next: "clip_next".into(),
            back: "clip_back".into(),
            auto: "clip_auto".into(),
            sets: Vec::new(),
            cast: "cast".into(),
            hold: 5.0,
            start: String::new(),
            index: -1,
            playing: false,
            timer: 0.0,
            signs: Vec::new(),
        }
    }
}

/// Every clip in the library in the browser's order: set by set as the library holds them, each
/// set's clips by name.
pub fn ids() -> Vec<u32> {
    let lib = crate::clips::library();
    lib.sets.iter().flat_map(|set| set.clips.keys().map(|name| crate::clips::clip_id(&set.set, name))).collect()
}

/// Seconds auto-play gives clip `id`: `hold` for a loop, two plays (and the pause between them)
/// for a one-shot.
fn showing_time(id: u32, hold: f32) -> f32 {
    crate::clips::with(id, |c| if c.looping { hold.max(c.dur) } else { 2.0 * c.dur + 1.6 }).unwrap_or(hold)
}

/// What the browser does on a tick.
#[derive(Debug, PartialEq)]
enum Step {
    None,
    By(i32),
    To(u32),
    Toggle,
}

impl ClipBrowserDef {
    /// As authored: nothing on show yet.
    pub fn fresh(&self) -> Self {
        Self { index: -1, playing: false, timer: 0.0, signs: Vec::new(), ..self.clone() }
    }

    fn step(&self, signals: &[String]) -> Step {
        let sent = |name: &String| !name.is_empty() && signals.contains(name);
        if sent(&self.next) {
            return Step::By(1);
        }
        if sent(&self.back) {
            return Step::By(-1);
        }
        if sent(&self.auto) {
            return Step::Toggle;
        }
        let lib = crate::clips::library();
        for [signal, set] in &self.sets {
            if sent(signal) {
                if let Some(s) = lib.sets.iter().find(|s| s.set.eq_ignore_ascii_case(set)) {
                    if let Some(first) = s.clips.keys().next() {
                        return Step::To(crate::clips::clip_id(&s.set, first));
                    }
                }
            }
        }
        Step::None
    }
}

/// The signs for clip `id`, the `n`th of `total`: its name above the stage, then what it is.
pub fn sign_text(id: u32, n: usize, total: usize, playing: bool) -> (String, String) {
    let lib = crate::clips::library();
    let name = lib.name_of(id).unwrap_or_else(|| format!("clip {id}"));
    let title = format!("{name}   {n} / {total}{}", if playing { "   AUTO" } else { "" });
    let detail = (|| {
        let (set_name, clip_name) = name.split_once('/')?;
        let set = lib.sets.iter().find(|s| s.set == set_name)?;
        let clip = set.clips.get(clip_name)?;
        let source = set.sources.get(&clip.src);
        let field = |key: &str| source.and_then(|s| s.get(key)).and_then(|v| v.as_str()).unwrap_or("").to_owned();
        let mut lines = vec![format!(
            "{:.1} s{}{}",
            clip.dur,
            if clip.looping { " · loop" } else { " · plays once" },
            set.fit
                .get(clip_name)
                .map_or(String::new(), |f| format!(" · follows its capture to {:.0} mm (worst {:.0})", f[0], f[1])),
        )];
        if !clip.desc.is_empty() {
            lines.push(clip.desc.clone());
        }
        if !clip.tags.is_empty() {
            lines.push(format!("tags: {}", clip.tags.join(", ")));
        }
        lines.push(format!("{} · {}", field("label"), field("license")));
        Some(lines.join("\n"))
    })()
    .unwrap_or_default();
    (title, detail)
}

impl Sim {
    /// Runs a clip browser for a tick: steps on a signal (or auto-play's timer), gives the cast
    /// the clip on show and lays its card at `origin`. Returns whether `def` changed.
    pub(crate) fn clip_browser_tick(
        &mut self,
        region: Option<RegionKey>,
        origin: Vec3,
        rot: Quat,
        def: &mut ClipBrowserDef,
        signals: &[String],
        dt: f32,
    ) -> bool {
        let step = def.step(signals);
        if step == Step::None && def.index >= 0 && !def.playing {
            return false;
        }
        let ids = ids();
        if ids.is_empty() {
            return false;
        }
        let total = ids.len() as i32;
        let mut target = match step {
            Step::By(k) => {
                Some(if def.index < 0 { if k > 0 { 0 } else { total - 1 } } else { (def.index + k).rem_euclid(total) })
            }
            Step::To(id) => ids.iter().position(|&x| x == id).map(|i| i as i32),
            Step::Toggle => {
                def.playing = !def.playing;
                def.timer = if def.index >= 0 { showing_time(ids[def.index as usize], def.hold) } else { 0.0 };
                Some(def.index.max(0))
            }
            Step::None => None,
        };
        if target.is_none() && def.index < 0 {
            // The first tick: the authored start (or the first clip), so the stage is never empty.
            let start = crate::clips::find(&def.start).and_then(|id| ids.iter().position(|&x| x == id));
            target = Some(start.unwrap_or(0) as i32);
        }
        if target.is_none() && def.playing {
            def.timer -= dt;
            if def.timer <= 0.0 {
                target = Some((def.index + 1).rem_euclid(total));
            }
        }
        let Some(index) = target else { return false };
        def.index = index;
        let id = ids[index as usize];
        def.timer = showing_time(id, def.hold);
        // Every performer starts the clip on this tick, so the cast moves together.
        let cast: Vec<_> = self
            .state
            .entities
            .iter()
            .filter(|e| e.region == region && e.ai.is_some() && !def.cast.is_empty() && e.name.starts_with(&def.cast))
            .map(|e| e.id)
            .collect();
        for member in cast {
            if let Some(ai) = self.state.entities.get_mut(member).and_then(|e| e.ai.as_mut()) {
                ai.perform = Some(crate::ai::Perform::new(vec![id], Vec::new(), 0, def.hold, 1.0));
            }
        }
        if let Some(region) = region {
            self.state.statics.remove_labels_from(region, &std::mem::take(&mut def.signs));
        }
        let (title, detail) = sign_text(id, index as usize + 1, ids.len(), def.playing);
        let label = |text: String, local: Vec3, size: f32, color: &str| Label {
            text,
            pos: origin + rot * local,
            size,
            color: Color::hex(color),
            mode: LabelMode::Floor,
            facing: Default::default(),
        };
        // On the floor: the view cuts away everything 2.4 m above the player within 9 m of them.
        def.signs = vec![
            label(title, Vec3::new(0.0, 0.03, 0.0), 0.6, "#ffe6a8"),
            label(detail, Vec3::new(0.0, 0.03, 1.5), 0.3, "#e8edf5"),
        ];
        if let Some(region) = region {
            for sign in &def.signs {
                self.state.statics.add_label_to(region, sign.clone());
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_order_covers_every_clip_once_and_signs_name_it() {
        let ids = ids();
        let lib = crate::clips::library();
        assert_eq!(ids.len(), lib.len());
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), ids.len());
        let (title, detail) = sign_text(ids[0], 1, ids.len(), false);
        assert!(title.contains(&lib.name_of(ids[0]).unwrap()) && title.ends_with(&format!("1 / {}", ids.len())), "{title}");
        assert!(detail.contains(" s · "), "{detail}");
    }

    #[test]
    fn signals_step_wrap_and_jump_to_a_set() {
        let def = ClipBrowserDef { sets: vec![["to_q".into(), "quaternius".into()]], ..Default::default() };
        assert_eq!(def.step(&["clip_next".into()]), Step::By(1));
        assert_eq!(def.step(&["clip_back".into()]), Step::By(-1));
        assert_eq!(def.step(&["clip_auto".into()]), Step::Toggle);
        assert_eq!(def.step(&["other".into()]), Step::None);
        let Step::To(id) = def.step(&["to_q".into()]) else { panic!("no jump") };
        assert!(crate::clips::library().name_of(id).unwrap().starts_with("QUATERNIUS/"));
    }
}
