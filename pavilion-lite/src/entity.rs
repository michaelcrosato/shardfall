//! Entities: one plain struct for everything (level blocks, props, triggers, characters,
//! pickups). Build them with `Spawn` and `World::spawn`.

use glam::{Quat, Vec3};
use rapier3d::prelude::{ColliderBuilder, RigidBodyHandle};
use serde::{Deserialize, Serialize};

use crate::character::Character;
use crate::puppet::Puppet;
use crate::util::Color;

/// Entity ids are never reused.
pub type Id = u32;

/// Collision and drawing shape, centred on the entity position.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Shape {
    Box {
        half: Vec3,
    },
    Sphere {
        radius: f32,
    },
    /// Along local Y; total height = 2 * (half_height + radius).
    Capsule {
        half_height: f32,
        radius: f32,
    },
    Cylinder {
        half_height: f32,
        radius: f32,
    },
}

impl Shape {
    /// A box from its full size.
    pub fn cube(size: Vec3) -> Shape {
        Shape::Box { half: size * 0.5 }
    }
    pub fn ball(radius: f32) -> Shape {
        Shape::Sphere { radius }
    }
    pub(crate) fn collider(&self) -> ColliderBuilder {
        match *self {
            Shape::Box { half } => ColliderBuilder::cuboid(half.x, half.y, half.z),
            Shape::Sphere { radius } => ColliderBuilder::ball(radius),
            Shape::Capsule { half_height, radius } => ColliderBuilder::capsule_y(half_height, radius),
            Shape::Cylinder { half_height, radius } => ColliderBuilder::cylinder(half_height, radius),
        }
    }
    /// Half size of the local bounding box.
    pub fn half_extents(&self) -> Vec3 {
        match *self {
            Shape::Box { half } => half,
            Shape::Sphere { radius } => Vec3::splat(radius),
            Shape::Capsule { half_height, radius } => Vec3::new(radius, half_height + radius, radius),
            Shape::Cylinder { half_height, radius } => Vec3::new(radius, half_height, radius),
        }
    }
}

crate::choice_enum! {
    /// How an entity takes part in physics.
    #[derive(Default)]
    pub enum Body {
        /// Never moves (walls, floors). Level blocks are static.
        #[default]
        Static => "static",
        /// Falls, collides, gets pushed (crates, balls).
        Dynamic => "dynamic",
        /// Moved by code (`vel`, `spin`, `mover`); pushes dynamic bodies and carries characters.
        Kinematic => "kinematic",
        /// No collision; reports `Event::Enter` / `Event::Exit` when bodies or characters overlap it.
        Trigger => "trigger",
        /// Not in physics at all (decorations, effects). Moved by `vel` / `spin`.
        None => "none",
    }
}

crate::choice_enum! {
    /// Surface style.
    #[derive(Default)]
    pub enum Look {
        /// Two-tone toon lighting with shadows (the default look).
        #[default]
        Cel => "cel",
        /// Smooth lighting.
        Lit => "lit",
        /// Base colour only (shadows still darken it).
        Flat => "flat",
        /// Self-lit, unaffected by light and shadow (pickups, lava, bullets).
        Glow => "glow",
    }
}

/// Back-and-forth motion for kinematic platforms, doors and hazards: the entity moves between
/// `from` and `from + offset`, one round trip per `period` seconds, pausing `hold` seconds at
/// each end. Position is a function of time, so rewinds stay exact.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Mover {
    pub from: Vec3,
    pub offset: Vec3,
    pub period: f32,
    #[serde(default)]
    pub hold: f32,
    /// Start offset as a share of the period (0..1).
    #[serde(default)]
    pub phase: f32,
}

impl Mover {
    /// Position at time `t` (seconds).
    pub fn at(&self, t: f32) -> Vec3 {
        let p = self.period.max(0.05);
        let h = self.hold.clamp(0.0, p * 0.45);
        let travel = (p - 2.0 * h) * 0.5;
        let x = (t / p + self.phase).rem_euclid(1.0) * p;
        let s = if x < travel {
            x / travel
        } else if x < travel + h {
            1.0
        } else if x < 2.0 * travel + h {
            1.0 - (x - travel - h) / travel
        } else {
            0.0
        };
        let s = s * s * (3.0 - 2.0 * s);
        self.from + self.offset * s
    }
}

#[derive(Clone, Debug)]
pub struct Entity {
    pub id: Id,
    pub name: String,
    /// What it is for game rules ("coin", "enemy", "goal"...). Level legends set it.
    pub kind: String,
    /// Centre of the shape. For characters: the FEET (bottom of the capsule).
    pub pos: Vec3,
    pub rot: Quat,
    /// Velocity (m/s). Dynamic bodies and characters: read it (physics owns it; use
    /// `World::push` / `set_vel`). Kinematic and None bodies: set it and the engine moves them.
    pub vel: Vec3,
    /// Angular velocity (rad/s, axis * speed) for kinematic and None bodies.
    pub spin: Vec3,
    pub shape: Shape,
    pub body: Body,
    pub color: Color,
    pub look: Look,
    pub visible: bool,
    /// Projectiles pass through entities of the shooter's team (0 = no team).
    pub team: u8,
    /// Health. Entities with hp > 0 take projectile damage (`Event::Killed` at 0).
    pub hp: f32,
    pub max_hp: f32,
    /// Seconds of white hit-flash left (drawing only).
    pub flash: f32,
    /// Seconds during which damage is ignored (dodges, mercy time after a hit).
    pub invuln: f32,
    /// Seconds until the entity removes itself.
    pub life: Option<f32>,
    pub mover: Option<Mover>,
    /// Walks, jumps and gets knocked back (see `character.rs`).
    pub character: Option<Box<Character>>,
    /// Drawn as a procedurally animated body instead of its shape.
    pub puppet: Option<Box<Puppet>>,
    pub(crate) handle: Option<RigidBodyHandle>,
}

impl Entity {
    /// The middle of the entity (for characters: halfway up the capsule).
    pub fn center(&self) -> Vec3 {
        match &self.character {
            Some(c) => self.pos + Vec3::Y * c.height * 0.5,
            None => self.pos,
        }
    }
    /// Distance on the ground plane.
    pub fn flat_dist(&self, p: Vec3) -> f32 {
        glam::Vec2::new(self.pos.x - p.x, self.pos.z - p.z).length()
    }
    pub fn alive(&self) -> bool {
        self.hp > 0.0
    }
}

/// Builder for `World::spawn`. Defaults: a 1 m static grey box.
#[derive(Clone, Debug)]
pub struct Spawn {
    pub name: String,
    pub kind: String,
    pub pos: Vec3,
    pub rot: Quat,
    pub vel: Vec3,
    pub spin: Vec3,
    pub shape: Shape,
    pub body: Body,
    pub color: Color,
    pub look: Look,
    pub visible: bool,
    pub team: u8,
    pub hp: f32,
    pub life: Option<f32>,
    pub mover: Option<Mover>,
    pub density: f32,
    pub friction: f32,
    pub restitution: f32,
    pub damping: f32,
    pub ccd: bool,
    pub character: Option<Character>,
    pub puppet: Option<Puppet>,
}

impl Spawn {
    /// `kind` doubles as the name until `.name()` is called.
    pub fn new(kind: &str, pos: Vec3) -> Self {
        Self {
            name: kind.into(),
            kind: kind.into(),
            pos,
            rot: Quat::IDENTITY,
            vel: Vec3::ZERO,
            spin: Vec3::ZERO,
            shape: Shape::Box { half: Vec3::splat(0.5) },
            body: Body::Static,
            color: Color::hex("#b0b0b0"),
            look: Look::Cel,
            visible: true,
            team: 0,
            hp: 0.0,
            life: None,
            mover: None,
            density: 1.0,
            friction: 0.6,
            restitution: 0.1,
            damping: 0.05,
            ccd: false,
            character: None,
            puppet: None,
        }
    }
    /// A walking character standing with its feet at `feet` (capsule 1.7 m tall, 0.32 m
    /// radius), drawn as a biped puppet. Change it with `.puppet(..)` / `.size(..)`.
    pub fn character(kind: &str, feet: Vec3) -> Self {
        let mut s = Self::new(kind, feet);
        s.body = Body::Kinematic;
        s.character = Some(Character::new(1.7, 0.32));
        s.shape = Shape::Capsule { half_height: 1.7 * 0.5 - 0.32, radius: 0.32 };
        s.puppet(Puppet::default())
    }
    pub fn name(mut self, n: &str) -> Self {
        self.name = n.into();
        self
    }
    pub fn shape(mut self, s: Shape) -> Self {
        self.shape = s;
        self
    }
    /// Box of full size `size`.
    pub fn cube(self, size: Vec3) -> Self {
        self.shape(Shape::cube(size))
    }
    pub fn ball(self, radius: f32) -> Self {
        self.shape(Shape::Sphere { radius })
    }
    pub fn body(mut self, b: Body) -> Self {
        self.body = b;
        self
    }
    pub fn color(mut self, hex: &str) -> Self {
        self.color = Color::hex(hex);
        self
    }
    pub fn look(mut self, l: Look) -> Self {
        self.look = l;
        self
    }
    pub fn rot(mut self, r: Quat) -> Self {
        self.rot = r;
        self
    }
    pub fn vel(mut self, v: Vec3) -> Self {
        self.vel = v;
        self
    }
    pub fn spin(mut self, w: Vec3) -> Self {
        self.spin = w;
        self
    }
    pub fn team(mut self, t: u8) -> Self {
        self.team = t;
        self
    }
    pub fn hp(mut self, hp: f32) -> Self {
        self.hp = hp;
        self
    }
    /// Removes itself after `seconds`.
    pub fn life(mut self, seconds: f32) -> Self {
        self.life = Some(seconds);
        self
    }
    pub fn hidden(mut self) -> Self {
        self.visible = false;
        self
    }
    /// Kinematic back-and-forth motion by `offset` (see `Mover`).
    pub fn mover(mut self, offset: Vec3, period: f32, hold: f32) -> Self {
        self.body = Body::Kinematic;
        self.mover = Some(Mover { from: self.pos, offset, period, hold, phase: 0.0 });
        self
    }
    /// Physical material: density (kg/m³ / 1000), friction, bounciness.
    pub fn material(mut self, density: f32, friction: f32, restitution: f32) -> Self {
        self.density = density;
        self.friction = friction;
        self.restitution = restitution;
        self
    }
    /// Continuous collision detection for small fast dynamic bodies.
    pub fn ccd(mut self) -> Self {
        self.ccd = true;
        self
    }
    /// Draw as a procedural body (its main colour becomes the entity's `color`).
    pub fn puppet(mut self, p: Puppet) -> Self {
        self.color = p.body;
        self.puppet = Some(p);
        self
    }
    /// Character capsule size: total height and radius (feet stay at `pos`).
    pub fn size(mut self, height: f32, radius: f32) -> Self {
        if let Some(c) = &mut self.character {
            *c = Character::new(height, radius);
            self.shape = Shape::Capsule { half_height: c.half_height(), radius: c.radius };
        }
        self
    }
    /// Character speed and jump multipliers (1 = the shared `movement.*` parameters).
    pub fn agility(mut self, speed: f32, jump: f32) -> Self {
        if let Some(c) = &mut self.character {
            c.speed = speed;
            c.jump = jump;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mover_goes_there_and_back() {
        let m = Mover { from: Vec3::ZERO, offset: Vec3::X * 4.0, period: 4.0, hold: 0.0, phase: 0.0 };
        assert!(m.at(0.0).distance(Vec3::ZERO) < 1e-4);
        assert!(m.at(2.0).distance(Vec3::X * 4.0) < 1e-4);
        assert!(m.at(4.0).distance(Vec3::ZERO) < 1e-4);
        let h = Mover { hold: 1.0, ..m };
        assert!(h.at(1.5).distance(Vec3::X * 4.0) < 1e-4);
        assert!(h.at(1.9).distance(Vec3::X * 4.0) < 1e-4);
        assert!(h.at(3.5).distance(Vec3::ZERO) < 1e-4);
    }
}
