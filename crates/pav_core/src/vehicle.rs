//! Vehicles the player can drive. A drift car uses rapier's raycast vehicle (four sprung wheels
//! on a dynamic chassis; the handbrake takes grip away from the rear wheels so it slides). A
//! helicopter is an arcade flyer: it hovers, climbs and descends on jump / crouch, and tilts into
//! its motion. The player rides along hidden; interact (E) gets in and out.

use glam::{Quat, Vec2, Vec3};
use rapier::control::{DynamicRayCastVehicleController, WheelTuning};
use rapier::prelude::*;
use serde::{Deserialize, Serialize};

use crate::choice_enum;
use crate::entity::{BodyKind, EntityId, Spawn};
use crate::input::{InputFrame, buttons};
use crate::params::{ParamVisitor, Tunable};
use crate::shape::{Shape, Visual};
use crate::sim::Sim;

choice_enum! {
    #[derive(Default)]
    pub enum VehicleKind {
        #[default]
        Car => "car",
        Helicopter => "helicopter",
    }
}

/// Unset fields come from the kind's preset (see `VehicleDef::preset`).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VehicleDef {
    pub kind: VehicleKind,
    pub color: String,
    pub accent: String,
    /// Body half extents (x width, y height, z length).
    pub size: Vec3,
    pub mass: f32,
    /// Car: engine force per driven wheel (N). Helicopter: acceleration (m/s²).
    pub power: f32,
    pub max_speed: f32,
    /// Car: sideways tyre grip (higher = less sliding).
    pub grip: f32,
    /// Car: rear grip while the handbrake (jump) is held: low = big drifts.
    pub drift_grip: f32,
    /// Car: steering lock (degrees).
    pub steer: f32,
    /// Helicopter: climb speed (m/s).
    pub climb: f32,
    /// Helicopter: highest flying height above the ground it was placed on (m).
    pub ceiling: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VehicleDefRaw {
    kind: Option<VehicleKind>,
    color: Option<String>,
    accent: Option<String>,
    size: Option<Vec3>,
    mass: Option<f32>,
    power: Option<f32>,
    max_speed: Option<f32>,
    grip: Option<f32>,
    drift_grip: Option<f32>,
    steer: Option<f32>,
    climb: Option<f32>,
    ceiling: Option<f32>,
}

impl<'de> Deserialize<'de> for VehicleDef {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let r = VehicleDefRaw::deserialize(d)?;
        let mut v = VehicleDef::preset(r.kind.unwrap_or_default());
        macro_rules! set {
            ($($f:ident),*) => { $( if let Some(x) = r.$f { v.$f = x; } )* };
        }
        set!(color, accent, size, mass, power, max_speed, grip, drift_grip, steer, climb, ceiling);
        Ok(v)
    }
}

impl Default for VehicleDef {
    fn default() -> Self {
        Self::preset(VehicleKind::Car)
    }
}

impl VehicleDef {
    pub fn preset(kind: VehicleKind) -> Self {
        match kind {
            VehicleKind::Car => Self {
                kind,
                color: "#e8443a".into(),
                accent: "#2b2f3a".into(),
                size: Vec3::new(0.85, 0.32, 1.9),
                mass: 900.0,
                power: 2600.0,
                max_speed: 22.0,
                grip: 3.0,
                drift_grip: 0.6,
                steer: 32.0,
                climb: 0.0,
                ceiling: 0.0,
            },
            VehicleKind::Helicopter => Self {
                kind,
                color: "#3a7be0".into(),
                accent: "#f2c14e".into(),
                size: Vec3::new(0.8, 0.7, 1.4),
                mass: 600.0,
                power: 12.0,
                max_speed: 14.0,
                grip: 0.0,
                drift_grip: 0.0,
                steer: 0.0,
                climb: 5.0,
                ceiling: 14.0,
            },
        }
    }
}

impl Tunable for VehicleDef {
    fn visit(&mut self, v: &mut dyn ParamVisitor) {
        v.float("power", &mut self.power, 0.0, 10000.0, "Engine force per wheel (car) / acceleration (helicopter)");
        v.float("max_speed", &mut self.max_speed, 1.0, 60.0, "Top speed (m/s)");
        v.float("grip", &mut self.grip, 0.1, 10.0, "Tyre side grip");
        v.float("drift_grip", &mut self.drift_grip, 0.05, 5.0, "Rear grip with the handbrake (drift)");
        v.float("steer", &mut self.steer, 5.0, 60.0, "Steering lock (degrees)");
        v.float("climb", &mut self.climb, 0.5, 15.0, "Helicopter climb speed (m/s)");
        v.float("ceiling", &mut self.ceiling, 1.0, 200.0, "Helicopter: highest flying height (m)");
    }
}

/// A drivable vehicle (an entity component).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Vehicle {
    pub def: VehicleDef,
    /// Height of the body centre above the ground it was placed on.
    pub base: f32,
    /// Height of that ground.
    #[serde(default)]
    pub ground: f32,
    pub car: Option<DynamicRayCastVehicleController>,
    pub driver: Option<EntityId>,
    pub steer: f32,
    pub throttle: f32,
    /// Smoothed sideways slide (m/s), for tyre smoke.
    pub drift: f32,
    pub rotor: f32,
    pub rotor_speed: f32,
    /// Helicopter: smoothed tilt (pitch, roll) and heading.
    pub tilt: Vec2,
    pub yaw: f32,
}

/// What the view needs to draw a vehicle.
#[derive(Clone, Debug, PartialEq)]
pub struct VehicleView {
    pub kind: VehicleKind,
    pub half: Vec3,
    pub color: String,
    pub accent: String,
    /// Wheel centres (world), spin angle (radians) and steering angle.
    pub wheels: Vec<(Vec3, f32, f32)>,
    pub rotor: f32,
    pub drift: f32,
    pub driven: bool,
}

impl Vehicle {
    pub fn view(&self) -> VehicleView {
        let wheels = self
            .car
            .as_ref()
            .map(|c| c.wheels().iter().map(|w| (w.center(), w.rotation as f32, w.steering as f32)).collect())
            .unwrap_or_default();
        VehicleView {
            kind: self.def.kind,
            half: self.def.size,
            color: self.def.color.clone(),
            accent: self.def.accent.clone(),
            wheels,
            rotor: self.rotor,
            drift: self.drift,
            driven: self.driver.is_some(),
        }
    }
}

fn wrap(a: f32) -> f32 {
    (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI
}

impl Sim {
    /// Creates a vehicle at `pos` (bottom centre of the body) facing `rot`.
    pub fn spawn_vehicle(
        &mut self,
        name: &str,
        def: VehicleDef,
        pos: Vec3,
        rot: Quat,
        region: Option<crate::statics::RegionKey>,
    ) -> EntityId {
        let half = def.size;
        let wheel_r = (half.y * 1.1).max(0.25);
        let center = pos + Vec3::Y * (half.y + if def.kind == VehicleKind::Car { wheel_r * 0.9 } else { 0.0 });
        let volume = 8.0 * half.x * half.y * half.z;
        let mut v =
            Visual::new(Shape::RoundedBox { half, radius: half.y.min(half.x) * 0.45 }, crate::color::Color::hex(&def.color));
        v.look = crate::shape::Look::Cel;
        let mut sp = Spawn::new(name, center).visual(v).body(BodyKind::Dynamic).rot(rot).density(def.mass / volume).friction(0.4);
        sp.region = region;
        let id = self.spawn(sp);
        let Some(body) = self.state.entities.get(id).and_then(|e| e.body) else { return id };
        if let Some(b) = self.state.physics.bodies.get_mut(body) {
            // Arcade handling: cars only turn about up (no flipping); helicopters are posed by
            // their controller.
            let heli = def.kind == VehicleKind::Helicopter;
            b.set_enabled_rotations(false, !heli, false, true);
            b.set_angular_damping(2.0);
            b.set_linear_damping(0.05);
            if def.kind == VehicleKind::Helicopter {
                b.set_gravity_scale(1.0, true);
            }
        }
        let car = (def.kind == VehicleKind::Car).then(|| {
            let mut c = DynamicRayCastVehicleController::new(body);
            c.index_forward_axis = 2;
            let tuning = WheelTuning {
                suspension_stiffness: 40.0,
                suspension_compression: 0.83 * 4.0,
                suspension_damping: 0.88 * 4.0,
                max_suspension_travel: 0.3,
                side_friction_stiffness: def.grip,
                friction_slip: 3.0,
                max_suspension_force: 60000.0,
            };
            let (x, z) = (half.x - 0.08, half.z * 0.68);
            for (wx, wz) in [(-x, z), (x, z), (-x, -z), (x, -z)] {
                c.add_wheel(Vec3::new(wx, -half.y * 0.4, wz), Vec3::NEG_Y, Vec3::NEG_X, wheel_r * 0.9, wheel_r, &tuning);
            }
            c
        });
        let yaw = {
            let f = rot * Vec3::Z;
            f.x.atan2(f.z)
        };
        if let Some(e) = self.state.entities.get_mut(id) {
            e.vehicle = Some(Box::new(Vehicle {
                base: center.y - pos.y,
                ground: pos.y,
                def,
                car,
                driver: None,
                steer: 0.0,
                throttle: 0.0,
                drift: 0.0,
                rotor: 0.0,
                rotor_speed: 0.0,
                tilt: Vec2::ZERO,
                yaw,
            }));
        }
        id
    }

    /// Puts a vehicle down at `feet` facing `yaw`, stopped (respawns while driving).
    pub fn place_vehicle(&mut self, vid: EntityId, feet: Vec3, yaw: f32) {
        let Some(e) = self.state.entities.get_mut(vid) else { return };
        let Some(v) = e.vehicle.as_mut() else { return };
        v.yaw = yaw;
        v.tilt = Vec2::ZERO;
        let center = feet + Vec3::Y * v.base;
        let rot = Quat::from_rotation_y(yaw);
        e.pos = center;
        e.rot = rot;
        if let Some(b) = e.body.and_then(|h| self.state.physics.bodies.get_mut(h)) {
            b.set_position(Pose::from_parts(center, rot), true);
            b.set_linvel(Vec3::ZERO, true);
            b.set_angvel(Vec3::ZERO, true);
        }
    }

    /// Riders sit in their vehicles (after the physics step moved them).
    pub(crate) fn seat_riders(&mut self) {
        let seats: Vec<(EntityId, Vec3)> = self
            .state
            .entities
            .iter()
            .filter_map(|e| {
                let ch = e.character.as_ref()?;
                let v = self.state.entities.get(ch.riding?)?;
                let half_y = v.vehicle.as_ref()?.def.size.y;
                Some((e.id, v.pos + Vec3::Y * (ch.height() * 0.5 - half_y)))
            })
            .collect();
        for (id, seat) in seats {
            if let Some(e) = self.state.entities.get_mut(id) {
                e.pos = seat;
            }
        }
    }

    /// Gets the player in / out of the nearest vehicle (interact).
    pub(crate) fn vehicle_interact(&mut self, input: &InputFrame) {
        if !input.just(buttons::INTERACT) {
            return;
        }
        let Some(pid) = self.state.player else { return };
        let riding = self.state.entities.get(pid).and_then(|e| e.character.as_ref()).and_then(|c| c.riding);
        match riding {
            Some(vid) => self.exit_vehicle(pid, vid),
            None => {
                let p = self.state.entities.get(pid).map(|e| e.pos).unwrap_or_default();
                let near = self
                    .state
                    .entities
                    .iter()
                    .filter(|e| e.vehicle.as_ref().is_some_and(|v| v.driver.is_none()))
                    .map(|e| (e.id, e.pos.distance(p)))
                    .filter(|(_, d)| *d < 3.2)
                    .min_by(|a, b| a.1.total_cmp(&b.1));
                if let Some((vid, _)) = near {
                    self.enter_vehicle(pid, vid);
                }
            }
        }
    }

    pub fn enter_vehicle(&mut self, who: EntityId, vid: EntityId) {
        let Some(v) = self.state.entities.get_mut(vid).and_then(|e| e.vehicle.as_mut()) else { return };
        v.driver = Some(who);
        if let Some(b) = self.state.entities.get(vid).and_then(|e| e.body).and_then(|h| self.state.physics.bodies.get_mut(h)) {
            b.wake_up(true);
        }
        let body = self.state.entities.get(who).and_then(|e| e.body);
        if let Some(ch) = self.state.entities.get_mut(who).and_then(|e| e.character.as_mut()) {
            ch.riding = Some(vid);
            ch.vel = Vec3::ZERO;
        }
        // The rider's capsule stops colliding while seated.
        if let Some(b) = body.and_then(|h| self.state.physics.bodies.get(h)) {
            for c in b.colliders().to_vec() {
                if let Some(col) = self.state.physics.colliders.get_mut(c) {
                    col.set_sensor(true);
                }
            }
        }
        self.events.push(crate::frame::SimEvent::Pad { pos: self.state.entities.get(vid).map(|e| e.pos).unwrap_or_default() });
    }

    pub fn exit_vehicle(&mut self, who: EntityId, vid: EntityId) {
        let (pos, rot, half) = match self.state.entities.get(vid) {
            Some(e) => (e.pos, e.rot, e.vehicle.as_ref().map(|v| v.def.size).unwrap_or(Vec3::ONE)),
            None => (Vec3::ZERO, Quat::IDENTITY, Vec3::ONE),
        };
        if let Some(v) = self.state.entities.get_mut(vid).and_then(|e| e.vehicle.as_mut()) {
            v.driver = None;
            v.throttle = 0.0;
        }
        let body = self.state.entities.get(who).and_then(|e| e.body);
        if let Some(b) = body.and_then(|h| self.state.physics.bodies.get(h)) {
            for c in b.colliders().to_vec() {
                if let Some(col) = self.state.physics.colliders.get_mut(c) {
                    col.set_sensor(false);
                }
            }
        }
        if let Some(ch) = self.state.entities.get_mut(who).and_then(|e| e.character.as_mut()) {
            ch.riding = None;
        }
        // Step out on the left side, at the vehicle's bottom.
        let side = rot * Vec3::NEG_X;
        let out = pos + side * (half.x + 0.7) - Vec3::Y * half.y;
        self.set_position(who, Vec3::new(out.x, out.y.max(pos.y - half.y - 1.0), out.z));
    }

    /// Drives every vehicle one tick (before the physics step).
    pub(crate) fn drive_vehicles(&mut self, input: &InputFrame, dt: f32) {
        let ids: Vec<EntityId> = self.state.entities.iter().filter(|e| e.vehicle.is_some()).map(|e| e.id).collect();
        let player = self.state.player;
        let gravity = self.config.gravity;
        for id in ids {
            let Some(e) = self.state.entities.map.get_mut(&id) else { continue };
            let (Some(body), Some(mut v)) = (e.body, e.vehicle.take()) else { continue };
            let driven = v.driver.is_some() && v.driver == player;
            let idle = InputFrame::default();
            let inp = if driven { input } else { &idle };
            let ph = &mut self.state.physics;
            match v.def.kind {
                VehicleKind::Car => {
                    let (fwd, speed, lat) = match ph.bodies.get(body) {
                        Some(b) => {
                            let f = *b.rotation() * Vec3::Z;
                            let r = *b.rotation() * Vec3::X;
                            (f, b.linvel().dot(f), b.linvel().dot(r))
                        }
                        None => (Vec3::Z, 0.0, 0.0),
                    };
                    let wish = Vec2::new(inp.move_dir.x, inp.move_dir.y);
                    let mut throttle = 0.0;
                    let mut steer = 0.0;
                    if wish.length() > 0.1 {
                        // Steer toward the stick direction; pull back to reverse.
                        let yaw = fwd.x.atan2(fwd.z);
                        let want = wish.x.atan2(wish.y);
                        let diff = wrap(want - yaw);
                        if diff.abs() < 1.9 || speed > 3.0 {
                            throttle = wish.length().min(1.0) * if diff.abs() < 1.9 { 1.0 } else { -0.4 };
                            steer = (diff / v.def.steer.to_radians().max(0.1)).clamp(-1.0, 1.0);
                        } else {
                            throttle = -0.6 * wish.length().min(1.0);
                            steer = -(wrap(diff - std::f32::consts::PI) / v.def.steer.to_radians().max(0.1)).clamp(-1.0, 1.0);
                        }
                    }
                    v.throttle += (throttle - v.throttle) * (1.0 - (-10.0 * dt).exp());
                    v.steer += (steer - v.steer) * (1.0 - (-12.0 * dt).exp());
                    let handbrake = inp.down(buttons::JUMP);
                    let brake = inp.down(buttons::CROUCH);
                    let lock = v.def.steer.to_radians();
                    if let Some(c) = &mut v.car {
                        let over = speed.abs() > v.def.max_speed && speed.signum() == v.throttle.signum();
                        for (i, w) in c.wheels_mut().iter_mut().enumerate() {
                            let front = i < 2;
                            w.steering = if front { (v.steer * lock) as Real } else { 0.0 };
                            w.engine_force = if front || over { 0.0 } else { (v.throttle * v.def.power) as Real };
                            w.brake = if brake {
                                40.0
                            } else if handbrake && !front {
                                12.0
                            } else if throttle == 0.0 {
                                2.0
                            } else {
                                0.0
                            };
                            w.side_friction_stiffness = if handbrake && !front { v.def.drift_grip } else { v.def.grip } as Real;
                        }
                        let filter = QueryFilter::default().exclude_rigid_body(body).exclude_sensors();
                        let qpm = ph.broad_phase.as_query_pipeline_mut(
                            ph.narrow_phase.query_dispatcher(),
                            &mut ph.bodies,
                            &mut ph.colliders,
                            filter,
                        );
                        c.update_vehicle(dt as Real, qpm);
                    }
                    v.drift += (lat.abs() - v.drift) * (1.0 - (-8.0 * dt).exp());
                }
                VehicleKind::Helicopter => {
                    let Some(b) = ph.bodies.get_mut(body) else {
                        self.state.entities.map.get_mut(&id).unwrap().vehicle = Some(v);
                        continue;
                    };
                    let target_rotor = if v.driver.is_some() { 1.0 } else { 0.0 };
                    v.rotor_speed += (target_rotor - v.rotor_speed) * (1.0 - (-1.5 * dt).exp());
                    let flying = v.rotor_speed > 0.7;
                    b.set_gravity_scale(if flying { 0.0 } else { 1.0 }, true);
                    if flying {
                        let vel = b.linvel();
                        let wish = Vec3::new(inp.move_dir.x, 0.0, inp.move_dir.y).clamp_length_max(1.0) * v.def.max_speed;
                        let dvh = (wish - Vec3::new(vel.x, 0.0, vel.z)).clamp_length_max(v.def.power * dt);
                        // Climbing eases off toward the ceiling.
                        let room = v.ground + v.def.ceiling - (b.translation().y - v.base);
                        let climb = if inp.down(buttons::JUMP) {
                            v.def.climb * room.clamp(0.0, 1.0)
                        } else if inp.down(buttons::CROUCH) {
                            -v.def.climb
                        } else {
                            0.0
                        };
                        let vy = vel.y + (climb - vel.y) * (1.0 - (-4.0 * dt).exp());
                        let nv = Vec3::new(vel.x + dvh.x, vy, vel.z + dvh.z);
                        b.set_linvel(nv, true);
                        // Lean into the motion, turn toward where it is going.
                        let h = Vec2::new(nv.x, nv.z);
                        if h.length() > 1.0 {
                            let want = h.x.atan2(h.y);
                            v.yaw += wrap(want - v.yaw) * (1.0 - (-2.5 * dt).exp());
                        }
                        let accel = dvh / dt.max(1e-4) / v.def.power.max(0.1);
                        let (s, c) = v.yaw.sin_cos();
                        let local = Vec2::new(accel.x * c - accel.z * s, accel.x * s + accel.z * c);
                        let speed_tilt = Vec2::new(h.length() / v.def.max_speed.max(0.1), 0.0);
                        let target = Vec2::new(local.y * 0.25 + speed_tilt.x * 0.18, -local.x * 0.25);
                        v.tilt += (target - v.tilt) * (1.0 - (-5.0 * dt).exp());
                    } else {
                        v.tilt *= (-3.0 * dt).exp();
                    }
                    let rot = Quat::from_rotation_y(v.yaw) * Quat::from_rotation_x(v.tilt.x) * Quat::from_rotation_z(v.tilt.y);
                    b.set_rotation(rot, true);
                    v.rotor = (v.rotor + v.rotor_speed * 28.0 * dt).rem_euclid(std::f32::consts::TAU);
                    let _ = gravity;
                }
            }
            if let Some(e) = self.state.entities.map.get_mut(&id) {
                e.vehicle = Some(v);
            }
        }
    }
}
