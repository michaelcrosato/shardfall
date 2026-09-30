//! Shapes shared by physics colliders and visuals, and the visual description of objects.

use glam::Vec3;
use rapier::prelude::*;
use serde::{Deserialize, Serialize};

use crate::choice_enum;
use crate::color::Color;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Shape {
    Box {
        half: Vec3,
    },
    RoundedBox {
        half: Vec3,
        radius: f32,
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
    pub fn collider(&self) -> ColliderBuilder {
        match *self {
            Shape::Box { half } => ColliderBuilder::cuboid(half.x as Real, half.y as Real, half.z as Real),
            Shape::RoundedBox { half, radius } => {
                let r = radius.min(half.min_element() * 0.99);
                ColliderBuilder::round_cuboid((half.x - r) as Real, (half.y - r) as Real, (half.z - r) as Real, r as Real)
            }
            Shape::Sphere { radius } => ColliderBuilder::ball(radius as Real),
            Shape::Capsule { half_height, radius } => ColliderBuilder::capsule_y(half_height as Real, radius as Real),
            Shape::Cylinder { half_height, radius } => ColliderBuilder::cylinder(half_height as Real, radius as Real),
        }
    }

    /// Half extents of the axis-aligned local bounds.
    pub fn half_extents(&self) -> Vec3 {
        match *self {
            Shape::Box { half } | Shape::RoundedBox { half, .. } => half,
            Shape::Sphere { radius } => Vec3::splat(radius),
            Shape::Capsule { half_height, radius } => Vec3::new(radius, half_height + radius, radius),
            Shape::Cylinder { half_height, radius } => Vec3::new(radius, half_height, radius),
        }
    }
}

choice_enum! {
    /// Surface style (mirrors the renderer's styles).
    #[derive(Default)]
    pub enum Look {
        Flat => "flat",
        #[default]
        Cel => "cel",
        Lit => "lit",
        Unlit => "unlit",
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Visual {
    pub shape: Shape,
    pub color: Color,
    #[serde(default)]
    pub look: Look,
    #[serde(default)]
    pub emissive: f32,
}

impl Visual {
    pub fn new(shape: Shape, color: Color) -> Self {
        Self { shape, color, look: Look::Cel, emissive: 0.0 }
    }
}
