//! Reads what the library needs from the first part of an image file, without decoding the
//! picture: pixel size, date taken, orientation, camera, and the embedded camera preview.

use std::io::Read;
use std::path::Path;

/// EXIF and the JPEG frame header are near the start; 256 KB covers them in practice.
const HEAD: usize = 256 * 1024;

#[derive(Debug, Default, Clone)]
pub struct Meta {
    pub width: i64,
    pub height: i64,
    /// seconds since 1970 of the camera's local wall-clock time (no time zone)
    pub taken: Option<i64>,
    pub orientation: i64,
    pub camera: Option<String>,
    /// the embedded preview JPEG, if any
    pub preview: Option<Vec<u8>>,
    pub tags: Vec<String>,
    pub caption: String,
}

pub fn read(path: &Path) -> std::io::Result<Meta> {
    let mut f = std::fs::File::open(path)?;
    let mut buf = Vec::with_capacity(HEAD);
    f.by_ref().take(HEAD as u64).read_to_end(&mut buf)?;
    let mut m = from_bytes(&buf);
    let labels = crate::tags::read(&buf);
    (m.tags, m.caption) = (labels.tags, labels.caption);
    if m.width == 0
        && let Ok((w, h)) = crate::wic::dimensions(path) {
            m.width = w as i64;
            m.height = h as i64;
        }
    Ok(m)
}

pub fn from_bytes(buf: &[u8]) -> Meta {
    let mut m = Meta { orientation: 1, ..Default::default() };
    if let Some((w, h)) = jpeg_size(buf) {
        m.width = w as i64;
        m.height = h as i64;
    }
    // Keep what can be read when part of the camera data is damaged (some programs, e.g. the
    // Python library piexif, write a directory without its closing link): one bad directory
    // must not lose the date, camera and preview held in the others.
    let read = exif::Reader::new()
        .continue_on_error(true)
        .read_from_container(&mut std::io::Cursor::new(buf))
        .or_else(|e| e.distill_partial_result(|_| ()));
    let Ok(ex) = read else {
        return m;
    };
    let field = |t: exif::Tag| ex.get_field(t, exif::In::PRIMARY);
    if let Some(o) = field(exif::Tag::Orientation).and_then(|f| f.value.get_uint(0))
        && (1..=8).contains(&o) {
            m.orientation = o as i64;
        }
    for tag in [exif::Tag::DateTimeOriginal, exif::Tag::DateTimeDigitized, exif::Tag::DateTime] {
        if let Some(f) = field(tag)
            && let exif::Value::Ascii(ref v) = f.value
                && let Some(t) = v.first().and_then(|s| exif::DateTime::from_ascii(s).ok()) {
                    m.taken = Some(days_from_civil(t.year as i64, t.month as i64, t.day as i64) * 86400
                        + t.hour as i64 * 3600
                        + t.minute as i64 * 60
                        + t.second as i64);
                    break;
                }
    }
    let text = |t: exif::Tag| {
        field(t).map(|f| f.display_value().to_string().trim_matches('"').trim().to_string()).filter(|s| !s.is_empty())
    };
    m.camera = match (text(exif::Tag::Make), text(exif::Tag::Model)) {
        (Some(a), Some(b)) if b.starts_with(&a) => Some(b),
        (Some(a), Some(b)) => Some(format!("{a} {b}")),
        (a, b) => a.or(b),
    };
    let off = ex.get_field(exif::Tag::JPEGInterchangeFormat, exif::In::THUMBNAIL).and_then(|f| f.value.get_uint(0));
    let len = ex.get_field(exif::Tag::JPEGInterchangeFormatLength, exif::In::THUMBNAIL).and_then(|f| f.value.get_uint(0));
    if let (Some(off), Some(len)) = (off, len) {
        let (off, len) = (off as usize, len as usize);
        if let Some(j) = ex.buf().get(off..off + len)
            && j.starts_with(&[0xFF, 0xD8]) {
                m.preview = Some(j.to_vec());
            }
    }
    m
}

/// Width and height from the JPEG frame header (SOFn), skipping other segments.
pub fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
    if !b.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut i = 2;
    while i + 9 < b.len() {
        if b[i] != 0xFF {
            return None;
        }
        let marker = b[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        let is_sof = matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_sof {
            let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
            let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
            return Some((w, h));
        }
        i += 2 + len;
    }
    None
}

/// Days from 1970-01-01 to the given civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

#[cfg(test)]
mod tests {
    use super::*;

    /// camera data as piexif writes it: the EXIF directory ends without its 4-byte link, so
    /// the date text sits where the link should be
    fn exif_missing_link() -> Vec<u8> {
        let date = b"2015:01:13 10:12:00\0";
        let mut t: Vec<u8> = b"MM\0*\0\0\0\x08".to_vec();
        // IFD0 at 8: Make (at 38), ExifIFD pointer (at 50); next 0
        let make_at = 8 + 2 + 24 + 4;
        let exif_at = make_at + 12;
        t.extend_from_slice(&2u16.to_be_bytes());
        t.extend_from_slice(&[&0x010Fu16.to_be_bytes()[..], &2u16.to_be_bytes(), &11u32.to_be_bytes(), &(make_at as u32).to_be_bytes()].concat());
        t.extend_from_slice(&[&0x8769u16.to_be_bytes()[..], &4u16.to_be_bytes(), &1u32.to_be_bytes(), &(exif_at as u32).to_be_bytes()].concat());
        t.extend_from_slice(&0u32.to_be_bytes());
        t.extend_from_slice(b"CasaPicLab\0\0");
        // the EXIF directory: one entry, no link, then the date text
        let text_at = exif_at + 2 + 12;
        t.extend_from_slice(&1u16.to_be_bytes());
        t.extend_from_slice(&[&0x9003u16.to_be_bytes()[..], &2u16.to_be_bytes(), &20u32.to_be_bytes(), &(text_at as u32).to_be_bytes()].concat());
        t.extend_from_slice(date);
        let img = image::RgbImage::from_pixel(16, 8, image::Rgb([10, 200, 30]));
        let j = crate::thumbs::encode_jpeg(&img, 90).unwrap();
        let mut o = vec![0xFF, 0xD8, 0xFF, 0xE1];
        o.extend_from_slice(&((2 + 6 + t.len()) as u16).to_be_bytes());
        o.extend_from_slice(b"Exif\0\0");
        o.extend_from_slice(&t);
        o.extend_from_slice(&j[2..]);
        o
    }

    #[test]
    fn damaged_camera_data_still_gives_date_and_camera() {
        let m = from_bytes(&exif_missing_link());
        assert_eq!(m.camera.as_deref(), Some("CasaPicLab"));
        assert_eq!(m.taken, Some(days_from_civil(2015, 1, 13) * 86400 + 10 * 3600 + 12 * 60));
        assert_eq!((m.width, m.height), (16, 8));
    }

    #[test]
    fn civil_days() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2015, 1, 13), 16448);
        assert_eq!(days_from_civil(2000, 3, 1), 11017);
    }
}
