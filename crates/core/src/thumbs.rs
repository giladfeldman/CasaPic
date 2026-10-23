//! Thumbnails, in two levels (step 0, docs/bench-step0.md):
//! level 1 = the camera's embedded preview (stored during the scan, ~0.2 ms each);
//! level 2 = a 256 px thumbnail made with WIC's scaled decode (~6 ms each on all cores).
//! Both are stored upright (EXIF orientation applied) as JPEG in the index.

use std::path::Path;

use anyhow::Result;
use image::RgbImage;
use rayon::prelude::*;

use crate::db::Db;

pub const EDGE: u32 = 256;
pub const LEVEL_PREVIEW: i64 = 1;
pub const LEVEL_FULL: i64 = 2;

pub fn encode_jpeg(img: &RgbImage, quality: u8) -> Result<Vec<u8>> {
    let mut out = Vec::with_capacity(img.as_raw().len() / 10);
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality);
    img.write_with_encoder(enc)?;
    Ok(out)
}

/// The embedded preview, rotated upright if needed (only then re-encoded).
pub fn upright_preview(jpeg: Vec<u8>, orientation: i64) -> Option<Vec<u8>> {
    if orientation == 1 {
        return Some(jpeg);
    }
    let img = image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg).ok()?.to_rgb8();
    encode_jpeg(&crate::wic::orient(img, orientation), 85).ok()
}

/// A 256 px thumbnail showing the photo as edited (its recipe applied), like Picasa's.
pub fn make_thumb(path: &Path, orientation: i64, recipe: &str) -> Result<Vec<u8>> {
    let ops = crate::recipe::parse(recipe);
    // decode a little larger when the recipe crops, so the result still reaches 256 px
    let (w, h) = crate::render::output_size(1.0, 1.0, &ops);
    let need = (EDGE as f32 / w.max(h).max(0.05)).ceil() as u32;
    let img = crate::wic::orient(crate::wic::decode(path, Some(need.max(EDGE)))?, orientation);
    if ops.is_empty() {
        return encode_jpeg(&img, 85);
    }
    let mut out = crate::render::apply(&crate::render::Img::from_rgb8(&img), &ops).to_rgb8();
    if out.width().max(out.height()) > EDGE {
        let (tw, th) = crate::wic::fit(out.width(), out.height(), EDGE);
        out = image::imageops::resize(&out, tw, th, image::imageops::FilterType::Triangle);
    }
    encode_jpeg(&out, 85)
}

/// Generates level-2 thumbnails for `ids` on all cores, storing them in batches.
/// `on_batch` gets the ids just stored (for UI refresh). Returns (done, failed).
pub fn generate(db: &Db, ids: &[i64], mut on_batch: impl FnMut(&[i64])) -> Result<(usize, usize)> {
    let (mut done, mut failed) = (0, 0);
    for chunk in ids.chunks(64) {
        let jobs: Vec<(i64, std::path::PathBuf, i64, String)> = chunk
            .iter()
            .filter_map(|&id| {
                let p = db.photo(id).ok()??;
                Some((id, db.photo_path(id).ok()??, p.orientation, p.recipe))
            })
            .collect();
        let made: Vec<(i64, Option<Vec<u8>>)> =
            jobs.par_iter().map(|(id, path, o, r)| (*id, make_thumb(path, *o, r).ok())).collect();
        let ok: Vec<(i64, i64, Vec<u8>)> =
            made.iter().filter_map(|(id, j)| j.clone().map(|j| (*id, LEVEL_FULL, j))).collect();
        failed += made.len() - ok.len();
        done += ok.len();
        db.put_thumbs(&ok)?;
        // unreadable files: stop retrying them (they keep their preview, if any)
        for (id, j) in &made {
            if j.is_none() {
                db.set_thumb_level(*id, LEVEL_FULL)?;
            }
        }
        let stored: Vec<i64> = ok.iter().map(|t| t.0).collect();
        on_batch(&stored);
    }
    Ok((done, failed))
}
