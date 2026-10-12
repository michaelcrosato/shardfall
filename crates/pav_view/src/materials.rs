//! The material table (asset plan §6.4-6.5): procedural materials interned into stable rows of
//! the renderer's material buffer.
//!
//! An instance draws a material through its `material` word, `slot | seed << 12`
//! (`MatSlot::param`), and carries the material's base colour, glow and look itself, so one
//! row serves every tint. A row is keyed by the material's name, the map it is laid with
//! (object, world or axis: packed into the row) and the theme colours its layers use (only a
//! material whose layers name roles needs a row per palette). Rows never move: when the
//! library changes, `refresh` (or interning the new `Arc<Material>` under the same name)
//! re-packs the row in place, so cached instances keep their slots and an edit is one
//! 368-byte upload. `instance_generation` changes when what travels in the instances changes,
//! for caches that bake them (static regions).

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use glam::Vec3;
use pav_core::Look;
use pav_core::material::eval::pcg;
use pav_core::material::{Library, Material};
use pav_core::props::doc::MapMode;
use pav_render::scene::{GpuMaterial, MATERIAL_SLOTS, Scene, material_param};

/// Rows beyond this many are a warning sign (§6.3: ≤ 256 soft); the hard limit is the
/// 12-bit slot (4,095 rows besides row 0).
pub const SOFT_SLOTS: usize = 256;

/// Table versions are unique across tables, so a renderer that outlives a view never mistakes
/// one table for another.
static NEXT_VERSION: AtomicU64 = AtomicU64::new(1);

/// A material ready for instances: its row and what travels in the instance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MatSlot {
    pub slot: u32,
    pub map: MapMode,
    /// Linear RGB: the instance colour.
    pub base: Vec3,
    /// The instance's emissive (the layers' glow adds to it on the GPU).
    pub emissive: f32,
    pub look: Look,
}

impl MatSlot {
    /// The instance's `material` word. World-mapped surfaces take seed 0, so neighbouring
    /// blocks (4 m floor tiles) join seamlessly.
    pub fn param(&self, seed: u32) -> u32 {
        material_param(self.slot, if self.map == MapMode::World { 0 } else { seed })
    }
}

/// The per-instance seed for object and axis maps: hash(entity, part), 20 bits.
pub fn seed(entity: u32, part: u32) -> u32 {
    pcg(entity.wrapping_mul(0x9e37_79b9) ^ pcg(part.wrapping_add(0x51))) & 0xfffff
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    name: String,
    /// The map the caller asked for; `None` follows the material's own map, also after a
    /// library change.
    map: Option<MapMode>,
    /// Hash of the role colours the layers use (0 when they use none).
    palette: u64,
}

struct Entry {
    key: Key,
    material: Arc<Material>,
    roles: Vec<(String, String)>,
    slot: MatSlot,
}

/// Interned materials and the rows the renderer uploads (row 0 is none).
pub struct MaterialTable {
    rows: Arc<Vec<GpuMaterial>>,
    entries: Vec<Entry>,
    index: HashMap<Key, usize>,
    version: u64,
    generation: u64,
    full_warned: bool,
}

impl Default for MaterialTable {
    fn default() -> Self {
        Self {
            rows: Arc::new(vec![GpuMaterial::default()]),
            entries: Vec::new(),
            index: HashMap::new(),
            version: NEXT_VERSION.fetch_add(1, Ordering::Relaxed),
            generation: 0,
            full_warned: false,
        }
    }
}

impl MaterialTable {
    /// Rows in use besides row 0.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Changes whenever a row's base colour, emissive or look changes (on a library refresh):
    /// caches of built instances compare it.
    pub fn instance_generation(&self) -> u64 {
        self.generation
    }

    /// The material `name` from `lib` laid with `map` (`None`: the material's own map, else
    /// object), its layer roles taken from `roles` (`$wall` → `#rrggbb`; the default roles
    /// fill gaps). `None` if the library has no such material.
    pub fn resolve(&mut self, lib: &Library, name: &str, map: Option<MapMode>, roles: &[(&str, &str)]) -> Option<MatSlot> {
        let material = lib.get(name)?;
        Some(self.intern(name, material, map, roles))
    }

    /// Interns `material` under `name` (a library name, or an owner-qualified key for an
    /// inline material): the same name, map and layer palette always get the same slot. A
    /// different `Arc` under a known name is a library change: the row is re-packed in place.
    /// When every slot is taken it returns slot 0 (the plain colour) and warns once.
    pub fn intern(&mut self, name: &str, material: &Arc<Material>, map: Option<MapMode>, roles: &[(&str, &str)]) -> MatSlot {
        let key = Key { name: name.to_owned(), map, palette: palette_hash(material, roles) };
        if let Some(&i) = self.index.get(&key) {
            if !Arc::ptr_eq(&self.entries[i].material, material) {
                self.entries[i].material = material.clone();
                self.repack(i);
            }
            let entry = &self.entries[i];
            if same_roles(&entry.roles, roles) {
                return entry.slot;
            }
            // The row is shared (the layers' roles agree), but the base colour, emissive and
            // look travel in the instance, so they come from these roles.
            let packed = entry.material.pack(entry.slot.map, roles);
            return MatSlot { base: packed.base, emissive: packed.emissive, look: packed.look, ..entry.slot };
        }
        let map = effective_map(map, material);
        let slot = self.entries.len() + 1;
        if slot >= MATERIAL_SLOTS as usize {
            if !self.full_warned {
                eprintln!("materials: all {} slots are taken; {name} draws in its plain colour", MATERIAL_SLOTS - 1);
                self.full_warned = true;
            }
            let packed = material.pack(map, roles);
            return MatSlot { slot: 0, map, base: packed.base, emissive: packed.emissive, look: packed.look };
        }
        if slot == SOFT_SLOTS + 1 {
            eprintln!("materials: more than {SOFT_SLOTS} rows in the table");
        }
        let roles = roles.iter().map(|(role, hex)| (role.to_string(), hex.to_string())).collect();
        let placeholder = MatSlot { slot: slot as u32, map, base: Vec3::ONE, emissive: 0.0, look: Look::Cel };
        self.entries.push(Entry { key: key.clone(), material: material.clone(), roles, slot: placeholder });
        Arc::make_mut(&mut self.rows).push(GpuMaterial::default());
        self.index.insert(key, slot - 1);
        self.repack(slot - 1);
        self.entries[slot - 1].slot
    }

    /// Re-resolves every row against `lib` in place (after the library changed). A material
    /// that is gone keeps its last row.
    pub fn refresh(&mut self, lib: &Library) {
        for i in 0..self.entries.len() {
            if let Some(material) = lib.get(&self.entries[i].key.name) {
                if !Arc::ptr_eq(&self.entries[i].material, material) {
                    self.entries[i].material = material.clone();
                    self.repack(i);
                }
            }
        }
    }

    fn repack(&mut self, i: usize) {
        let entry = &self.entries[i];
        let roles: Vec<(&str, &str)> = entry.roles.iter().map(|(role, hex)| (role.as_str(), hex.as_str())).collect();
        let map = effective_map(entry.key.map, &entry.material);
        let packed = entry.material.pack(map, &roles);
        let slot = MatSlot { slot: entry.slot.slot, map, base: packed.base, emissive: packed.emissive, look: packed.look };
        if slot != entry.slot {
            self.entries[i].slot = slot;
            self.generation += 1;
        }
        let row = GpuMaterial::from_words(&packed.gpu.words());
        let rows = Arc::make_mut(&mut self.rows);
        if rows[i + 1] != row {
            rows[i + 1] = row;
            self.version = NEXT_VERSION.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Gives `scene` the table (shared, uploaded only when it changed).
    pub fn attach(&self, scene: &mut Scene) {
        scene.materials = self.rows.clone();
        scene.materials_version = self.version;
    }
}

/// The map a row is laid with: the caller's, else the material's own, else object.
fn effective_map(map: Option<MapMode>, material: &Material) -> MapMode {
    map.or(material.map).unwrap_or(MapMode::Object)
}

fn same_roles(stored: &[(String, String)], roles: &[(&str, &str)]) -> bool {
    stored.len() == roles.len() && stored.iter().zip(roles).all(|((r, h), (r2, h2))| r == r2 && h == h2)
}

/// Hash of the role colours a material's layers use, as `roles` resolves them (0 for none).
fn palette_hash(material: &Material, roles: &[(&str, &str)]) -> u64 {
    if !material.layers_use_roles() {
        return 0;
    }
    let mut used: Vec<&str> = Vec::new();
    for layer in &material.layers {
        used.extend(layer.color().filter(|c| c.starts_with('$')));
        used.extend(layer.cells().iter().map(String::as_str).filter(|c| c.starts_with('$')));
    }
    used.sort_unstable();
    used.dedup();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for role in used {
        role.hash(&mut h);
        roles.iter().find(|(r, _)| *r == role).map(|(_, hex)| *hex).hash(&mut h);
    }
    h.finish() | 1
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAIN: &str =
        r##"{"format":1,"name":"plain","unit":"cm","color":"#806040","layers":[{"type":"brick","size":[40,20]}]}"##;
    const ROLED: &str =
        r##"{"format":1,"name":"roled","unit":"cm","color":"$wall","layers":[{"type":"noise","size":50,"color":"$accent"}]}"##;

    #[test]
    fn slots_are_stable_and_rows_repack_in_place() {
        let lib = Library::from_sources(&[("plain", PLAIN), ("roled", ROLED)]);
        let mut table = MaterialTable::default();
        let a = table.resolve(&lib, "MAT/plain", None, &[]).unwrap();
        let b = table.resolve(&lib, "MAT/plain", Some(MapMode::World), &[]).unwrap();
        assert_eq!((a.slot, b.slot), (1, 2), "a map is part of the row");
        assert_eq!(table.resolve(&lib, "MAT/plain", None, &[("$wall", "#000000")]).unwrap().slot, 1, "no roles: one row");
        // Roles in a layer: one row per palette; the base colour's role travels in the instance.
        let r1 = table.resolve(&lib, "MAT/roled", None, &[("$accent", "#ff0000")]).unwrap();
        let r2 = table.resolve(&lib, "MAT/roled", None, &[("$accent", "#00ff00")]).unwrap();
        let r3 = table.resolve(&lib, "MAT/roled", None, &[("$accent", "#ff0000"), ("$wall", "#123456")]).unwrap();
        assert_ne!(r1.slot, r2.slot);
        assert_eq!(r1.slot, r3.slot);
        assert_ne!(r1.base, r3.base, "the shared row's instance colour follows these roles");
        assert_eq!(table.resolve(&lib, "MAT/roled", None, &[("$accent", "#ff0000")]).unwrap(), r1);
        assert_eq!(table.len(), 4);
        // World maps ignore the seed.
        assert_eq!(b.param(77), b.slot);
        assert_eq!(a.param(77), a.slot | 77 << 12);

        // A library change re-packs the row in place: same slot, new row, new version.
        let mut scene = Scene::default();
        table.attach(&mut scene);
        let (version, generation) = (scene.materials_version, table.instance_generation());
        let edited = PLAIN.replace("#806040", "#204060").replace("[40,20]", "[60,20]");
        let lib2 = Library::from_sources(&[("plain", &edited), ("roled", ROLED)]);
        table.refresh(&lib2);
        let a2 = table.resolve(&lib2, "MAT/plain", None, &[]).unwrap();
        assert_eq!(a2.slot, a.slot);
        assert_ne!(a2.base, a.base);
        assert_ne!(table.instance_generation(), generation, "the base colour travels in instances");
        table.attach(&mut scene);
        assert_ne!(scene.materials_version, version);
        assert_eq!(scene.materials.len(), 5);
        let words = lib2.get("MAT/plain").unwrap().pack(MapMode::Object, &[]).gpu.words();
        assert_eq!(scene.materials[a.slot as usize], GpuMaterial::from_words(&words));
        // Nothing changed: the version holds, so the renderer uploads nothing.
        let version = scene.materials_version;
        table.refresh(&lib2);
        table.attach(&mut scene);
        assert_eq!(scene.materials_version, version);
    }

    #[test]
    fn an_inherited_map_follows_the_library() {
        let lib = Library::from_sources(&[("plain", PLAIN)]);
        let mut table = MaterialTable::default();
        let own = table.resolve(&lib, "MAT/plain", None, &[]).unwrap();
        let fixed = table.resolve(&lib, "MAT/plain", Some(MapMode::Object), &[]).unwrap();
        assert_eq!((own.map, fixed.map), (MapMode::Object, MapMode::Object));
        let world = PLAIN.replace(r#""color""#, r#""map":"world","color""#);
        let lib2 = Library::from_sources(&[("plain", &world)]);
        table.refresh(&lib2);
        let own2 = table.resolve(&lib2, "MAT/plain", None, &[]).unwrap();
        let fixed2 = table.resolve(&lib2, "MAT/plain", Some(MapMode::Object), &[]).unwrap();
        assert_eq!((own2.slot, own2.map), (own.slot, MapMode::World), "the inherited map follows");
        assert_eq!((fixed2.slot, fixed2.map), (fixed.slot, MapMode::Object), "an explicit map holds");
        let mut scene = Scene::default();
        table.attach(&mut scene);
        let words = lib2.get("MAT/plain").unwrap().pack(MapMode::World, &[]).gpu.words();
        assert_eq!(scene.materials[own2.slot as usize], GpuMaterial::from_words(&words));
    }

    #[test]
    fn seeds_are_twenty_bits_and_vary() {
        let seeds: Vec<u32> = (0..64).map(|part| seed(7, part)).collect();
        assert!(seeds.iter().all(|s| *s <= 0xfffff));
        let mut unique = seeds.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), seeds.len());
        assert_ne!(seed(7, 0), seed(8, 0));
    }
}
