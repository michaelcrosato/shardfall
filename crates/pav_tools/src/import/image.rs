//! Texture images: their kind and size from the header, and PNG decoded to RGBA8 (sRGB as
//! stored). JPEG decoding is deferred until a pack needs it (plan I9): a JPEG is listed with its
//! size and left undecoded, so its material falls back to its colour factors.

use anyhow::{Result, anyhow, bail};

use super::scene::{Image, MAX_IMAGE_SIDE};

/// An image's kind from its first bytes.
pub fn kind_of(b: &[u8]) -> &'static str {
    if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else if b.starts_with(&[0xff, 0xd8]) {
        "jpeg"
    } else if b.len() >= 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        "webp"
    } else if b.starts_with(&[0xab, b'K', b'T', b'X', b' ', b'2', b'0']) {
        "ktx2"
    } else {
        "unknown"
    }
}

/// An image's size from its header (PNG's IHDR, JPEG's start of frame), if it can be read.
pub fn size_of(b: &[u8]) -> Option<(u32, u32)> {
    let be32 = |o: usize| b.get(o..o + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]]));
    let be16 = |o: usize| b.get(o..o + 2).map(|s| u16::from_be_bytes([s[0], s[1]]) as u32);
    match kind_of(b) {
        "png" => Some((be32(16)?, be32(20)?)),
        "jpeg" => {
            let mut p = 2;
            while p + 4 <= b.len() {
                if b[p] != 0xff {
                    return None;
                }
                let m = b[p + 1];
                if m == 0xff {
                    p += 1;
                    continue;
                }
                let len = be16(p + 2)? as usize;
                // Start of frame: baseline, extended, progressive, lossless (not DHT/JPG/DAC).
                if matches!(m, 0xc0..=0xcf) && !matches!(m, 0xc4 | 0xc8 | 0xcc) {
                    return Some((be16(p + 7)?, be16(p + 5)?));
                }
                p += 2 + len;
            }
            None
        }
        _ => None,
    }
}

/// Decodes a PNG to RGBA8.
pub fn decode_png(b: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    let mut d = png::Decoder::new(std::io::Cursor::new(b));
    d.set_transformations(png::Transformations::normalize_to_color8());
    let mut r = d.read_info().map_err(|e| anyhow!("png: {e}"))?;
    let (w, h) = (r.info().width, r.info().height);
    if w > MAX_IMAGE_SIDE || h > MAX_IMAGE_SIDE {
        bail!("png: {w}x{h} is over the {MAX_IMAGE_SIDE} px limit");
    }
    let mut buf = vec![0u8; r.output_buffer_size().ok_or_else(|| anyhow!("png: too large"))?];
    let out = r.next_frame(&mut buf).map_err(|e| anyhow!("png: {e}"))?;
    let px = &buf[..out.buffer_size()];
    let n = (w * h) as usize;
    let rgba: Vec<u8> = match out.color_type {
        png::ColorType::Rgba => px.to_vec(),
        png::ColorType::Rgb => px.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, 255]).collect(),
        png::ColorType::GrayscaleAlpha => px.as_chunks::<2>().0.iter().flat_map(|&[g, a]| [g, g, g, a]).collect(),
        png::ColorType::Grayscale => px.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => bail!("png: an indexed image was not expanded"),
    };
    if rgba.len() != n * 4 {
        bail!("png: {} bytes of pixels for {w}x{h}", rgba.len());
    }
    Ok((w, h, rgba))
}

/// An image read from its bytes: decoded when `decode` (PNG within the size limit), else only
/// its kind and size. Why it was not decoded goes to `warnings`.
pub fn read(name: &str, b: &[u8], decode: bool, warnings: &mut Vec<String>) -> Image {
    let kind = kind_of(b).to_string();
    let (w, h) = size_of(b).unwrap_or((0, 0));
    let mut img = Image { name: name.to_string(), kind, w, h, rgba: Vec::new() };
    if !decode {
        return img;
    }
    if img.kind != "png" {
        warnings.push(format!("image {name}: {} is not decoded (PNG only): its material uses its colour factors", img.kind));
        return img;
    }
    match decode_png(b) {
        Ok((w, h, rgba)) => {
            (img.w, img.h, img.rgba) = (w, h, rgba);
        }
        Err(e) => warnings.push(format!("image {name}: {e}: its material uses its colour factors")),
    }
    img
}
