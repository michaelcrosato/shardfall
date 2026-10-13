//! The open model libraries the asset importer knows (its directory, `asset_import` with no
//! arguments), and `get`: a pack downloaded into `.cache/assets/<lib>/<pack>/` from the source
//! its catalog pins (`assets/catalogs/<lib>.json`, `$sources.<pack>`), checked against the
//! pinned SHA-256, unpacked inside that folder, with `SOURCE.json` saying where it came from.
//!
//! A pinned zip source: `url` (a whole archive up to 64 MB, else its members by ranged reads),
//! `sha256` (of the archive), `models` (a glob of the model files, relative to the archive's
//! root), `licence_file` and `licence_sha256`. A git source: `repo` (owner/name), `version` (the
//! commit), `files` (paths at that commit, fetched one by one) and `sha256` (of the files'
//! `sha256sum` lines). Placement: `up`, `yaw`, `scale`, `place`. The licence file must also say
//! the source's `license` (`licence::check_text`). For translation: `pack` (the LIB pack),
//! `label` and `credit` (its `PACK.json`), `listed: true` (only models with a catalog line) and
//! `local: "<why>"` (kept in the local tier though its licence could be shared).

use std::path::{Path, PathBuf};

use anyhow::{Result, anyhow, bail};
use serde_json::{Map, Value, json};

use super::catalog::Catalog;
use super::fetch::{self, ASSET_AGENT, RemoteZip};
use super::ledger::Ledger;

/// The largest archive downloaded whole (else its members are read by ranges).
pub const WHOLE_ZIP: u64 = 64 << 20;

/// A library in the directory.
pub struct Library {
    pub id: &'static str,
    pub name: &'static str,
    pub what: &'static str,
    pub licence: &'static str,
    pub access: &'static str,
    pub packs: &'static str,
}

/// The libraries, as tested on 2026-10-11 (plan §8.8).
pub const LIBRARIES: &[Library] = &[
    Library {
        id: "kenney",
        name: "Kenney",
        what: "~4,600 low-poly game models in themed kits (dungeon, castle, nature, food...), GLB/OBJ/FBX",
        licence: "CC0-1.0",
        access: "pack page -> version-hashed zip; range requests work",
        packs: "mini_dungeon, castle_kit, nature_kit, food_kit",
    },
    Library {
        id: "kaykit",
        name: "KayKit",
        what: "low-poly kits (dungeon, furniture, ...) in 10 GitHub repos, glTF",
        licence: "CC0-1.0",
        access: "raw files at a pinned commit, listed in the catalog (the GitHub API is blocked here)",
        packs: "dungeon_remastered; furniture_bits",
    },
    Library {
        id: "quaternius",
        name: "Quaternius",
        what: "low-poly packs (house interior, modular dungeon, ...); OBJ mirrors on OpenGameArt",
        licence: "CC0-1.0 (Bestiary: QAL, local only)",
        access: "OpenGameArt zips, 10 s crawl delay (Google Drive is quota-blocked)",
        packs: "medieval_village (OBJ); more on OpenGameArt",
    },
    Library {
        id: "polypizza",
        name: "Poly Pizza",
        what: "CC0/CC-BY model search",
        licence: "CC0-1.0 / CC-BY",
        access: "needs POLY_PIZZA_KEY; blocks cloud containers: skipped (D6)",
        packs: "",
    },
    Library {
        id: "khronos",
        name: "Khronos glTF-Sample-Assets",
        what: "glTF conformance models (Box, Duck, OrientationTest...)",
        licence: "per model (CC0/CC-BY)",
        access: "raw files at a pinned commit",
        packs: "conformance only: cargo test -p pav_tools --test import_khronos -- --ignored",
    },
];

/// Where catalogs and ledgers live: $PAV_CATALOGS, else assets/catalogs (like assets/props,
/// relative to the repository root the tools run in).
pub fn catalogs_dir() -> PathBuf {
    std::env::var_os("PAV_CATALOGS").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("assets").join("catalogs"))
}

/// A library's catalog, if there is one.
pub fn catalog(lib: &str) -> Option<Catalog> {
    Catalog::read(&catalogs_dir().join(format!("{lib}.json"))).ok()
}

/// A pack's pinned source from its library's catalog.
pub fn source(lib: &str, pack: &str) -> Result<Map<String, Value>> {
    let cat = catalog(lib).ok_or_else(|| anyhow!("no catalog assets/catalogs/{lib}.json (known: {})", known()))?;
    cat.sources.get(pack).and_then(Value::as_object).cloned().ok_or_else(|| {
        let packs: Vec<&String> = cat.sources.keys().collect();
        anyhow!("{lib} pins no pack {pack} (pinned: {packs:?})")
    })
}

fn known() -> String {
    LIBRARIES.iter().map(|l| l.id).collect::<Vec<_>>().join(", ")
}

/// Splits `lib/pack`.
pub fn lib_pack(s: &str) -> Result<(&str, &str)> {
    s.split_once('/')
        .filter(|(l, p)| !l.is_empty() && !p.is_empty())
        .ok_or_else(|| anyhow!("{s}: lib/pack, like kenney/mini_dungeon"))
}

/// The folder a pack is downloaded into.
pub fn pack_dir(lib: &str, pack: &str) -> PathBuf {
    fetch::cache_dir_for("assets").join(lib).join(pack)
}

/// The model files under a folder (.glb, .gltf, .obj), relative and sorted; with `glob`, only
/// those it matches.
pub fn models_in(dir: &Path, glob: Option<&str>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if is_model(&p) {
                let rel = p.strip_prefix(dir).unwrap_or(&p).to_path_buf();
                let text = rel.to_string_lossy().replace('\\', "/");
                if glob.is_none_or(|g| glob_match(g, &text)) {
                    out.push(rel);
                }
            }
        }
    }
    out.sort();
    out
}

pub fn is_model(p: &Path) -> bool {
    matches!(p.extension().map(|e| e.to_string_lossy().to_lowercase()).as_deref(), Some("glb" | "gltf" | "obj"))
}

/// A path glob: `*` within one folder name, `**` across folders, `?` one character; several
/// globs separated by commas.
pub fn glob_match(glob: &str, path: &str) -> bool {
    fn m(g: &[u8], p: &[u8]) -> bool {
        match (g.first(), p.first()) {
            (None, None) => true,
            (Some(b'*'), _) if g.get(1) == Some(&b'*') => {
                let rest = g[2..].strip_prefix(b"/").unwrap_or(&g[2..]);
                (0..=p.len()).any(|i| m(rest, &p[i..]))
            }
            (Some(b'*'), _) => (0..=p.len()).take_while(|&i| i == 0 || p[i - 1] != b'/').any(|i| m(&g[1..], &p[i..])),
            (Some(b'?'), Some(c)) if *c != b'/' => m(&g[1..], &p[1..]),
            (Some(a), Some(b)) if a == b => m(&g[1..], &p[1..]),
            _ => false,
        }
    }
    glob.split(',').map(str::trim).any(|g| m(g.as_bytes(), path.as_bytes()))
}

/// The folder part of a glob before its first wildcard (`Models/GLB format/`).
pub fn glob_root(glob: &str) -> &str {
    let w = glob.find(['*', '?']).unwrap_or(glob.len());
    glob[..w].rfind('/').map_or("", |i| &glob[..=i])
}

/// The library directory: each library with its catalog, cache and ledger counts.
pub fn directory() -> Value {
    let libs: Vec<Value> = LIBRARIES
        .iter()
        .map(|l| {
            let cat = catalog(l.id);
            let cached = std::fs::read_dir(fetch::cache_dir_for("assets").join(l.id))
                .map(|d| d.flatten().filter(|e| e.path().is_dir()).map(|e| models_in(&e.path(), None).len()).sum::<usize>())
                .unwrap_or(0);
            let ledger_rows: usize = std::fs::read_dir(catalogs_dir().join(l.id))
                .map(|d| {
                    d.flatten()
                        .filter(|e| e.path().extension().is_some_and(|x| x == "tsv"))
                        .map(|e| Ledger::read(&e.path()).rows.len())
                        .sum()
                })
                .unwrap_or(0);
            let mut v = json!({
                "lib": l.id, "name": l.name, "what": l.what, "licence": l.licence, "access": l.access,
                "packs": l.packs, "models_cached": cached, "ledger_rows": ledger_rows,
            });
            if let Some(c) = cat {
                v["pinned"] = json!(c.sources.keys().collect::<Vec<_>>());
            }
            v
        })
        .collect();
    json!({
        "libraries": libs,
        "cache": fetch::cache_dir_for("assets").display().to_string(),
        "use": "get=kenney/mini_dungeon downloads a pinned pack; survey=kenney/mini_dungeon (or a folder) censuses it; from=<model> list=true summarises one",
    })
}

/// The date today (UTC), for SOURCE.json only.
fn today() -> String {
    let days =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() / 86_400).unwrap_or(0) as i64;
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The source fields that decide a pack's bytes: when any changes, the pack is fetched afresh.
const PIN_KEYS: [&str; 7] = ["url", "repo", "version", "sha256", "files", "models", "licence_file"];

fn pin_of(src: &Map<String, Value>) -> Map<String, Value> {
    PIN_KEYS.iter().filter_map(|k| src.get(*k).map(|v| (k.to_string(), v.clone()))).collect()
}

/// Whether the pack's `SOURCE.json` says `get` fetched it under this pin. A pin that only adds
/// the `sha256` that fetch recorded (the usual first `get`, then the hash pinned) is the same.
fn fetched_under(dir: &Path, pin: &Map<String, Value>) -> bool {
    let Some(record) = std::fs::read(dir.join("SOURCE.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else {
        return false;
    };
    let Some(mut fetched) = record["pin"].as_object().cloned() else { return false };
    if pin.contains_key("sha256") && !fetched.contains_key("sha256") && !record["sha256"].is_null() {
        fetched.insert("sha256".into(), record["sha256"].clone());
    }
    &fetched == pin
}

/// The gate for translating a pinned pack into `lib/`, where it is committed and credited to
/// its source: the source pins its bytes and licence text with real values, and the cache is
/// what `get` fetched under that pin, its archive hash verified and its licence text checked
/// for the catalog's licence.
pub fn verified_for_lib(lib: &str, pack: &str) -> Result<()> {
    let (src, dir) = (source(lib, pack)?, pack_dir(lib, pack));
    let keys = ["sha256", "licence_file", "licence_sha256", "license"];
    let missing: Vec<&str> =
        keys.into_iter().filter(|k| src.get(*k).and_then(Value::as_str).is_none_or(|v| v.trim().is_empty())).collect();
    if !missing.is_empty() {
        bail!("{lib}/{pack}: a lib/ pack pins its bytes and licence text: add {} to its source", missing.join(", "));
    }
    if !fetched_under(&dir, &pin_of(&src)) || !licence_checked(&dir, &src) {
        bail!("{lib}/{pack}: the cached pack is not the one the catalog pins: asset_import get={lib}/{pack} first");
    }
    let record = std::fs::read(dir.join("SOURCE.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok());
    if record.as_ref().map(|r| &r["sha256"]) != src.get("sha256") {
        bail!(
            "{lib}/{pack}: get could not check the archive's SHA-256 (a pack over 64 MB is read by ranges), and a lib/ pack needs it checked"
        );
    }
    Ok(())
}

/// Whether `get` checked the licence text for the catalog's `license` (and `licence_sha256`).
fn licence_checked(dir: &Path, src: &Map<String, Value>) -> bool {
    let Some(record) = std::fs::read(dir.join("SOURCE.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()) else {
        return false;
    };
    let want = |k: &str| src.get(k).cloned().unwrap_or(Value::Null);
    record["licence"] == want("license")
        && (want("licence_sha256").is_null() || record["licence_sha256"] == want("licence_sha256"))
}

/// The library pack other than `lib/pack` whose translations also go to the output pack
/// `name`, if any: two sources writing one folder would credit or prune each other's assets.
pub fn pack_name_owner(lib: &str, pack: &str, name: &str) -> Option<String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(catalogs_dir()).ok()?.flatten().map(|e| e.path()).collect();
    files.sort();
    files.iter().filter(|p| p.extension().is_some_and(|e| e == "json")).find_map(|p| {
        let other_lib = p.file_stem()?.to_str()?;
        let cat = Catalog::read(p).ok()?;
        cat.sources.iter().find_map(|(other, v)| {
            let out = v.get("pack").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| format!("{other_lib}_{other}"));
            (out == name && (other_lib, other.as_str()) != (lib, pack)).then(|| format!("{other_lib}/{other}"))
        })
    })
}

/// Downloads a pinned pack. Returns what it did, for the reply.
///
/// A pack already fetched under the same pin is completed in place. A new or changed pin is
/// fetched into a staging folder beside it, checked (archive and licence SHA-256), and only then
/// swapped in, so no old file survives under a new `SOURCE.json` and a failed check leaves the
/// old pack as it was.
pub fn get(lib: &str, pack: &str) -> Result<Value> {
    let src = source(lib, pack)?;
    let s = |k: &str| src.get(k).and_then(Value::as_str);
    let pin = pin_of(&src);
    let dir = pack_dir(lib, pack);
    let current = fetched_under(&dir, &pin);
    let work = if current { dir.clone() } else { dir.with_file_name(format!(".{pack}.part")) };
    if !current && work.exists() {
        std::fs::remove_dir_all(&work)?;
    }
    std::fs::create_dir_all(&work)?;
    let mut files: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    if !current && dir.exists() {
        notes.push("a new or changed pin: the pack is fetched afresh".into());
    }
    let (sha, version) = if let Some(url) = s("url") {
        let globs = s("models").unwrap_or("**/*.glb");
        let roots: Vec<&str> = globs.split(',').map(|g| glob_root(g.trim())).collect();
        let licence = s("licence_file");
        let wanted = |name: &str| roots.iter().any(|r| name.starts_with(r)) || licence.is_some_and(|l| name == l);
        let size = fetch::size_as(ASSET_AGENT, url)?;
        let (zip, sha) = if size.is_some_and(|n| n <= WHOLE_ZIP) {
            let name = url.rsplit('/').next().filter(|n| !n.is_empty()).unwrap_or("pack.zip");
            let path = work.join(name);
            fetch::get_as(Some(ASSET_AGENT), url, &path)?;
            let sha = fetch::sha256_hex(&std::fs::read(&path)?);
            if let Some(want) = s("sha256").filter(|w| *w != sha) {
                let _ = std::fs::remove_file(&path);
                bail!(
                    "{lib}/{pack}: the archive's SHA-256 is {sha}, not the pinned {want}: the source changed; review it and re-pin"
                );
            }
            if s("sha256").is_none() {
                notes.push(format!("no sha256 pinned: pin {sha} in assets/catalogs/{lib}.json"));
            }
            (RemoteZip::open_file(&path)?, Some(sha))
        } else {
            notes.push("over 64 MB: members read by ranges (each CRC-checked; no archive SHA-256)".into());
            (RemoteZip::open(url)?, None)
        };
        for e in zip.entries.iter().filter(|e| wanted(&e.name)) {
            let Some(rel) = e.rel_path() else { continue };
            let out = work.join(&rel);
            if !out.exists() {
                let bytes = zip.read(e)?;
                if let Some(d) = out.parent() {
                    std::fs::create_dir_all(d)?;
                }
                std::fs::write(&out, bytes)?;
            }
            files.push(rel.to_string_lossy().replace('\\', "/"));
        }
        (sha, s("version").map(str::to_string))
    } else if let (Some(repo), Some(commit)) = (s("repo"), s("version")) {
        let list: Vec<&str> =
            src.get("files").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
        if list.is_empty() {
            bail!("{lib}/{pack}: a git source lists its files (\"files\": [paths at the commit])");
        }
        let mut manifest = Vec::new();
        for f in list {
            let rel = PathBuf::from(f);
            if rel.components().any(|c| !matches!(c, std::path::Component::Normal(_))) {
                bail!("{lib}/{pack}: {f} is not a plain relative path");
            }
            let url = format!("https://raw.githubusercontent.com/{repo}/{commit}/{}", f.replace(' ', "%20"));
            fetch::get_as(Some(ASSET_AGENT), &url, &work.join(&rel))?;
            manifest.push(format!("{}  {f}\n", fetch::sha256_hex(&std::fs::read(work.join(&rel))?)));
            files.push(f.to_string());
        }
        // The SHA-256 of the files' `sha256sum` lines, sorted by path.
        manifest.sort_by(|a, b| a[66..].cmp(&b[66..]));
        let sha = fetch::sha256_hex(manifest.concat().as_bytes());
        if let Some(want) = s("sha256").filter(|w| *w != sha) {
            bail!("{lib}/{pack}: the files' SHA-256 manifest is {sha}, not the pinned {want}: review the change and re-pin");
        }
        if s("sha256").is_none() {
            notes.push(format!("no sha256 pinned: pin {sha} in assets/catalogs/{lib}.json"));
        }
        (Some(sha), Some(commit.to_string()))
    } else {
        bail!("{lib}/{pack}: its source has neither url nor repo + version");
    };
    // The licence text, checked against its pin.
    let mut licence_sha = None;
    if let Some(l) = s("licence_file") {
        let p = work.join(l);
        let text = std::fs::read(&p).map_err(|_| anyhow!("{lib}/{pack}: the licence file {l} is not in the pack"))?;
        let h = fetch::sha256_hex(&text);
        if let Some(want) = s("licence_sha256").filter(|w| *w != h) {
            bail!("{lib}/{pack}: {l} changed (SHA-256 {h}, pinned {want}): review the licence before using the pack");
        }
        if s("licence_sha256").is_none() {
            notes.push(format!("no licence_sha256 pinned: pin {h} in assets/catalogs/{lib}.json"));
        }
        if let Some(license) = s("license") {
            super::licence::check_text(license, &String::from_utf8_lossy(&text))
                .map_err(|e| anyhow!("{lib}/{pack}: {l}: {e}"))?;
        }
        licence_sha = Some(h);
    }
    // Everything checked: the staged pack replaces the old one whole.
    if work != dir {
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        std::fs::rename(&work, &dir)?;
    }
    files.sort();
    let models = models_in(&dir, s("models")).len();
    let record = json!({
        "url": s("url").map(str::to_string).or_else(|| s("repo").map(|r| format!("https://github.com/{r}"))),
        "version": version, "sha256": sha, "licence": s("license"), "licence_sha256": licence_sha,
        "files": files, "fetched": today(), "pin": pin,
    });
    std::fs::write(dir.join("SOURCE.json"), serde_json::to_string_pretty(&record)? + "\n")?;
    Ok(json!({
        "pack": format!("{lib}/{pack}"), "dir": dir.display().to_string(), "files": files.len(), "models": models,
        "sha256": sha, "notes": notes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pack_is_current_under_its_pin_or_the_hash_its_fetch_recorded() {
        let dir = std::env::temp_dir().join(format!("pav-pin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let record = json!({"sha256": "abc", "pin": {"url": "u", "models": "*.glb"}});
        std::fs::write(dir.join("SOURCE.json"), record.to_string()).unwrap();
        let pin = |v: Value| v.as_object().unwrap().clone();
        let record =
            json!({"sha256": "abc", "licence": "CC0-1.0", "licence_sha256": "f00", "pin": {"url": "u", "models": "*.glb"}});
        std::fs::write(dir.join("SOURCE.json"), record.to_string()).unwrap();
        assert!(licence_checked(&dir, &pin(json!({"license": "CC0-1.0", "licence_sha256": "f00"}))));
        assert!(licence_checked(&dir, &pin(json!({"license": "CC0-1.0"}))));
        assert!(!licence_checked(&dir, &pin(json!({"license": "CC-BY-4.0", "licence_sha256": "f00"}))));
        assert!(!licence_checked(&dir, &pin(json!({"license": "CC0-1.0", "licence_sha256": "f01"}))));
        assert!(fetched_under(&dir, &pin(json!({"url": "u", "models": "*.glb"}))));
        assert!(fetched_under(&dir, &pin(json!({"url": "u", "models": "*.glb", "sha256": "abc"}))));
        assert!(!fetched_under(&dir, &pin(json!({"url": "u", "models": "*.glb", "sha256": "abd"}))));
        assert!(!fetched_under(&dir, &pin(json!({"url": "v", "models": "*.glb"}))));
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(!fetched_under(&dir, &pin(json!({"url": "u"}))));
    }

    #[test]
    fn globs_match_folders_and_names() {
        assert!(glob_match("Models/GLB format/*.glb", "Models/GLB format/chair.glb"));
        assert!(!glob_match("Models/GLB format/*.glb", "Models/GLB format/Textures/x.glb"));
        assert!(glob_match("**/*.gltf", "a/b/c.gltf"));
        assert!(glob_match("**/*.gltf", "c.gltf"));
        assert!(glob_match("x/*.obj, y/*.glb", "y/z.glb"));
        assert!(glob_match("ch?ir.glb", "chair.glb"));
        assert_eq!(glob_root("Models/GLB format/*.glb"), "Models/GLB format/");
        assert_eq!(glob_root("*.glb"), "");
    }

    #[test]
    fn today_is_a_date() {
        let t = today();
        assert_eq!(t.len(), 10);
        assert!(t.as_str() >= "2026-01-01", "{t}");
    }
}
