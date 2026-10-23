//! Export Picture to Folder: writes an edited copy; the original is only read.
//!
//! The copy keeps the original's camera data (EXIF), with two corrections so other programs
//! show it right: orientation is set to "upright" (the rotation is now in the pixels) and the
//! embedded preview is unlinked (it would show the unedited picture).

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

use crate::recipe::Op;
use crate::render::{self, Img};

#[derive(Debug, Clone)]
pub struct ExportOptions {
    /// long edge in pixels; None = original size
    pub max_edge: Option<u32>,
    /// JPEG quality 1–100
    pub quality: u8,
}

/// Exports `src` with `ops` applied into `dest_dir`, never overwriting: returns the new path.
pub fn export(src: &Path, orientation: i64, ops: &[Op], dest_dir: &Path, opt: &ExportOptions) -> Result<PathBuf> {
    std::fs::create_dir_all(dest_dir)?;
    let (w0, h0) = crate::wic::dimensions(src)?;
    // upright size, then size after the recipe
    let (uw, uh) = if orientation >= 5 { (h0 as f32, w0 as f32) } else { (w0 as f32, h0 as f32) };
    let (fw, fh) = render::output_size(uw, uh, ops);
    let decode_edge = match opt.max_edge {
        Some(m) if fw.max(fh) > m as f32 => {
            let scale = m as f32 / fw.max(fh);
            Some(((w0.max(h0) as f32) * scale).ceil() as u32 + 2)
        }
        _ => None,
    };
    let base = crate::wic::orient(crate::wic::decode(src, decode_edge)?, orientation);
    let mut out = render::apply(&Img::from_rgb8(&base), ops).to_rgb8();
    if let Some(m) = opt.max_edge {
        if out.width().max(out.height()) > m {
            let (tw, th) = crate::wic::fit(out.width(), out.height(), m);
            out = image::imageops::resize(&out, tw, th, image::imageops::FilterType::CatmullRom);
        }
    }
    let jpeg = crate::thumbs::encode_jpeg(&out, opt.quality.clamp(1, 100))?;
    let jpeg = match original_exif(src) {
        Some(app1) => insert_app1(&jpeg, &app1),
        None => jpeg,
    };
    let stem = src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "photo".into());
    let mut dest = dest_dir.join(format!("{stem}.jpg"));
    let mut n = 1;
    while dest.exists() {
        dest = dest_dir.join(format!("{stem}-{n}.jpg"));
        n += 1;
    }
    if dest.canonicalize().ok() == src.canonicalize().ok() {
        bail!("refusing to overwrite the original");
    }
    std::fs::write(&dest, jpeg).with_context(|| format!("write {}", dest.display()))?;
    Ok(dest)
}

/// The original's EXIF APP1 segment, patched (orientation = 1, embedded preview unlinked).
fn original_exif(src: &Path) -> Option<Vec<u8>> {
    let mut head = Vec::new();
    std::fs::File::open(src).ok()?.take(256 * 1024).read_to_end(&mut head).ok()?;
    if !head.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut i = 2;
    while i + 4 <= head.len() && head[i] == 0xFF {
        let marker = head[i + 1];
        let len = u16::from_be_bytes([head[i + 2], head[i + 3]]) as usize;
        if marker == 0xE1 && head.get(i + 4..i + 10) == Some(b"Exif\0\0") {
            let mut seg = head.get(i..i + 2 + len)?.to_vec();
            patch_tiff(&mut seg[10..]);
            return Some(seg);
        }
        if marker == 0xDA {
            break;
        }
        i += 2 + len;
    }
    None
}

/// In a TIFF/EXIF block: set Orientation (0x0112) to 1 and cut the link to IFD1.
fn patch_tiff(t: &mut [u8]) -> Option<()> {
    let le = match t.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let rd16 = |b: &[u8], o: usize| -> Option<u16> {
        let x = [*b.get(o)?, *b.get(o + 1)?];
        Some(if le { u16::from_le_bytes(x) } else { u16::from_be_bytes(x) })
    };
    let rd32 = |b: &[u8], o: usize| -> Option<u32> {
        let x = [*b.get(o)?, *b.get(o + 1)?, *b.get(o + 2)?, *b.get(o + 3)?];
        Some(if le { u32::from_le_bytes(x) } else { u32::from_be_bytes(x) })
    };
    let ifd0 = rd32(t, 4)? as usize;
    let n = rd16(t, ifd0)? as usize;
    for k in 0..n {
        let e = ifd0 + 2 + k * 12;
        if rd16(t, e)? == 0x0112 {
            let one = if le { 1u16.to_le_bytes() } else { 1u16.to_be_bytes() };
            t.get_mut(e + 8..e + 10)?.copy_from_slice(&one);
        }
    }
    let next = ifd0 + 2 + n * 12;
    t.get_mut(next..next + 4)?.copy_from_slice(&[0, 0, 0, 0]);
    Some(())
}

/// Puts an APP1 segment right after the JPEG start marker (dropping any JFIF APP0 header,
/// since EXIF files should start with APP1).
fn insert_app1(jpeg: &[u8], app1: &[u8]) -> Vec<u8> {
    let mut rest = &jpeg[2..];
    if rest.len() > 4 && rest[0] == 0xFF && rest[1] == 0xE0 {
        let len = u16::from_be_bytes([rest[2], rest[3]]) as usize;
        rest = &rest[2 + len..];
    }
    let mut out = Vec::with_capacity(jpeg.len() + app1.len());
    out.extend_from_slice(&[0xFF, 0xD8]);
    out.extend_from_slice(app1);
    out.extend_from_slice(rest);
    out
}
