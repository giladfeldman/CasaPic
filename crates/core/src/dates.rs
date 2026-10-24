//! Adjust Date and Time: writes a new date into a JPEG's camera data (EXIF), as Picasa does,
//! after keeping the untouched original in `.casapicoriginals` (the same safety as Save).
//!
//! Only the date text changes: the three EXIF dates (DateTimeOriginal, DateTimeDigitized and
//! DateTime) are fixed-length "YYYY:MM:DD HH:MM:SS" fields, so the new date is written over the
//! old one byte for byte and nothing else in the file moves; the picture is not re-encoded. A
//! JPEG with no camera data at all gets a small EXIF block holding only the date.

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail};

const DATE_LEN: usize = 19;

/// The positions (in the file) of the EXIF date fields, each 19 ASCII characters.
fn date_fields(file: &[u8]) -> Option<Vec<usize>> {
    if !file.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut i = 2;
    while i + 4 <= file.len() && file[i] == 0xFF {
        let marker = file[i + 1];
        let len = u16::from_be_bytes([file[i + 2], file[i + 3]]) as usize;
        if marker == 0xE1 && file.get(i + 4..i + 10) == Some(b"Exif\0\0") {
            let base = i + 10;
            let tiff = file.get(base..i + 2 + len)?;
            return Some(tiff_dates(tiff)?.into_iter().map(|o| base + o).collect());
        }
        if marker == 0xDA {
            break;
        }
        i += 2 + len;
    }
    Some(Vec::new())
}

fn tiff_dates(t: &[u8]) -> Option<Vec<usize>> {
    let le = match t.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let rd16 = |o: usize| -> Option<usize> {
        let x = [*t.get(o)?, *t.get(o + 1)?];
        Some(if le { u16::from_le_bytes(x) } else { u16::from_be_bytes(x) } as usize)
    };
    let rd32 = |o: usize| -> Option<usize> {
        let x = [*t.get(o)?, *t.get(o + 1)?, *t.get(o + 2)?, *t.get(o + 3)?];
        Some(if le { u32::from_le_bytes(x) } else { u32::from_be_bytes(x) } as usize)
    };
    let mut out = Vec::new();
    let walk = |ifd: usize, wanted: &[usize], out: &mut Vec<usize>| -> Option<Option<usize>> {
        let n = rd16(ifd)?;
        let mut exif_ifd = None;
        for k in 0..n {
            let e = ifd + 2 + k * 12;
            let tag = rd16(e)?;
            if tag == 0x8769 {
                exif_ifd = Some(rd32(e + 8)?);
            }
            // type 2 = ASCII, 20 bytes with the closing NUL (stored elsewhere, at the offset)
            if wanted.contains(&tag) && rd16(e + 2)? == 2 && rd32(e + 4)? >= DATE_LEN {
                let at = rd32(e + 8)?;
                let text = t.get(at..at + DATE_LEN)?;
                if parse(text).is_some() {
                    out.push(at);
                }
            }
        }
        Some(exif_ifd)
    };
    let exif_ifd = walk(rd32(4)?, &[0x0132], &mut out)?;
    if let Some(e) = exif_ifd {
        walk(e, &[0x9003, 0x9004], &mut out)?;
    }
    Some(out)
}

/// "YYYY:MM:DD HH:MM:SS" as seconds since 1970 of that wall-clock time (no time zone), the
/// same scale as the library's dates.
pub fn parse(text: &[u8]) -> Option<i64> {
    let s = std::str::from_utf8(text).ok()?;
    let n = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0, 4)?, n(5, 7)?, n(8, 10)?, n(11, 13)?, n(14, 16)?, n(17, 19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 60 {
        return None;
    }
    Some(days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + se)
}

pub fn format(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}:{m:02}:{d:02} {:02}:{:02}:{:02}", rem / 3600, rem / 60 % 60, rem % 60)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

/// A minimal EXIF block (little-endian) holding only DateTimeOriginal: IFD0 with a pointer to
/// the EXIF IFD, which has the one date.
fn minimal_exif(date: &str) -> Vec<u8> {
    let mut t: Vec<u8> = Vec::new();
    t.extend_from_slice(b"II*\0");
    t.extend_from_slice(&8u32.to_le_bytes()); // IFD0 at 8
    // IFD0: 1 entry (ExifIFD pointer) = 2 + 12 + 4 bytes -> EXIF IFD at 26
    t.extend_from_slice(&1u16.to_le_bytes());
    t.extend_from_slice(&0x8769u16.to_le_bytes());
    t.extend_from_slice(&4u16.to_le_bytes());
    t.extend_from_slice(&1u32.to_le_bytes());
    t.extend_from_slice(&26u32.to_le_bytes());
    t.extend_from_slice(&0u32.to_le_bytes());
    // EXIF IFD: 1 entry (DateTimeOriginal) -> its text at 26 + 18 = 44
    t.extend_from_slice(&1u16.to_le_bytes());
    t.extend_from_slice(&0x9003u16.to_le_bytes());
    t.extend_from_slice(&2u16.to_le_bytes());
    t.extend_from_slice(&20u32.to_le_bytes());
    t.extend_from_slice(&44u32.to_le_bytes());
    t.extend_from_slice(&0u32.to_le_bytes());
    t.extend_from_slice(date.as_bytes());
    t.push(0);
    let mut seg = vec![0xFF, 0xE1];
    seg.extend_from_slice(&((2 + 6 + t.len()) as u16).to_be_bytes());
    seg.extend_from_slice(b"Exif\0\0");
    seg.extend_from_slice(&t);
    seg
}

/// How to change a photo's date.
#[derive(Debug, Clone, Copy)]
pub enum Change {
    /// move every date by this many seconds (a camera clock that was off)
    Shift(i64),
    /// set the date to this time (seconds, same scale as `parse`)
    Set(i64),
}

/// The new file bytes, or None when the photo's date is already that.
/// `fallback` is the date shown for a photo without camera data (its file date).
pub fn rewrite(file: &[u8], change: Change, fallback: i64) -> Result<Option<Vec<u8>>> {
    let Some(fields) = date_fields(file) else { bail!("only JPEG photos can have their date changed") };
    let mut out = file.to_vec();
    if fields.is_empty() {
        // no date in the camera data: refuse if there is EXIF (it would need rebuilding);
        // otherwise add a small EXIF block with the date
        if file.windows(6).take(65536).any(|w| w == b"Exif\0\0") {
            bail!("this photo's camera data has no date field; CasaPic cannot add one to it yet");
        }
        let when = match change {
            Change::Shift(s) => fallback + s,
            Change::Set(t) => t,
        };
        let mut rest = &file[2..];
        if rest.len() > 4 && rest[0] == 0xFF && rest[1] == 0xE0 {
            let len = u16::from_be_bytes([rest[2], rest[3]]) as usize;
            rest = &rest[2 + len..];
        }
        let mut o = vec![0xFF, 0xD8];
        o.extend_from_slice(&minimal_exif(&format(when)));
        o.extend_from_slice(rest);
        return Ok(Some(o));
    }
    for at in fields {
        let old = parse(&file[at..at + DATE_LEN]).expect("checked in tiff_dates");
        let new = match change {
            Change::Shift(s) => old + s,
            Change::Set(t) => t,
        };
        out[at..at + DATE_LEN].copy_from_slice(format(new).as_bytes());
    }
    Ok((out != file).then_some(out))
}

/// Changes the date inside `folder/file_name`, keeping the untouched original first (unless an
/// earlier Save or date change already kept it). Returns false when nothing needed changing.
pub fn adjust(folder: &Path, file_name: &str, change: Change, fallback: i64) -> Result<bool> {
    let photo = folder.join(file_name);
    let mut bytes = Vec::new();
    std::fs::File::open(&photo).and_then(|mut f| f.read_to_end(&mut bytes)).with_context(|| format!("read {}", photo.display()))?;
    let Some(new) = rewrite(&bytes, change, fallback)? else { return Ok(false) };
    crate::save::install(folder, file_name, &new)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn jpeg_with_exif(date: &str) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(16, 8, image::Rgb([10, 200, 30]));
        let j = crate::thumbs::encode_jpeg(&img, 90).unwrap();
        let mut o = vec![0xFF, 0xD8];
        o.extend_from_slice(&minimal_exif(date));
        o.extend_from_slice(&j[2..]);
        o
    }

    #[test]
    fn dates_round_trip() {
        for s in ["1970:01:01 00:00:00", "2015:02:28 23:59:59", "2016:02:29 12:00:00", "1999:12:31 08:05:09"] {
            assert_eq!(format(parse(s.as_bytes()).unwrap()), s);
        }
        assert!(parse(b"2015:13:01 00:00:00").is_none());
    }

    #[test]
    fn shift_changes_only_the_date_bytes() {
        let f = jpeg_with_exif("2015:01:31 23:30:00");
        let out = rewrite(&f, Change::Shift(3600), 0).unwrap().unwrap();
        assert_eq!(out.len(), f.len());
        let diff: Vec<usize> = (0..f.len()).filter(|&i| f[i] != out[i]).collect();
        assert!(diff.len() <= DATE_LEN && diff.iter().all(|&i| i - diff[0] < DATE_LEN));
        let m = crate::meta::from_bytes(&out);
        assert_eq!(m.taken, parse(b"2015:02:01 00:30:00"), "the library reads the new date");
        assert!(rewrite(&out, Change::Set(m.taken.unwrap()), 0).unwrap().is_none(), "no change, no write");
    }

    #[test]
    fn a_jpeg_without_camera_data_gets_one() {
        let img = image::RgbImage::from_pixel(16, 8, image::Rgb([10, 200, 30]));
        let plain = crate::thumbs::encode_jpeg(&img, 90).unwrap();
        let when = parse(b"2001:09:09 01:46:40").unwrap();
        let out = rewrite(&plain, Change::Set(when), 0).unwrap().unwrap();
        assert_eq!(crate::meta::from_bytes(&out).taken, Some(when));
        assert_eq!(image::load_from_memory(&out).unwrap().to_rgb8(), image::load_from_memory(&plain).unwrap().to_rgb8(), "same picture");
        assert!(rewrite(b"\x89PNG....", Change::Set(when), 0).is_err());
    }

    #[test]
    fn adjust_keeps_the_original_and_revert_restores_it() {
        let d = tempfile::tempdir().unwrap();
        let f = jpeg_with_exif("2015:01:31 23:30:00");
        std::fs::write(d.path().join("a.jpg"), &f).unwrap();
        assert!(adjust(d.path(), "a.jpg", Change::Shift(-86400), 0).unwrap());
        let m = crate::meta::read(&d.path().join("a.jpg")).unwrap();
        assert_eq!(m.taken, parse(b"2015:01:30 23:30:00"));
        assert_eq!(std::fs::read(crate::save::backup_of(d.path(), "a.jpg")).unwrap(), f);
        assert!(crate::save::revert(d.path(), "a.jpg").unwrap());
        assert_eq!(std::fs::read(d.path().join("a.jpg")).unwrap(), f);
    }
}
