//! Downloads for the importer: whole files, and single files out of a zip archive on the web
//! (read with ranged requests from its central directory, so one take of a 1.5 GB dataset costs
//! only its own bytes). Requests go through `curl`, which every cloud container has and which
//! follows the environment's proxy settings.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Result, anyhow, bail};

/// Where downloaded captures are kept: $PAV_MOCAP_CACHE, else .cache/mocap beside the anim
/// folder (not committed).
pub fn cache_dir() -> PathBuf {
    if let Ok(d) = std::env::var("PAV_MOCAP_CACHE") {
        return d.into();
    }
    let anim = pav_core::anim::disk_dir().unwrap_or_else(|| PathBuf::from("anim"));
    let root = anim.parent().filter(|p| !p.as_os_str().is_empty()).map(Path::to_path_buf).unwrap_or_else(|| ".".into());
    root.join(".cache").join("mocap")
}

fn curl(args: &[&str]) -> Result<Vec<u8>> {
    let out = Command::new("curl")
        .args(["-sSfL", "--retry", "3", "--connect-timeout", "30"])
        .args(args)
        .output()
        .map_err(|e| anyhow!("curl is needed to download: {e}"))?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(out.stdout)
}

/// Downloads `url` to `path` unless it is already there. Returns whether it downloaded.
pub fn get(url: &str, path: &Path) -> Result<bool> {
    if path.exists() {
        return Ok(false);
    }
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let part = path.with_extension("part");
    curl(&["-o", &part.to_string_lossy(), url]).map_err(|e| anyhow!("{url}: {e}"))?;
    std::fs::rename(&part, path)?;
    Ok(true)
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
        let eocd =
            tail.windows(4).rposition(|w| w == [0x50, 0x4b, 0x05, 0x06]).ok_or_else(|| anyhow!("{name} is not a zip archive"))?;
        let n = u16le(&tail, eocd + 10) as usize;
        let size = u32le(&tail, eocd + 12) as u64;
        let off = u32le(&tail, eocd + 16) as u64;
        if off == u32::MAX as u64 || size == u32::MAX as u64 {
            bail!("{name}: a zip64 archive (over 4 GB) is not read");
        }
        let cd = src.bytes(off as i64, (off + size - 1) as i64)?;
        let mut entries = Vec::with_capacity(n);
        let mut p = 0;
        while p + 46 <= cd.len() && u32le(&cd, p) == 0x0201_4b50 {
            let (nl, xl, cl) = (u16le(&cd, p + 28) as usize, u16le(&cd, p + 30) as usize, u16le(&cd, p + 32) as usize);
            entries.push(Entry {
                name: String::from_utf8_lossy(&cd[p + 46..p + 46 + nl]).to_string(),
                method: u16le(&cd, p + 10),
                packed: u32le(&cd, p + 20) as u64,
                size: u32le(&cd, p + 24) as u64,
                local: u32le(&cd, p + 42) as u64,
            });
            p += 46 + nl + xl + cl;
        }
        Ok(RemoteZip { src, entries })
    }

    /// One file's bytes.
    pub fn read(&self, e: &Entry) -> Result<Vec<u8>> {
        let head = self.src.bytes(e.local as i64, e.local as i64 + 29)?;
        if head.len() < 30 || u32le(&head, 0) != 0x0403_4b50 {
            bail!("{}: no local header", e.name);
        }
        let start = e.local + 30 + u16le(&head, 26) as u64 + u16le(&head, 28) as u64;
        let packed = if e.packed > 0 { self.src.bytes(start as i64, (start + e.packed - 1) as i64)? } else { Vec::new() };
        match e.method {
            0 => Ok(packed),
            8 => miniz_oxide::inflate::decompress_to_vec(&packed).map_err(|err| anyhow!("{}: inflate: {err:?}", e.name)),
            m => bail!("{}: compression method {m} is not read", e.name),
        }
    }
}
