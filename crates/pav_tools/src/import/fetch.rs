//! Downloads for the importers: whole files, and single files out of a zip archive on the web
//! (read with ranged requests from its central directory, so one take of a 1.5 GB dataset costs
//! only its own bytes). Requests go through `curl`, which every cloud container has and which
//! follows the environment's proxy settings. Requests carry no personal data: asset downloads
//! name only the importer (`pavilion-asset-import/<version>`).
//!
//! Zip entries are checked as they are read: an inflated entry is at most 256 MiB and must match
//! its CRC-32; names written to disk use `Entry::base` (the last path component only).

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, anyhow, bail};

/// The User-Agent of asset downloads (nothing personal).
pub const ASSET_AGENT: &str = concat!("pavilion-asset-import/", env!("CARGO_PKG_VERSION"));

/// The largest zip entry read (inflated).
pub const MAX_ENTRY: u64 = 256 << 20;

/// The repository root the caches sit in: beside the anim folder.
fn root() -> PathBuf {
    let anim = pav_core::anim::disk_dir().unwrap_or_else(|| PathBuf::from("anim"));
    anim.parent().filter(|p| !p.as_os_str().is_empty()).map(Path::to_path_buf).unwrap_or_else(|| ".".into())
}

/// Where downloaded captures are kept: $PAV_MOCAP_CACHE, else .cache/mocap beside the anim
/// folder (not committed).
pub fn cache_dir() -> PathBuf {
    cache_dir_for("mocap")
}

/// The download cache of one importer: `mocap` ($PAV_MOCAP_CACHE) or `assets`
/// ($PAV_ASSET_CACHE), else .cache/<kind> at the repository root (git-ignored).
pub fn cache_dir_for(kind: &str) -> PathBuf {
    let var = match kind {
        "mocap" => "PAV_MOCAP_CACHE",
        "assets" => "PAV_ASSET_CACHE",
        _ => "",
    };
    if let Ok(d) = std::env::var(var) {
        return d.into();
    }
    root().join(".cache").join(kind)
}

fn curl_as(agent: Option<&str>, args: &[&str]) -> Result<Vec<u8>> {
    let mut c = Command::new("curl");
    c.args(["-sSfL", "--retry", "3", "--connect-timeout", "30"]);
    if let Some(a) = agent {
        c.args(["-A", a]);
    }
    let out = c.args(args).output().map_err(|e| anyhow!("curl is needed to download: {e}"))?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

fn curl(args: &[&str]) -> Result<Vec<u8>> {
    curl_as(None, args)
}

/// Downloads `url` to `path` unless it is already there. Returns whether it downloaded.
pub fn get(url: &str, path: &Path) -> Result<bool> {
    get_as(None, url, path)
}

/// `get` with a User-Agent (asset downloads pass `ASSET_AGENT`).
pub fn get_as(agent: Option<&str>, url: &str, path: &Path) -> Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let part = path.with_extension("part");
    curl_as(agent, &["-o", &part.to_string_lossy(), url]).map_err(|e| anyhow!("{url}: {e}"))?;
    std::fs::rename(&part, path)?;
    Ok(true)
}

/// A small text resource (an API listing), with a User-Agent.
pub fn text_as(agent: &str, url: &str) -> Result<String> {
    let b = curl_as(Some(agent), &[url]).map_err(|e| anyhow!("{url}: {e}"))?;
    String::from_utf8(b).map_err(|_| anyhow!("{url}: not text"))
}

/// The size of a file on the web (its Content-Length after redirects), if the server says.
pub fn size_as(agent: &str, url: &str) -> Result<Option<u64>> {
    let head = curl_as(Some(agent), &["-I", url]).map_err(|e| anyhow!("{url}: {e}"))?;
    let text = String::from_utf8_lossy(&head);
    Ok(text
        .lines()
        .filter_map(|l| l.split_once(':'))
        .filter(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .filter_map(|(_, v)| v.trim().parse().ok())
        .next_back())
}

/// Bytes `from..=to` of a file on the web (`from` < 0: the last `-from` bytes).
pub fn range(url: &str, from: i64, to: i64) -> Result<Vec<u8>> {
    let r = if from < 0 { format!("{from}") } else { format!("{from}-{to}") };
    curl(&["-r", &r, url]).map_err(|e| anyhow!("{url} ({r}): {e}"))
}

/// One file in a zip archive.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    method: u16,
    packed: u64,
    pub size: u64,
    local: u64,
    crc: u32,
}

impl Entry {
    /// The entry's file name alone (no folders, so it cannot write outside the folder it is
    /// unpacked into); empty for a folder or a name that is only dots.
    pub fn base(&self) -> &str {
        let b = self.name.rsplit(['/', '\\']).next().unwrap_or("");
        if b.chars().all(|c| c == '.') { "" } else { b }
    }

    /// The entry's path as relative folders and a file name that stay inside the folder it is
    /// unpacked into (no root, drive, `.` or `..` parts); None for a folder.
    pub fn rel_path(&self) -> Option<PathBuf> {
        let parts: Vec<&str> =
            self.name.split(['/', '\\']).filter(|p| !p.is_empty() && !p.chars().all(|c| c == '.') && !p.contains(':')).collect();
        if parts.is_empty() || self.name.ends_with('/') {
            return None;
        }
        Some(parts.iter().collect())
    }
}

/// A zip archive, on the web (read by ranges) or on disk.
pub struct RemoteZip {
    src: Src,
    pub entries: Vec<Entry>,
}

enum Src {
    Url(String),
    File(PathBuf),
}

impl Src {
    /// Bytes `from..=to` (`from` < 0: the last `-from` bytes).
    fn bytes(&self, from: i64, to: i64) -> Result<Vec<u8>> {
        match self {
            Src::Url(u) => range(u, from, to),
            Src::File(p) => {
                use std::io::{Read, Seek, SeekFrom};
                let mut f = std::fs::File::open(p)?;
                let len = f.metadata()?.len() as i64;
                let (a, b) = if from < 0 { ((len + from).max(0), len - 1) } else { (from, to.min(len - 1)) };
                f.seek(SeekFrom::Start(a as u64))?;
                let mut out = vec![0u8; (b - a + 1).max(0) as usize];
                f.read_exact(&mut out)?;
                Ok(out)
            }
        }
    }
    fn name(&self) -> String {
        match self {
            Src::Url(u) => u.clone(),
            Src::File(p) => p.display().to_string(),
        }
    }
}

fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

impl RemoteZip {
    /// Reads the list of files of an archive on the web (its end record, then its central
    /// directory).
    pub fn open(url: &str) -> Result<RemoteZip> {
        Self::list(Src::Url(url.to_string()))
    }

    /// The same, for an archive on disk.
    pub fn open_file(path: &Path) -> Result<RemoteZip> {
        Self::list(Src::File(path.to_path_buf()))
    }

    fn list(src: Src) -> Result<RemoteZip> {
        let name = src.name();
        let tail = src.bytes(-65_557, 0)?;
        let eocd = tail
            .windows(4)
            .rposition(|w| w == [0x50, 0x4b, 0x05, 0x06])
            .filter(|&p| p + 22 <= tail.len())
            .ok_or_else(|| anyhow!("{name} is not a zip archive"))?;
        let n = u16le(&tail, eocd + 10) as usize;
        let size = u32le(&tail, eocd + 12) as u64;
        let off = u32le(&tail, eocd + 16) as u64;
        if off == u32::MAX as u64 || size == u32::MAX as u64 {
            bail!("{name}: a zip64 archive (over 4 GB) is not read");
        }
        let cd = if size > 0 { src.bytes(off as i64, (off + size - 1) as i64)? } else { Vec::new() };
        let mut entries = Vec::with_capacity(n);
        let mut p = 0;
        while p + 46 <= cd.len() && u32le(&cd, p) == 0x0201_4b50 {
            let (nl, xl, cl) = (u16le(&cd, p + 28) as usize, u16le(&cd, p + 30) as usize, u16le(&cd, p + 32) as usize);
            let Some(raw) = cd.get(p + 46..p + 46 + nl) else { bail!("{name}: a truncated central directory") };
            entries.push(Entry {
                name: String::from_utf8_lossy(raw).to_string(),
                method: u16le(&cd, p + 10),
                crc: u32le(&cd, p + 16),
                packed: u32le(&cd, p + 20) as u64,
                size: u32le(&cd, p + 24) as u64,
                local: u32le(&cd, p + 42) as u64,
            });
            p += 46 + nl + xl + cl;
        }
        Ok(RemoteZip { src, entries })
    }

    /// One file's bytes (at most `MAX_ENTRY` inflated, CRC-32 checked).
    pub fn read(&self, e: &Entry) -> Result<Vec<u8>> {
        if e.size > MAX_ENTRY {
            bail!("{}: {} bytes inflated, over the {} MiB limit", e.name, e.size, MAX_ENTRY >> 20);
        }
        let head = self.src.bytes(e.local as i64, e.local as i64 + 29)?;
        if head.len() < 30 || u32le(&head, 0) != 0x0403_4b50 {
            bail!("{}: no local header", e.name);
        }
        let start = e.local + 30 + u16le(&head, 26) as u64 + u16le(&head, 28) as u64;
        let packed = if e.packed > 0 { self.src.bytes(start as i64, (start + e.packed - 1) as i64)? } else { Vec::new() };
        let out = match e.method {
            0 => packed,
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(&packed, MAX_ENTRY as usize)
                .map_err(|err| anyhow!("{}: inflate: {err:?}", e.name))?,
            m => bail!("{}: compression method {m} is not read", e.name),
        };
        if crc32(&out) != e.crc {
            bail!("{}: the CRC-32 does not match (a damaged archive)", e.name);
        }
        Ok(out)
    }
}

/// CRC-32 (IEEE 802.3, as zip archives use it).
pub fn crc32(data: &[u8]) -> u32 {
    static TABLE: std::sync::OnceLock<[u32; 256]> = std::sync::OnceLock::new();
    let t = TABLE.get_or_init(|| {
        std::array::from_fn(|i| {
            let mut c = i as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            c
        })
    });
    !data.iter().fold(!0u32, |c, &b| t[((c ^ b as u32) & 0xff) as usize] ^ (c >> 8))
}

/// SHA-256 of `data` as lowercase hex (FIPS 180-4), for pinned downloads.
pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01,
        0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
        0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116, 0x1e376c08,
        0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19];
    let mut msg = data.to_vec();
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&((data.len() as u64) * 8).to_be_bytes());
    for block in msg.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([block[4 * i], block[4 * i + 1], block[4 * i + 2], block[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for (a, b) in h.iter_mut().zip(v) {
            *a = a.wrapping_add(b);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksums_match_their_standards() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(sha256_hex(b""), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        let long = vec![b'a'; 1000];
        assert_eq!(sha256_hex(&long), "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3");
    }

    #[test]
    fn entry_names_stay_inside_their_folder() {
        let e = |n: &str| Entry { name: n.into(), method: 0, packed: 0, size: 0, local: 0, crc: 0 };
        assert_eq!(e("a/b/../c.glb").base(), "c.glb");
        assert_eq!(e("..\\..\\evil.bat").base(), "evil.bat");
        assert_eq!(e("x/..").base(), "");
        assert_eq!(
            e("/etc/../Models/GLB format/chair.glb").rel_path(),
            Some(["etc", "Models", "GLB format", "chair.glb"].iter().collect())
        );
        assert_eq!(e("C:\\x\\y.png").rel_path(), Some(["x", "y.png"].iter().collect()));
        assert_eq!(e("Models/").rel_path(), None);
    }
}
