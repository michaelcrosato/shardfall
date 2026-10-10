//! Generated creature data. JSON is the adapter format; this module has no compiler process,
//! filesystem or renderer. Definitions are immutable after validation and shareable in snapshots.

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Deref;
use std::sync::{Arc, OnceLock, RwLock};

use glam::{Quat, Vec3};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub use crate::props::Bounds;

pub const FORMAT: u32 = 1;
pub const GENERATOR_REVISION: &str = "851880256987ecdb2895c6afd01f84df64199bdb";
pub const MAX_BONES: usize = 4096;
pub const MAX_VERTICES: usize = 250_000;
pub const MAX_TRIANGLES: usize = 500_000;
pub const MAX_CLIP_VALUES: usize = 32_000_000;
const POSITION_LIMIT: f32 = 10_000.0;

/// Parent-first bones. Bind transforms are in model space; `rest` contains local rotations.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatureBones {
    pub names: Vec<String>,
    pub parents: Vec<i32>,
    pub positions: Vec<f32>,
    pub rotations: Vec<f32>,
    pub lengths: Vec<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rest: Option<Vec<f32>>,
}

/// Flat arrays, matching the compiler's indexing. Colours are linear RGB, without alpha.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatureMesh {
    pub positions: Vec<f32>,
    pub normals: Vec<f32>,
    pub indices: Vec<u32>,
    pub colors: Vec<f32>,
    pub skin_indices: Vec<u16>,
    pub skin_weights: Vec<f32>,
    #[serde(default)]
    pub double_sided: bool,
}

/// One local position and quaternion per bone per frame. The final frame is at `duration`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatureClip {
    pub duration: f32,
    pub looping: bool,
    pub frames: u32,
    pub positions: Vec<f32>,
    pub rotations: Vec<f32>,
    #[serde(default)]
    pub root_motion: bool,
    #[serde(default)]
    pub speed: f32,
    #[serde(default)]
    pub distance: f32,
    #[serde(default)]
    pub events: Vec<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatureSocket {
    pub name: String,
    pub bone: u32,
    pub offset: [f32; 3],
}

/// Versioned numeric adapter output. The editable blueprint is owned by the authoring tools.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreatureData {
    pub format: u32,
    pub generator_revision: String,
    pub name: String,
    #[serde(default)]
    pub title: String,
    pub source_revision: String,
    pub quality: String,
    pub bones: CreatureBones,
    pub meshes: BTreeMap<String, CreatureMesh>,
    #[serde(default)]
    pub clips: BTreeMap<String, CreatureClip>,
    pub bounds: Bounds,
    #[serde(default)]
    pub sockets: Vec<CreatureSocket>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// A validated, immutable definition. Field access uses `Deref`; there is no mutable deref.
/// Serde writes only `CreatureData` and validates it again when reading a saved snapshot.
#[derive(Clone, Debug)]
pub struct CreatureAsset {
    data: CreatureData,
    revision: String,
    bind_positions: Vec<Vec3>,
    inverse_bind_rotations: Vec<Quat>,
    rest_positions: Vec<Vec3>,
    rest_rotations: Vec<Quat>,
}

impl Deref for CreatureAsset {
    type Target = CreatureData;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl Serialize for CreatureAsset {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.data.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for CreatureAsset {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::from_data(CreatureData::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Current bone transforms in model space, suitable for CPU skinning and sockets.
#[derive(Clone, Debug)]
pub struct CreaturePose {
    pub positions: Vec<Vec3>,
    pub rotations: Vec<Quat>,
}

#[derive(Clone, Debug)]
pub struct SkinnedMesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
}

fn triple(values: &[f32], index: usize) -> Vec3 {
    Vec3::from_slice(&values[index * 3..index * 3 + 3])
}

fn rotation(values: &[f32], index: usize) -> Quat {
    Quat::from_slice(&values[index * 4..index * 4 + 4]).normalize()
}

fn label(value: &str, path: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 160 || value.chars().any(char::is_control) {
        return Err(format!("{path} must have 1 to 160 printable characters"));
    }
    Ok(())
}

fn array_len(actual: usize, expected: usize, path: &str) -> Result<(), String> {
    if actual != expected {
        return Err(format!("{path} needs {expected} numbers, got {actual}"));
    }
    Ok(())
}

fn finite(values: &[f32], min: f32, max: f32, path: &str) -> Result<(), String> {
    if let Some(index) = values.iter().position(|v| !v.is_finite() || !(min..=max).contains(v)) {
        return Err(format!("{path}[{index}] must be finite and between {min} and {max}"));
    }
    Ok(())
}

fn quaternions(values: &[f32], count: usize, path: &str) -> Result<(), String> {
    array_len(values.len(), count * 4, path)?;
    finite(values, -1.01, 1.01, path)?;
    for (index, value) in values.chunks_exact(4).enumerate() {
        let norm: f32 = value.iter().map(|v| v * v).sum();
        if (norm - 1.0).abs() > 0.01 {
            return Err(format!("{path} quaternion {index} must have unit length"));
        }
    }
    Ok(())
}

impl CreatureData {
    pub fn validate(&self) -> Result<(), String> {
        if self.format != FORMAT {
            return Err(format!("unsupported creature format {}; expected {FORMAT}", self.format));
        }
        if self.generator_revision != GENERATOR_REVISION {
            return Err(format!("unsupported generator revision; expected {GENERATOR_REVISION}"));
        }
        crate::props::validate_name(&self.name).map_err(|e| format!("name: {e}"))?;
        label(&self.source_revision, "source_revision")?;
        if self.title.len() > 256 || self.title.chars().any(char::is_control) {
            return Err("title must have at most 256 printable characters".into());
        }
        if !["low", "medium", "high"].contains(&self.quality.as_str()) {
            return Err("quality must be low, medium or high".into());
        }
        if !self.bounds.min.is_finite()
            || !self.bounds.max.is_finite()
            || self.bounds.min.abs().max_element() > POSITION_LIMIT
            || self.bounds.max.abs().max_element() > POSITION_LIMIT
            || self.bounds.min.cmpgt(self.bounds.max).any()
            || self.bounds.size().max_element() <= 0.0
        {
            return Err("bounds must have finite ordered min/max within 10000 metres and nonzero size".into());
        }
        let bones = &self.bones;
        let count = bones.names.len();
        if !(1..=MAX_BONES).contains(&count) {
            return Err(format!("bones must contain 1 to {MAX_BONES} named bones"));
        }
        array_len(bones.parents.len(), count, "bones.parents")?;
        array_len(bones.positions.len(), count * 3, "bones.positions")?;
        array_len(bones.lengths.len(), count, "bones.lengths")?;
        finite(&bones.positions, -POSITION_LIMIT, POSITION_LIMIT, "bones.positions")?;
        finite(&bones.lengths, 0.0, POSITION_LIMIT, "bones.lengths")?;
        quaternions(&bones.rotations, count, "bones.rotations")?;
        if let Some(rest) = &bones.rest {
            quaternions(rest, count, "bones.rest")?;
        }
        let mut names = BTreeSet::new();
        for (index, name) in bones.names.iter().enumerate() {
            label(name, "bone name")?;
            if !names.insert(name) {
                return Err(format!("duplicate bone name '{name}'"));
            }
            let parent = bones.parents[index];
            if parent < -1 || parent >= index as i32 || (index == 0 && parent != -1) {
                return Err(format!("bones.parents[{index}] must be -1 or an earlier bone index"));
            }
        }
        let mut vertices = 0;
        let mut triangles = 0;
        for (name, mesh) in &self.meshes {
            if !["skin", "parts", "eyes", "membranes"].contains(&name.as_str()) {
                return Err(format!("unknown mesh stream '{name}'; use skin, parts, eyes or membranes"));
            }
            if mesh.positions.len() % 3 != 0 || mesh.indices.len() % 3 != 0 {
                return Err(format!("meshes.{name}: positions and triangle indices must have lengths divisible by 3"));
            }
            let n = mesh.positions.len() / 3;
            vertices += n;
            triangles += mesh.indices.len() / 3;
            if vertices > MAX_VERTICES || triangles > MAX_TRIANGLES {
                return Err(format!("creature exceeds {MAX_VERTICES} vertices or {MAX_TRIANGLES} triangles"));
            }
            for (field, actual, expected) in [
                ("normals", mesh.normals.len(), n * 3),
                ("colors", mesh.colors.len(), n * 3),
                ("skin_indices", mesh.skin_indices.len(), n * 4),
                ("skin_weights", mesh.skin_weights.len(), n * 4),
            ] {
                array_len(actual, expected, &format!("meshes.{name}.{field}"))?;
            }
            finite(&mesh.positions, -POSITION_LIMIT, POSITION_LIMIT, &format!("meshes.{name}.positions"))?;
            finite(&mesh.normals, -1.01, 1.01, &format!("meshes.{name}.normals"))?;
            finite(&mesh.colors, 0.0, 1.0, &format!("meshes.{name}.colors"))?;
            finite(&mesh.skin_weights, 0.0, 1.0, &format!("meshes.{name}.skin_weights"))?;
            if mesh.indices.iter().any(|i| *i as usize >= n) {
                return Err(format!("meshes.{name}.indices refers to a missing vertex"));
            }
            if mesh.skin_indices.iter().any(|i| *i as usize >= count) {
                return Err(format!("meshes.{name}.skin_indices refers to a missing bone"));
            }
            for (i, weights) in mesh.skin_weights.chunks_exact(4).enumerate() {
                if (weights.iter().sum::<f32>() - 1.0).abs() > 0.002 {
                    return Err(format!("meshes.{name}.skin_weights at vertex {i} must sum to 1"));
                }
            }
            for (i, normal) in mesh.normals.chunks_exact(3).enumerate() {
                let length: f32 = normal.iter().map(|v| v * v).sum();
                if !(0.5..=1.5).contains(&length) {
                    return Err(format!("meshes.{name}.normals at vertex {i} must be a nonzero unit direction"));
                }
            }
        }
        if triangles == 0 {
            return Err("creature needs at least one mesh triangle".into());
        }
        if self.clips.len() > 128 {
            return Err("creature has more than 128 clips".into());
        }
        let mut clip_values = 0usize;
        for (name, clip) in &self.clips {
            label(name, "clip name")?;
            if name == "rest" {
                return Err("clip name 'rest' is reserved for the rest pose".into());
            }
            if !clip.duration.is_finite() || !(0.001..=3600.0).contains(&clip.duration) {
                return Err(format!("clips.{name}.duration must be between 0.001 and 3600 seconds"));
            }
            if !(2..=108_001).contains(&clip.frames) {
                return Err(format!("clips.{name}.frames must be between 2 and 108001"));
            }
            let poses = count * clip.frames as usize;
            clip_values = clip_values.saturating_add(clip.positions.len()).saturating_add(clip.rotations.len());
            if clip_values > MAX_CLIP_VALUES {
                return Err(format!("creature clips exceed {MAX_CLIP_VALUES} numeric values"));
            }
            array_len(clip.positions.len(), poses * 3, &format!("clips.{name}.positions"))?;
            finite(&clip.positions, -POSITION_LIMIT, POSITION_LIMIT, &format!("clips.{name}.positions"))?;
            quaternions(&clip.rotations, poses, &format!("clips.{name}.rotations"))?;
            finite(&[clip.speed, clip.distance], 0.0, POSITION_LIMIT, &format!("clips.{name}.speed/distance"))?;
            if clip.events.len() > 10_000 {
                return Err(format!("clips.{name}.events has more than 10000 entries"));
            }
        }
        if self.sockets.len() > 4096 {
            return Err("creature has more than 4096 sockets".into());
        }
        let mut socket_names = BTreeSet::new();
        for socket in &self.sockets {
            label(&socket.name, "socket name")?;
            if !socket_names.insert(&socket.name) {
                return Err(format!("duplicate socket name '{}'", socket.name));
            }
            if socket.bone as usize >= count {
                return Err(format!("socket '{}' refers to a missing bone", socket.name));
            }
            finite(&socket.offset, -POSITION_LIMIT, POSITION_LIMIT, "socket.offset")?;
        }
        if self.warnings.len() > 1024 || self.warnings.iter().any(|w| w.len() > 4096) {
            return Err("warnings exceed 1024 entries or 4096 bytes per entry".into());
        }
        Ok(())
    }
}

impl CreatureAsset {
    pub fn parse(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| e.to_string())
    }

    pub fn from_data(data: CreatureData) -> Result<Self, String> {
        data.validate()?;
        let bytes = serde_json::to_vec(&data).map_err(|e| e.to_string())?;
        let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3));
        let revision = format!("creature1-{hash:016x}");
        let bones = &data.bones;
        let count = bones.names.len();
        let bind_positions: Vec<_> = (0..count).map(|i| triple(&bones.positions, i)).collect();
        let bind_rotations: Vec<_> = (0..count).map(|i| rotation(&bones.rotations, i)).collect();
        let inverse_bind_rotations: Vec<_> = bind_rotations.iter().map(|q| q.conjugate()).collect();
        let mut rest_positions = Vec::with_capacity(count);
        let mut rest_rotations = Vec::with_capacity(count);
        for i in 0..count {
            let parent = bones.parents[i];
            let (p, q) = if parent >= 0 {
                let parent = parent as usize;
                (
                    inverse_bind_rotations[parent] * (bind_positions[i] - bind_positions[parent]),
                    inverse_bind_rotations[parent] * bind_rotations[i],
                )
            } else {
                (bind_positions[i], bind_rotations[i])
            };
            rest_positions.push(p);
            rest_rotations.push(bones.rest.as_ref().map(|rest| rotation(rest, i)).unwrap_or(q));
        }
        Ok(Self { data, revision, bind_positions, inverse_bind_rotations, rest_positions, rest_rotations })
    }

    pub fn validate(&self) -> Result<(), String> {
        self.data.validate()
    }

    pub fn revision(&self) -> String {
        self.revision.clone()
    }

    pub fn vertex_count(&self) -> usize {
        self.meshes.values().map(|mesh| mesh.positions.len() / 3).sum()
    }

    pub fn triangle_count(&self) -> usize {
        self.meshes.values().map(|mesh| mesh.indices.len() / 3).sum()
    }

    /// `None` selects the rest pose. In-range endpoints remain exact; times outside a loop
    /// wrap, while non-looping times clamp. Interpolated rotations take the shortest arc.
    pub fn sample_pose(&self, clip: Option<&str>, time: f32, looping: bool) -> Result<CreaturePose, String> {
        if !time.is_finite() {
            return Err("pose time must be finite".into());
        }
        let clip = clip.map(|name| self.clips.get(name).ok_or_else(|| format!("no creature clip '{name}'"))).transpose()?;
        let count = self.bones.names.len();
        let mut positions = Vec::with_capacity(count);
        let mut rotations = Vec::with_capacity(count);
        let sample = clip.map(|clip| {
            let t = if looping && (time < 0.0 || time > clip.duration) {
                time.rem_euclid(clip.duration)
            } else {
                time.clamp(0.0, clip.duration)
            };
            let f = t / clip.duration * (clip.frames - 1) as f32;
            let a = (f.floor() as usize).min(clip.frames as usize - 1);
            (clip, a, (a + 1).min(clip.frames as usize - 1), (f - a as f32).clamp(0.0, 1.0))
        });
        for i in 0..count {
            let (p, q) = match sample {
                Some((clip, a, b, t)) => (
                    triple(&clip.positions, a * count + i).lerp(triple(&clip.positions, b * count + i), t),
                    rotation(&clip.rotations, a * count + i).slerp(rotation(&clip.rotations, b * count + i), t).normalize(),
                ),
                None => (self.rest_positions[i], self.rest_rotations[i]),
            };
            let parent = self.bones.parents[i];
            if parent < 0 {
                positions.push(p);
                rotations.push(q);
            } else {
                let parent = parent as usize;
                positions.push(positions[parent] + rotations[parent] * p);
                rotations.push((rotations[parent] * q).normalize());
            }
        }
        Ok(CreaturePose { positions, rotations })
    }

    /// Skin positions and directions in model space. Four rigid transforms are blended; a
    /// translation never changes a normal, and directions are normalized after blending.
    pub fn skin(&self, pose: &CreaturePose) -> Result<BTreeMap<String, SkinnedMesh>, String> {
        let count = self.bones.names.len();
        if pose.positions.len() != count
            || pose.rotations.len() != count
            || pose.positions.iter().any(|p| !p.is_finite())
            || pose.rotations.iter().any(|q| !q.is_finite() || !(0.5..=1.5).contains(&q.length_squared()))
        {
            return Err("pose must have one finite position and rotation per bone".into());
        }
        let turns: Vec<_> = (0..count).map(|i| pose.rotations[i].normalize() * self.inverse_bind_rotations[i]).collect();
        let mut out = BTreeMap::new();
        for (name, mesh) in &self.meshes {
            if mesh.indices.is_empty() {
                continue;
            }
            let n = mesh.positions.len() / 3;
            let mut positions = Vec::with_capacity(n);
            let mut normals = Vec::with_capacity(n);
            for i in 0..n {
                let p = triple(&mesh.positions, i);
                let normal = triple(&mesh.normals, i);
                let mut position = Vec3::ZERO;
                let mut direction = Vec3::ZERO;
                let mut sum = 0.0;
                for k in 0..4 {
                    let weight = mesh.skin_weights[i * 4 + k];
                    if weight == 0.0 {
                        continue;
                    }
                    let bone = mesh.skin_indices[i * 4 + k] as usize;
                    position += (turns[bone] * (p - self.bind_positions[bone]) + pose.positions[bone]) * weight;
                    direction += turns[bone] * normal * weight;
                    sum += weight;
                }
                positions.push(position / sum);
                normals.push(direction.normalize_or(Vec3::Y));
            }
            out.insert(name.clone(), SkinnedMesh { positions, normals });
        }
        Ok(out)
    }

    pub fn socket_position(&self, name: &str, pose: &CreaturePose) -> Option<Vec3> {
        let socket = self.sockets.iter().find(|socket| socket.name == name)?;
        let bone = socket.bone as usize;
        Some(*pose.positions.get(bone)? + *pose.rotations.get(bone)? * Vec3::from_array(socket.offset))
    }
}

#[derive(Clone, Debug, Default)]
pub struct CreatureLibrary {
    pub assets: BTreeMap<String, Arc<CreatureAsset>>,
}

static LIBRARY: OnceLock<RwLock<Arc<CreatureLibrary>>> = OnceLock::new();

fn shared() -> &'static RwLock<Arc<CreatureLibrary>> {
    LIBRARY.get_or_init(|| RwLock::new(Arc::new(CreatureLibrary::default())))
}

pub fn canonical(name: &str) -> Result<String, String> {
    let (prefix, leaf) = name.split_once('/').unwrap_or(("CREATURE", name));
    if !prefix.eq_ignore_ascii_case("CREATURE") {
        return Err("creature names use CREATURE/name".into());
    }
    let leaf = leaf.to_ascii_lowercase();
    crate::props::validate_name(&leaf)?;
    Ok(format!("CREATURE/{leaf}"))
}

pub fn library() -> Arc<CreatureLibrary> {
    shared().read().unwrap().clone()
}

pub fn get(name: &str) -> Option<Arc<CreatureAsset>> {
    library().assets.get(&canonical(name).ok()?).cloned()
}

pub fn install(name: &str, asset: Arc<CreatureAsset>) -> Result<(), String> {
    let name = canonical(name)?;
    if name.split_once('/').map(|(_, leaf)| leaf) != Some(asset.name.as_str()) {
        return Err(format!("{name} does not match creature name '{}'", asset.name));
    }
    // Every constructor and Deserialize validate; immutable assets cannot become invalid.
    let mut current = shared().write().unwrap();
    let mut next = (**current).clone();
    next.assets.insert(name, asset);
    *current = Arc::new(next);
    Ok(())
}

pub fn remove(name: &str) -> Result<bool, String> {
    let name = canonical(name)?;
    let mut current = shared().write().unwrap();
    if !current.assets.contains_key(&name) {
        return Ok(false);
    }
    let mut next = (**current).clone();
    next.assets.remove(&name);
    *current = Arc::new(next);
    Ok(true)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn fixture(name: &str) -> serde_json::Value {
        json!({
            "format": FORMAT, "generator_revision": GENERATOR_REVISION,
            "name": name, "title": "Jointed test", "source_revision": "test-source", "quality": "low",
            "bones": {"names":["root","tip"],"parents":[-1,0],
                "positions":[0,0,0,1,0,0],"rotations":[0,0,0,1,0,0,0,1],"lengths":[1,1]},
            "meshes": {"skin":{"positions":[1,0,0,2,0,0,1,1,0],"normals":[0,0,1,0,0,1,0,0,1],
                "indices":[0,1,2],"colors":[1,0,0,0,1,0,0,0,1],
                "skin_indices":[1,0,0,0,1,0,0,0,1,0,0,0],"skin_weights":[1,0,0,0,1,0,0,0,1,0,0,0]}},
            "bounds":{"min":[1,0,0],"max":[2,1,0]},
            "clips":{"bend":{"duration":1,"looping":false,"frames":2,
                "positions":[0,0,0,1,0,0,0,0,0,1,0,0],
                "rotations":[0,0,0,1,0,0,0,1,0,0,0,1,0,0,1,0]}},
            "sockets":[{"name":"end","bone":1,"offset":[1,0,0]}]
        })
    }

    #[test]
    fn bad_hierarchy_weights_and_versions_are_rejected_before_publication() {
        let valid = fixture("validation-fixture");
        let good: CreatureAsset = serde_json::from_value(valid.clone()).unwrap();
        install("validation-fixture", Arc::new(good)).unwrap();
        let before = get("validation-fixture").unwrap();
        for path in ["parent", "weight", "version", "rotation", "index", "unknown"] {
            let mut bad = valid.clone();
            match path {
                "parent" => bad["bones"]["parents"][1] = json!(1),
                "weight" => bad["meshes"]["skin"]["skin_weights"][0] = json!(0.2),
                "version" => bad["generator_revision"] = json!("other"),
                "rotation" => bad["bones"]["rotations"][3] = json!(0),
                "index" => bad["meshes"]["skin"]["indices"][0] = json!(99),
                _ => bad["unrecognized"] = json!(true),
            }
            assert!(serde_json::from_value::<CreatureAsset>(bad).is_err(), "{path}");
        }
        assert!(Arc::ptr_eq(&before, &get("validation-fixture").unwrap()));
        remove("validation-fixture").unwrap();
    }

    #[test]
    fn bind_rest_and_sampled_skin_preserve_normals_and_socket_coordinates() {
        let value = fixture("pose-fixture");
        let asset: CreatureAsset = serde_json::from_value(value.clone()).unwrap();
        let pose = asset.sample_pose(None, 0.0, false).unwrap();
        let skinned = asset.skin(&pose).unwrap();
        assert!((skinned["skin"].positions[1] - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-5);
        let mut invalid = pose.clone();
        invalid.rotations[0] = Quat::from_xyzw(f32::MAX, 0.0, 0.0, 1.0);
        assert!(asset.skin(&invalid).is_err());
        let half = asset.sample_pose(Some("bend"), 0.5, false).unwrap();
        let skinned = asset.skin(&half).unwrap();
        assert!((skinned["skin"].positions[1] - Vec3::new(1.0, 1.0, 0.0)).length() < 1e-5);
        assert!((skinned["skin"].normals[1] - Vec3::Z).length() < 1e-5);
        assert!((asset.socket_position("end", &half).unwrap() - Vec3::new(1.0, 1.0, 0.0)).length() < 1e-5);
        let wrapped = asset.sample_pose(Some("bend"), 1.5, true).unwrap();
        assert!((wrapped.rotations[1].dot(half.rotations[1]).abs() - 1.0).abs() < 1e-5);
        let end = asset.sample_pose(Some("bend"), 1.0, true).unwrap();
        assert!((asset.skin(&end).unwrap()["skin"].positions[1] - Vec3::ZERO).length() < 1e-5);
        let mut folded = value;
        folded["bones"]["rest"] = json!([0, 0, 0, 1, 0, 0, 1, 0]);
        let folded: CreatureAsset = serde_json::from_value(folded).unwrap();
        let skin = folded.skin(&folded.sample_pose(None, 0.0, false).unwrap()).unwrap();
        assert!((skin["skin"].positions[1] - Vec3::ZERO).length() < 1e-5);
    }

    #[test]
    fn snapshot_round_trip_keeps_revision_and_independent_registry_arcs() {
        let asset: CreatureAsset = serde_json::from_value(fixture("roundtrip-fixture")).unwrap();
        let revision = asset.revision();
        let restored = CreatureAsset::parse(&serde_json::to_string(&asset).unwrap()).unwrap();
        assert_eq!(restored.revision(), revision);
        let first = Arc::new(restored);
        install("roundtrip-fixture", first.clone()).unwrap();
        let other: CreatureAsset = serde_json::from_value(fixture("other-fixture")).unwrap();
        install("other-fixture", Arc::new(other)).unwrap();
        assert!(Arc::ptr_eq(&first, &get("roundtrip-fixture").unwrap()));
        remove("roundtrip-fixture").unwrap();
        remove("other-fixture").unwrap();
    }
}
