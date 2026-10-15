//! Library assets drawn from game code. A game that keeps its own state (units, shots, scenery
//! it moves itself) draws an asset each frame with one call, `emit_asset`, the way the view
//! draws placed props: the same parts, generated meshes, glow and lights.
//!
//! ```ignore
//! // In the game's view, while building the frame's scene (as `arpg.rs` draws Shardfall):
//! for unit in &game.units {
//!     let at = Placement::at(unit.pos).yaw(unit.heading).id(unit.id);
//!     pav_view::assets::emit_asset(scene, "BUILTIN/crate", at, unit.id % 8, &palette)?;
//! }
//! ```

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use glam::{Quat, Vec3};
use pav_core::fxdef::LightDef;
use pav_core::prop_instance::{validate_roles, validate_scale};
use pav_core::shape::Visual;
use pav_core::{Color, EntityId, RenderObject, Shape, props};
use pav_render::Scene;
use pav_render::scene::MeshKey;

use pav_core::material::Library;

use crate::build::{GeneratedMeshes, StyleOverride, Surfaces, emit_prim, part_color, role_hex};
use crate::materials::{self, MaterialTable};

/// Colours for the theme roles an asset's parts name (`"$wall"` → `"#5a6b7c"`). Roles left out
/// take `props::DEFAULT_ROLES`; an empty palette draws the asset as authored.
pub type Palette = BTreeMap<String, String>;

/// Where an asset is drawn: its origin (the floor under it, as authored) in the world, its
/// turn, a uniform scale, and the id of the thing it draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub pos: Vec3,
    pub rot: Quat,
    pub scale: f32,
    /// The thing's id: its outline group (`id + 2`, as an entity's) and its lights' flicker
    /// phase. Give each unit its own, so outlines separate units that overlap.
    pub id: u32,
}

impl Placement {
    /// At `pos`, unturned, full size, id 0.
    pub fn at(pos: Vec3) -> Self {
        Self { pos, rot: Quat::IDENTITY, scale: 1.0, id: 0 }
    }

    /// Turned `degrees` about +y: +yaw turns the asset's front (+z) toward +x, as room objects.
    pub fn yaw(self, degrees: f32) -> Self {
        Self { rot: Quat::from_rotation_y(degrees.to_radians()), ..self }
    }

    pub fn rot(self, rot: Quat) -> Self {
        Self { rot, ..self }
    }

    pub fn scale(self, scale: f32) -> Self {
        Self { scale, ..self }
    }

    pub fn id(self, id: u32) -> Self {
        Self { id, ..self }
    }
}

/// The frame the view is building: its whole-scene style and its material table. Drawn assets
/// take both, as placed copies do.
#[derive(Clone)]
struct Frame {
    style: StyleOverride,
    materials: Arc<Mutex<MaterialTable>>,
}

thread_local! {
    static FRAME: RefCell<Option<Frame>> = const { RefCell::new(None) };
}

/// Sets the frame `emit_asset` draws into until it drops (the view, while it builds a frame).
pub(crate) struct FrameScope(Option<Frame>);

impl FrameScope {
    pub(crate) fn new(style: StyleOverride, materials: Arc<Mutex<MaterialTable>>) -> Self {
        Self(FRAME.replace(Some(Frame { style, materials })))
    }
}

impl Drop for FrameScope {
    fn drop(&mut self) {
        FRAME.set(self.0.take());
    }
}

/// Generated meshes kept across calls and frames (they depend only on the shape).
static MESHES: Mutex<Option<GeneratedMeshes>> = Mutex::new(None);
/// Meshes kept before the cache starts over.
const KEEP: usize = 1024;

/// Draws the asset `name` (`LIB/core/barrel`, `BUILTIN/crate`, `WORKSHOP/tank`; a family's
/// preset as `LIB/core/fence:low`) into `scene`
/// for this frame at `at`: its parts, their glow and its lights, as a placed copy draws them.
/// `seed` picks the variant (`chance`, `jitter`, `pick`; 0 as authored; each asset keeps 64
/// seeds compiled, so use a few, e.g. `id % 8`). `palette` colours its `$roles`. Particles
/// (`fx`) need a placed prop. Call it while the view builds the frame's scene, so filters aimed
/// at objects and a whole-scene style reach it. Errors: an unknown name (with the nearest one), a bad scale, position
/// or palette.
pub fn emit_asset(scene: &mut Scene, name: &str, at: Placement, seed: u32, palette: &Palette) -> Result<(), String> {
    let (asset, def, params) = props::resolve_placed(name, &props::Params::new())?;
    validate_scale(at.scale)?;
    if !at.pos.is_finite() || !at.rot.is_finite() || at.rot.length_squared() < 1e-6 {
        return Err(format!("{name}: placement needs a finite pos and rotation"));
    }
    validate_roles(palette)?;
    // A turn that drifted from unit length (accumulated in game code) is renormalised.
    let rot = if (at.rot.length_squared() - 1.0).abs() > 1e-4 { at.rot.normalize() } else { at.rot };
    let parts = def.variant_with(&params, seed);
    let o = RenderObject {
        id: EntityId(at.id),
        pos: at.pos,
        rot,
        visual: Visual::new(Shape::Sphere { radius: 0.0 }, Color::WHITE),
        prop: None,
        puppet: None,
        pulse: -1.0,
        soft: None,
        vehicle: None,
        cone: None,
        scenery: false,
    };
    let mut cache = MESHES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let meshes = cache.get_or_insert_with(GeneratedMeshes::default);
    if meshes.built.len() > KEEP {
        meshes.built.clear();
    }
    // A generated mesh goes into the scene once, however many things draw it.
    meshes.sent = scene
        .custom_meshes
        .iter()
        .filter_map(|(key, _)| match key {
            MeshKey::Custom(key) => Some(*key),
            _ => None,
        })
        .collect();
    // Outside a frame (or while the view holds its table) parts draw in their colours alone.
    let frame = FRAME.with_borrow(Clone::clone);
    let style = frame.as_ref().map_or(StyleOverride::PerObject, |frame| frame.style);
    let mut table = frame.as_ref().and_then(|frame| frame.materials.try_lock().ok());
    let library = Library::current();
    for (index, part) in parts.parts.iter().enumerate().filter(|(_, part)| !part.hidden) {
        let (color, material) = match table.as_deref_mut() {
            Some(table) => Surfaces { table, library: &library, asset: &asset, roles: palette }
                .part(part, materials::seed(at.id, index as u32)),
            None => (part_color(part.surface_color(), part.tint, palette), 0),
        };
        emit_prim(scene, meshes, part, &o, at.scale, style, color, material);
    }
    for (i, light) in parts.lights.iter().enumerate() {
        let def = LightDef {
            color: role_hex(&light.color, palette).to_owned(),
            radius: light.radius * at.scale,
            intensity: light.intensity,
            flicker: light.flicker,
            pulse: light.pulse,
            shadows: light.shadows,
            offset: Vec3::ZERO,
        };
        let pos = at.pos + rot * (light.pos * at.scale);
        scene.point_lights.push(crate::fx::light(&def, pos, scene.time, at.id as f32 * 1.7 + i as f32));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ViewBuilder;
    use crate::build::ViewSettings;
    use pav_core::prop_instance::PropFrame;
    use pav_core::props::{Doc, PropDef};
    use std::sync::Arc;

    /// A fixture installed as `WORKSHOP/<name>`.
    fn install(text: &str, name: &str) -> Arc<PropDef> {
        let mut doc = Doc::read(text).unwrap();
        doc.name = name.into();
        props::install(&format!("WORKSHOP/{name}"), PropDef::new(doc).unwrap()).unwrap();
        props::get(&format!("WORKSHOP/{name}")).unwrap()
    }

    /// What the view draws for a placed copy with entity id `id`.
    fn placed(name: &str, def: Arc<PropDef>, at: Placement, palette: &Palette) -> Scene {
        let o = RenderObject {
            id: EntityId(at.id),
            pos: at.pos,
            rot: at.rot,
            visual: Visual::new(Shape::Sphere { radius: 0.0 }, Color::WHITE),
            prop: Some(PropFrame {
                asset: name.to_owned(),
                parts: def.compiled.clone(),
                definition: def,
                scale: at.scale,
                roles: palette.clone(),
            }),
            puppet: None,
            pulse: -1.0,
            soft: None,
            vehicle: None,
            cone: None,
            scenery: false,
        };
        let mut scene = Scene::default();
        let settings = ViewSettings { particles: false, ..Default::default() };
        ViewBuilder::new().emit_prop(&mut scene, &o, o.prop.as_ref().unwrap(), &settings, 0.0, 0.0);
        scene
    }

    fn summary(scene: &Scene) -> String {
        let meshes = scene.meshes.iter().map(|m| (m.mesh, m.transform, m.color, m.emissive, m.group));
        let sdfs = scene.sdfs.iter().map(|s| (s.a, s.b, s.ra, s.rb, s.color, s.group));
        let lights = scene.point_lights.iter().map(|l| (l.position, l.color, l.radius));
        format!("{:?}\n{:?}\n{:?}", meshes.collect::<Vec<_>>(), sdfs.collect::<Vec<_>>(), lights.collect::<Vec<_>>())
    }

    #[test]
    fn a_game_draws_an_asset_as_a_placed_copy_draws_it() {
        let torch = install(include_str!("../../pav_core/tests/fixtures/props/format2/wall_torch.json"), "view_emit_torch");
        let stall = install(include_str!("../../pav_core/tests/fixtures/props/format2/market_stall.json"), "view_emit_stall");
        let palette: Palette = [("$light".to_owned(), "#40a0ff".to_owned())].into();
        for (name, def) in [
            ("BUILTIN/lantern", props::get("BUILTIN/lantern").unwrap()),
            ("WORKSHOP/view_emit_torch", torch),
            ("WORKSHOP/view_emit_stall", stall),
        ] {
            let at = Placement::at(Vec3::new(3.0, 0.5, -2.0)).yaw(30.0).scale(1.5).id(41);
            let mut scene = Scene::default();
            emit_asset(&mut scene, name, at, 0, &palette).unwrap();
            assert!(!scene.meshes.is_empty() || !scene.sdfs.is_empty(), "{name}");
            assert_eq!(summary(&scene), summary(&placed(name, def, at, &palette)), "{name}");
        }
        let mut scene = Scene::default();
        emit_asset(&mut scene, "WORKSHOP/view_emit_torch", Placement::at(Vec3::ZERO), 0, &Palette::new()).unwrap();
        assert!(!scene.point_lights.is_empty(), "the torch lights the scene");
        for name in ["WORKSHOP/view_emit_torch", "WORKSHOP/view_emit_stall"] {
            props::remove(name).unwrap();
        }
    }

    fn table() -> Arc<Mutex<MaterialTable>> {
        Arc::new(Mutex::new(MaterialTable::default()))
    }

    #[test]
    fn a_whole_scene_style_reaches_drawn_assets() {
        let styles = |style| {
            let _scope = FrameScope::new(style, table());
            let mut scene = Scene::default();
            emit_asset(&mut scene, "BUILTIN/lantern", Placement::at(Vec3::ZERO), 0, &Palette::new()).unwrap();
            scene.meshes.iter().map(|m| m.style).chain(scene.sdfs.iter().map(|s| s.style)).collect::<Vec<_>>()
        };
        let cel = styles(StyleOverride::Cel);
        assert!(!cel.is_empty() && cel.iter().all(|s| *s == pav_render::scene::Style::Cel), "{cel:?}");
        assert!(FRAME.with_borrow(Option::is_none), "the scope ends with the frame");
    }

    #[test]
    fn drawn_assets_wear_their_materials_from_the_frames_table() {
        install(include_str!("../../pav_core/tests/fixtures/props/materials/banded_barrel.json"), "view_emit_barrel");
        let materials = table();
        let draw = |frame: bool| {
            let _scope = frame.then(|| FrameScope::new(StyleOverride::PerObject, materials.clone()));
            let mut scene = Scene::default();
            let at = Placement::at(Vec3::ZERO).id(7);
            emit_asset(&mut scene, "WORKSHOP/view_emit_barrel", at, 0, &Palette::new()).unwrap();
            scene.meshes.iter().map(|m| (m.color, m.material)).collect::<Vec<_>>()
        };
        let (worn, plain) = (draw(true), draw(false));
        assert!(worn.iter().any(|(_, material)| *material != 0), "patterned parts get a material slot: {worn:?}");
        assert!(plain.iter().all(|(_, material)| *material == 0), "outside a frame parts draw in their colours");
        let colors = |list: &[(Vec3, u32)]| list.iter().map(|(color, _)| *color).collect::<Vec<_>>();
        assert_eq!(colors(&worn), colors(&plain), "the same colours either way");
        props::remove("WORKSHOP/view_emit_barrel").unwrap();
    }

    #[test]
    fn units_move_per_frame_share_meshes_and_vary_by_seed() {
        let stall = install(include_str!("../../pav_core/tests/fixtures/props/format2/market_stall.json"), "view_emit_units");
        let mut scene = Scene::default();
        for (i, x) in [0.0, 5.0, 10.0].into_iter().enumerate() {
            emit_asset(&mut scene, "WORKSHOP/view_emit_units", Placement::at(Vec3::X * x).id(i as u32), 0, &Palette::new())
                .unwrap();
        }
        let parts = stall.compiled.parts.iter().filter(|p| !p.hidden).count();
        assert!(scene.meshes.len() + scene.sdfs.len() >= parts * 3);
        let keys: Vec<_> = scene.custom_meshes.iter().map(|(key, _)| *key).collect();
        let unique: std::collections::BTreeSet<_> = keys.iter().collect();
        assert!(!keys.is_empty() && unique.len() == keys.len(), "each generated mesh is sent once: {keys:?}");
        // Next frame the unit has moved: its parts move with it.
        let first = |x: f32| {
            let mut scene = Scene::default();
            emit_asset(&mut scene, "BUILTIN/crate", Placement::at(Vec3::new(x, 0.0, 0.0)), 0, &Palette::new()).unwrap();
            scene.meshes[0].transform.w_axis.x
        };
        assert!((first(2.0) - first(0.0) - 2.0).abs() < 1e-5);
        // A seed picks a variant; the same seed always draws the same.
        let jitter = r##"{"format":2,"name":"x","unit":"cm","parts":{"post":{"type":"box","size":[10,100,10],"pos":[0,50,0],"color":"#806040","jitter":{"pos":[20,0,20]}}}}"##;
        install(jitter, "view_emit_jitter");
        let draw = |seed: u32| {
            let mut scene = Scene::default();
            emit_asset(&mut scene, "WORKSHOP/view_emit_jitter", Placement::at(Vec3::ZERO), seed, &Palette::new()).unwrap();
            scene.meshes[0].transform
        };
        assert_eq!(draw(3), draw(3));
        assert_ne!(draw(3), draw(4));
        for name in ["WORKSHOP/view_emit_units", "WORKSHOP/view_emit_jitter"] {
            props::remove(name).unwrap();
        }
    }

    #[test]
    fn a_two_sided_mesh_part_draws_its_back_lit() {
        use pav_core::props::mesh::{MeshFile, install as install_mesh};
        let sheet = |two_sided| MeshFile {
            name: "sheet".into(),
            budget: 4096,
            two_sided,
            palette: vec!["#a02020".into()],
            v: vec![[0, 0, 0], [500, 0, 0], [0, 500, 0]],
            f: vec![[0, 1, 2, 0]],
            source: None,
        };
        install_mesh("view_two/both", sheet(true)).unwrap();
        install_mesh("view_two/front", sheet(false)).unwrap();
        install(
            r#"{"format":2,"name":"x","unit":"cm","parts":{"a":{"mesh":"view_two/both"},"b":{"mesh":"view_two/front","pos":[100,0,0]}}}"#,
            "view_two_sided",
        );
        let mut scene = Scene::default();
        emit_asset(&mut scene, "WORKSHOP/view_two_sided", Placement::at(Vec3::ZERO), 0, &Palette::new()).unwrap();
        let two = |x: f32| {
            let m = scene.meshes.iter().find(|m| (m.transform.w_axis.x - x).abs() < 1e-4).unwrap();
            m.flags & pav_render::scene::flags::TWO_SIDED != 0
        };
        assert!(two(0.0) && !two(1.0));
        props::remove("WORKSHOP/view_two_sided").unwrap();
    }

    #[test]
    fn bad_names_and_palettes_say_what_is_wrong() {
        let mut scene = Scene::default();
        let none = Palette::new();
        let error = emit_asset(&mut scene, "BUILTIN/crat", Placement::at(Vec3::ZERO), 0, &none).unwrap_err();
        assert_eq!(error, r#"no prop 'BUILTIN/crat' — did you mean "BUILTIN/crate"?"#);
        assert!(emit_asset(&mut scene, "LIB/core/no_such_asset", Placement::at(Vec3::ZERO), 0, &none).is_err());
        let bad: Palette = [("$wal".to_owned(), "#ffffff".to_owned())].into();
        assert!(
            emit_asset(&mut scene, "BUILTIN/crate", Placement::at(Vec3::ZERO), 0, &bad).unwrap_err().contains("did you mean")
        );
        assert!(emit_asset(&mut scene, "BUILTIN/crate", Placement::at(Vec3::ZERO).scale(0.0), 0, &none).is_err());
        assert!(scene.meshes.is_empty(), "nothing is drawn on an error");
    }
}
