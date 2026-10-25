//! Renders a recipe onto a picture, and analyses pictures for the automatic fixes.
//!
//! All fixes are written from scratch to behave like Picasa's (clean room; see SPEC.md §5).
//! Pixels are processed as 0–1 floats in the file's own (sRGB) encoding, rows in parallel.

use image::RgbImage;
use rayon::prelude::*;

use crate::recipe::Op;

// The automatic fixes follow rules measured from Picasa 3.9's own exports of lab photos
// (tools/parity, docs/results-build1.md). Auto Contrast, I'm Feeling Lucky and Auto Color
// are straight-line changes per colour; Fill Light is a curve per colour, weighted by the
// pixel's brightness.

/// Share of pixels allowed to clip at each end when stretching levels (0.5%).
const CLIP: f32 = 0.005;
/// I'm Feeling Lucky moves each colour's black and white points this far from the shared
/// points toward that colour's own (fitted: about 0.7).
const LUCKY_COLOR: f32 = 0.7;
/// Auto Color (chosen on 17 lab photos by colour difference, checked leaving each photo out):
/// gain = (reference / colour's average)^COLOR_POWER over pixels brighter than COLOR_MIN_LUMA,
/// kept within COLOR_RANGE, then scaled so overall brightness is unchanged.
const COLOR_POWER: f32 = 0.9;
const COLOR_RANGE: (f32, f32) = (0.88, 1.12);
const COLOR_MIN_LUMA: f32 = 0.3;
/// Fill Light, measured at five slider amounts (12 lab photos each): each colour value c
/// moves toward K * c^gamma, by (1 - brightness). Rows: amount, K, gamma; straight lines
/// between them.
const FILL_TABLE: [(f32, f32, f32); 6] = [
    (0.0, 1.0, 1.0),
    (0.15, 1.1023, 0.9301),
    (0.3, 1.2108, 0.848),
    (0.504673, 1.358, 0.7271),
    (0.7, 1.4828, 0.6017),
    (1.0, 1.5821, 0.3894),
];
/// Luminance weights (Rec. 601, as used by JPEG).
const WR: f32 = 0.299;
const WG: f32 = 0.587;
const WB: f32 = 0.114;

#[derive(Clone)]
pub struct Img {
    pub w: usize,
    pub h: usize,
    /// RGB, 3 floats per pixel, 0–1
    pub px: Vec<f32>,
}

impl Img {
    pub fn from_rgb8(img: &RgbImage) -> Img {
        Img { w: img.width() as usize, h: img.height() as usize, px: img.as_raw().iter().map(|&v| v as f32 / 255.0).collect() }
    }

    pub fn to_rgb8(&self) -> RgbImage {
        let raw = self.px.par_iter().map(|&v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8).collect();
        RgbImage::from_raw(self.w as u32, self.h as u32, raw).expect("size")
    }

    /// RGBA bytes for the screen.
    pub fn to_rgba8(&self) -> Vec<u8> {
        let mut out = vec![255u8; self.w * self.h * 4];
        out.par_chunks_mut(4).zip(self.px.par_chunks(3)).for_each(|(o, p)| {
            for c in 0..3 {
                o[c] = (p[c].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            }
        });
        out
    }

    fn rows_mut(&mut self) -> rayon::slice::ChunksMut<'_, f32> {
        let w = self.w;
        self.px.par_chunks_mut(w * 3)
    }
}

pub(crate) fn luma(p: &[f32]) -> f32 {
    WR * p[0] + WG * p[1] + WB * p[2]
}

/// Picture size after the recipe, for a source of `w` x `h` (used to plan exports).
pub fn output_size(w: f32, h: f32, ops: &[Op]) -> (f32, f32) {
    let (mut w, mut h) = (w, h);
    for op in ops {
        match op {
            Op::Rotate(q) if q.rem_euclid(2) == 1 => std::mem::swap(&mut w, &mut h),
            Op::Crop { x0, y0, x1, y1 } => {
                w *= (x1 - x0).abs();
                h *= (y1 - y0).abs();
            }
            Op::Straighten(a) => {
                let s = straighten_scale(w, h, *a);
                w *= s;
                h *= s;
            }
            _ => {}
        }
    }
    (w, h)
}

/// Renders a recipe on a full-size picture.
pub fn apply(src: &Img, ops: &[Op]) -> Img {
    apply_scaled(src, ops, 1.0)
}

/// Renders a recipe on a picture that has `scale` pixels per pixel of the original photo
/// (0.25 for a 1000 px preview of a 4000 px photo). Only the effects that work at the scale of
/// the original's pixels (Sharpen, Glow, Film Grain) use it.
pub fn apply_scaled(src: &Img, ops: &[Op], scale: f32) -> Img {
    let mut img = src.clone();
    for op in ops {
        img = apply_one_scaled(img, op, scale);
    }
    img
}

pub fn apply_one(img: Img, op: &Op) -> Img {
    apply_one_scaled(img, op, 1.0)
}

pub fn apply_one_scaled(img: Img, op: &Op, scale: f32) -> Img {
    use crate::effects as fx;
    let mut img = img;
    match op {
        Op::Tune { fill, highlights, shadows, neutral, temp } => {
            fx::tune(&mut img, *fill, *highlights, *shadows, *neutral, *temp);
            return img;
        }
        Op::Sharpen(a) => return fx::sharpen(&img, *a, scale),
        Op::Sepia => {
            fx::sepia(&mut img);
            return img;
        }
        Op::BlackWhite => {
            fx::black_white(&mut img);
            return img;
        }
        Op::Warmify => {
            fx::warmify(&mut img);
            return img;
        }
        Op::Grain { amount, lighten } => {
            fx::grain(&mut img, *amount, *lighten, scale);
            return img;
        }
        Op::Tint { fade, color } => {
            fx::tint(&mut img, *fade, *color);
            return img;
        }
        Op::Saturation(a) => {
            fx::saturation(&mut img, *a);
            return img;
        }
        Op::SoftFocus { x, y, size, amount } => return fx::soft_focus(&img, *x, *y, *size, *amount),
        Op::Glow { intensity, radius } => return fx::glow(&img, *intensity, *radius, scale),
        Op::FilteredBw(c) => {
            fx::filtered_bw(&mut img, *c);
            return img;
        }
        Op::FocalBw { x, y, size, sharpness } => {
            fx::focal_bw(&mut img, *x, *y, *size, *sharpness);
            return img;
        }
        Op::GradTint { x, y, feather, shade, color, turn } => {
            fx::grad_tint(&mut img, *x, *y, *feather, *shade, *color, *turn);
            return img;
        }
        Op::TuneImage { brightness, contrast, saturation, ambiance, highlights, shadows, warmth } => {
            crate::studio::tune_image(&mut img, *brightness, *contrast, *saturation, *ambiance, *highlights, *shadows, *warmth);
            return img;
        }
        Op::Details { structure, sharpening } => return crate::studio::details(&img, *structure, *sharpening, scale),
        Op::Vignette { outer, inner, size, x, y } => {
            crate::studio::vignette(&mut img, *outer, *inner, *size, *x, *y);
            return img;
        }
        Op::HdrScape { strength, brightness, saturation } => {
            crate::studio::hdr_scape(&mut img, *strength, *brightness, *saturation);
            return img;
        }
        _ => {}
    }
    match op {
        Op::Rotate(q) => rotate_quarter(&img, q.rem_euclid(4)),
        Op::Flip(b) => flip(&img, *b),
        Op::Crop { x0, y0, x1, y1 } => crop(&img, *x0, *y0, *x1, *y1),
        Op::Straighten(a) => straighten(&img, *a),
        Op::AutoContrast { lo, hi } => {
            let mut img = img;
            levels_shared(&mut img, *lo, *hi);
            img
        }
        Op::AutoColor { gain } => {
            let mut img = img;
            gains(&mut img, *gain);
            img
        }
        Op::Lucky { lo, hi } => {
            let mut img = img;
            levels_per_color(&mut img, *lo, *hi);
            img
        }
        Op::Fill(a) => {
            let mut img = img;
            fill_light(&mut img, *a);
            img
        }
        _ => img,
    }
}

// ---------- analysis for the automatic fixes ----------

fn percentiles(values: impl Iterator<Item = f32>, n: usize) -> (f32, f32) {
    const BINS: usize = 1024;
    let mut hist = [0u32; BINS];
    for v in values {
        hist[((v.clamp(0.0, 1.0)) * (BINS - 1) as f32) as usize] += 1;
    }
    let cut = ((n as f32) * CLIP) as u32;
    let (mut acc, mut lo) = (0u32, 0usize);
    for (i, &c) in hist.iter().enumerate() {
        acc += c;
        if acc > cut {
            lo = i;
            break;
        }
    }
    let (mut acc, mut hi) = (0u32, BINS - 1);
    for (i, &c) in hist.iter().enumerate().rev() {
        acc += c;
        if acc > cut {
            hi = i;
            break;
        }
    }
    (lo as f32 / (BINS - 1) as f32, hi as f32 / (BINS - 1) as f32)
}

/// One black and one white point shared by all colours: the darkest 0.5% of each pixel's
/// smallest colour value and the brightest 0.5% of its largest. Stretching all three colours
/// alike keeps hues unchanged (this matched Picasa's Auto Contrast exactly in the lab).
fn shared_points(img: &Img) -> (f32, f32) {
    let n = img.w * img.h;
    let (lo, _) = percentiles(img.px.chunks(3).map(|p| p[0].min(p[1]).min(p[2])), n);
    let (_, hi) = percentiles(img.px.chunks(3).map(|p| p[0].max(p[1]).max(p[2])), n);
    safe_levels(lo, hi)
}

pub fn contrast_op(img: &Img) -> Op {
    let (lo, hi) = shared_points(img);
    Op::AutoContrast { lo, hi }
}

/// Auto Color: one gain per colour, no offset. Each colour's average over the pixels that are
/// not dark is pulled toward the geometric mean of the three, within 0.88x-1.12x; the gains are
/// then scaled so brightness (Rec. 601) stays the same. An approximation of Picasa's rule,
/// not an exact match (docs/results-build1.md).
pub fn color_op(img: &Img) -> Op {
    let mean_over = |min_luma: f32| {
        let (mut m, mut k) = ([0f64; 3], 0usize);
        for p in img.px.chunks(3).filter(|p| (min_luma..1.0).contains(&luma(p))) {
            for c in 0..3 {
                m[c] += p[c] as f64;
            }
            k += 1;
        }
        (m.map(|v| (v / k.max(1) as f64).max(1e-3) as f32), k)
    };
    let (mut m, k) = mean_over(COLOR_MIN_LUMA);
    if k * 1000 < img.w * img.h {
        m = mean_over(f32::MIN).0; // almost nothing bright: use every pixel
    }
    let reference = (m[0] * m[1] * m[2]).cbrt();
    let raw = m.map(|v| (reference / v).powf(COLOR_POWER).clamp(COLOR_RANGE.0, COLOR_RANGE.1));
    let k = WR * raw[0] + WG * raw[1] + WB * raw[2];
    Op::AutoColor { gain: raw.map(|g| g / k) }
}

/// Never stretch a nearly flat picture into noise.
fn safe_levels(lo: f32, hi: f32) -> (f32, f32) {
    if hi - lo < 0.1 { (0.0, 1.0) } else { (lo, hi) }
}

/// I'm Feeling Lucky: levels per colour, with each colour's points 70% of the way from the
/// shared points (Auto Contrast's) to that colour's own 0.5% / 99.5% points. This both
/// stretches contrast and removes much of a colour cast.
pub fn lucky_op(img: &Img) -> Op {
    let n = img.w * img.h;
    let (slo, shi) = shared_points(img);
    let mut lo = [slo; 3];
    let mut hi = [shi; 3];
    for c in 0..3 {
        let (l, h) = percentiles(img.px.chunks(3).map(|p| p[c]), n);
        let (l, h) = safe_levels(slo + LUCKY_COLOR * (l - slo), shi + LUCKY_COLOR * (h - shi));
        lo[c] = l;
        hi[c] = h;
    }
    Op::Lucky { lo, hi }
}

// ---------- tone and colour ----------

fn levels_shared(img: &mut Img, lo: f32, hi: f32) {
    let k = 1.0 / (hi - lo).max(1e-3);
    img.rows_mut().for_each(|row| {
        for v in row.iter_mut() {
            *v = (*v - lo) * k;
        }
    });
}

fn levels_per_color(img: &mut Img, lo: [f32; 3], hi: [f32; 3]) {
    let k = [0, 1, 2].map(|c| 1.0 / (hi[c] - lo[c]).max(1e-3));
    img.rows_mut().for_each(|row| {
        for p in row.chunks_mut(3) {
            for c in 0..3 {
                p[c] = (p[c] - lo[c]) * k[c];
            }
        }
    });
}

fn gains(img: &mut Img, g: [f32; 3]) {
    img.rows_mut().for_each(|row| {
        for p in row.chunks_mut(3) {
            for c in 0..3 {
                p[c] *= g[c];
            }
        }
    });
}

/// Fill Light's (K, gamma) for an amount, between the measured rows of FILL_TABLE.
fn fill_params(a: f32) -> (f32, f32) {
    let a = a.clamp(0.0, 1.0);
    for w in FILL_TABLE.windows(2) {
        let ((a0, k0, g0), (a1, k1, g1)) = (w[0], w[1]);
        if a <= a1 {
            let t = (a - a0) / (a1 - a0);
            return (k0 + t * (k1 - k0), g0 + t * (g1 - g0));
        }
    }
    let (_, k, g) = FILL_TABLE[FILL_TABLE.len() - 1];
    (k, g)
}

/// Brightens dark colours most and leaves bright pixels nearly alone: each colour value c
/// becomes c + (1 - brightness) * (K * c^gamma - c). Measured from Picasa: the lift works on
/// each colour separately (not one factor for all three) and shrinks as the pixel brightens.
pub fn fill_value(c: f32, y: f32, a: f32) -> f32 {
    let (k, g) = fill_params(a);
    fill_with(c, y, k, g)
}

fn fill_with(c: f32, y: f32, k: f32, g: f32) -> f32 {
    let c0 = c.max(0.0);
    c + (1.0 - y.clamp(0.0, 1.0)) * (k * c0.powf(g) - c0)
}

pub(crate) fn fill_light(img: &mut Img, a: f32) {
    if a <= 0.0 {
        return;
    }
    let (k, g) = fill_params(a);
    img.rows_mut().for_each(|row| {
        for p in row.chunks_mut(3) {
            let y = luma(p);
            for v in p.iter_mut() {
                *v = fill_with(*v, y, k, g);
            }
        }
    });
}

// ---------- geometry ----------

/// Mirrors the picture: bit 1 left-right, bit 2 top-bottom.
fn flip(img: &Img, b: u8) -> Img {
    let (w, h) = (img.w, img.h);
    let mut px = vec![0.0f32; img.px.len()];
    px.par_chunks_mut(w * 3).enumerate().for_each(|(y, row)| {
        let sy = if b & 2 != 0 { h - 1 - y } else { y };
        for x in 0..w {
            let sx = if b & 1 != 0 { w - 1 - x } else { x };
            let s = (sy * w + sx) * 3;
            row[x * 3..x * 3 + 3].copy_from_slice(&img.px[s..s + 3]);
        }
    });
    Img { w, h, px }
}

fn rotate_quarter(img: &Img, q: i32) -> Img {
    if q == 0 {
        return img.clone();
    }
    let (w, h) = (img.w, img.h);
    let (nw, nh) = if q % 2 == 1 { (h, w) } else { (w, h) };
    let mut px = vec![0.0f32; img.px.len()];
    px.par_chunks_mut(nw * 3).enumerate().for_each(|(y, row)| {
        for x in 0..nw {
            let (sx, sy) = match q {
                1 => (y, h - 1 - x),
                2 => (w - 1 - x, h - 1 - y),
                _ => (w - 1 - y, x),
            };
            let s = (sy * w + sx) * 3;
            row[x * 3..x * 3 + 3].copy_from_slice(&img.px[s..s + 3]);
        }
    });
    Img { w: nw, h: nh, px }
}

fn crop(img: &Img, x0: f32, y0: f32, x1: f32, y1: f32) -> Img {
    let cl = |v: f32| v.clamp(0.0, 1.0);
    let (ax, bx) = (cl(x0.min(x1)), cl(x0.max(x1)));
    let (ay, by) = (cl(y0.min(y1)), cl(y0.max(y1)));
    let l = (ax * img.w as f32).round() as usize;
    let t = (ay * img.h as f32).round() as usize;
    let r = ((bx * img.w as f32).round() as usize).clamp(l + 1, img.w);
    let b = ((by * img.h as f32).round() as usize).clamp(t + 1, img.h);
    let nw = r - l;
    let mut px = Vec::with_capacity(nw * (b - t) * 3);
    for y in t..b {
        let s = (y * img.w + l) * 3;
        px.extend_from_slice(&img.px[s..s + nw * 3]);
    }
    Img { w: nw, h: b - t, px }
}

/// Size factor of the largest same-shape rectangle inside a picture rotated by `deg`.
pub fn straighten_scale(w: f32, h: f32, deg: f32) -> f32 {
    let t = deg.abs().to_radians();
    let (c, s) = (t.cos(), t.sin());
    (w / (w * c + h * s)).min(h / (w * s + h * c))
}

fn straighten(img: &Img, deg: f32) -> Img {
    if deg.abs() < 1e-3 {
        return img.clone();
    }
    let (w, h) = (img.w as f32, img.h as f32);
    let sc = straighten_scale(w, h, deg);
    let nw = ((w * sc).floor() as usize).max(1);
    let nh = ((h * sc).floor() as usize).max(1);
    let t = deg.to_radians();
    let (c, s) = (t.cos(), t.sin());
    let (cx, cy) = (w / 2.0, h / 2.0);
    let (ncx, ncy) = (nw as f32 / 2.0, nh as f32 / 2.0);
    let mut px = vec![0.0f32; nw * nh * 3];
    px.par_chunks_mut(nw * 3).enumerate().for_each(|(y, row)| {
        let dy = y as f32 + 0.5 - ncy;
        for x in 0..nw {
            let dx = x as f32 + 0.5 - ncx;
            // inverse rotation: output pixel -> source position (clockwise rotation of content)
            let sx = c * dx + s * dy + cx - 0.5;
            let sy = -s * dx + c * dy + cy - 0.5;
            let v = bilinear(img, sx, sy);
            row[x * 3..x * 3 + 3].copy_from_slice(&v);
        }
    });
    Img { w: nw, h: nh, px }
}

fn bilinear(img: &Img, x: f32, y: f32) -> [f32; 3] {
    let x = x.clamp(0.0, (img.w - 1) as f32);
    let y = y.clamp(0.0, (img.h - 1) as f32);
    let (x0, y0) = (x.floor() as usize, y.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(img.w - 1), (y0 + 1).min(img.h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let at = |xx: usize, yy: usize, c: usize| img.px[(yy * img.w + xx) * 3 + c];
    let mut out = [0.0; 3];
    for (c, o) in out.iter_mut().enumerate() {
        let a = at(x0, y0, c) * (1.0 - fx) + at(x1, y0, c) * fx;
        let b = at(x0, y1, c) * (1.0 - fx) + at(x1, y1, c) * fx;
        *o = a * (1.0 - fy) + b * fy;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flip_mirrors_and_both_ways_is_a_half_turn() {
        let img = ramp(5, 3);
        let lr = apply(&img, &[Op::Flip(1)]);
        assert_eq!(&lr.px[0..3], &img.px[4 * 3..5 * 3], "left-right: first pixel is the old last of the row");
        let tb = apply(&img, &[Op::Flip(2)]);
        assert_eq!(&tb.px[0..3], &img.px[2 * 5 * 3..2 * 5 * 3 + 3]);
        assert_eq!(apply(&img, &[Op::Flip(3)]).px, apply(&img, &[Op::Rotate(2)]).px);
        assert_eq!(apply(&img, &[Op::Flip(1), Op::Flip(1)]).px, img.px);
    }

    fn ramp(w: usize, h: usize) -> Img {
        let mut px = Vec::with_capacity(w * h * 3);
        for y in 0..h {
            for x in 0..w {
                let v = 0.2 + 0.5 * (x as f32 / (w - 1) as f32);
                px.extend_from_slice(&[v, v * 0.9, v * 0.8 + 0.0 * y as f32]);
            }
        }
        Img { w, h, px }
    }

    #[test]
    fn quarter_turns_compose_to_identity() {
        let a = ramp(7, 4);
        let b = (0..4).fold(a.clone(), |i, _| rotate_quarter(&i, 1));
        assert_eq!((b.w, b.h), (7, 4));
        assert_eq!(a.px, b.px);
        let r = rotate_quarter(&a, 1);
        assert_eq!((r.w, r.h), (4, 7));
        // top-left of the result is bottom-left of the source when turning clockwise
        assert_eq!(&r.px[0..3], &a.px[(3 * 7) * 3..(3 * 7) * 3 + 3]);
    }

    #[test]
    fn crop_and_output_size_agree() {
        let a = ramp(100, 50);
        let ops = [Op::Crop { x0: 0.1, y0: 0.2, x1: 0.6, y1: 1.0 }, Op::Rotate(1)];
        let r = apply(&a, &ops);
        let (w, h) = output_size(100.0, 50.0, &ops);
        assert_eq!((r.w, r.h), (w.round() as usize, h.round() as usize));
    }

    #[test]
    fn straighten_keeps_shape_and_stays_inside() {
        let a = ramp(400, 300);
        let r = straighten(&a, 5.0);
        let s = straighten_scale(400.0, 300.0, 5.0);
        assert!(s < 1.0 && s > 0.8);
        assert!(((r.w as f32 / r.h as f32) - 4.0 / 3.0).abs() < 0.02);
        // no black corners: every output pixel comes from inside the source
        assert!(r.px.iter().all(|&v| v >= 0.15));
    }

    #[test]
    fn auto_contrast_stretches_to_full_range_without_changing_hue() {
        let a = ramp(256, 8);
        let r = apply(&a, &[contrast_op(&a)]);
        let (mn, mx) = r.px.iter().fold((1f32, 0f32), |(a, b), &v| (a.min(v), b.max(v)));
        assert!(mn < 0.02 && mx > 0.98, "{mn} {mx}");
        // a grey picture stays grey (the earlier brightness-only version turned blues cyan)
        let grey = Img { w: 64, h: 1, px: (0..64).flat_map(|i| [0.3 + i as f32 / 200.0; 3]).collect() };
        let g = apply(&grey, &[contrast_op(&grey)]);
        assert!(g.px.chunks(3).all(|p| (p[0] - p[1]).abs() < 1e-6 && (p[1] - p[2]).abs() < 1e-6));
    }

    #[test]
    fn auto_contrast_points_come_from_smallest_and_largest_colour() {
        // black point from the smallest colour (red, 28/255 at the dark end), white point
        // from the largest (blue, 240/255 at the bright end), as Picasa's on lab photo base_13
        let px: Vec<f32> = (0..1000)
            .flat_map(|i| {
                let t = i as f32 / 999.0;
                [(28.0 + 150.0 * t) / 255.0, (50.0 + 150.0 * t) / 255.0, (60.0 + 180.0 * t) / 255.0]
            })
            .collect();
        let Op::AutoContrast { lo, hi } = contrast_op(&Img { w: 1000, h: 1, px }) else { unreachable!() };
        assert!((lo * 255.0 - 28.0).abs() < 1.5 && (hi * 255.0 - 240.0).abs() < 1.5, "{} {}", lo * 255.0, hi * 255.0);
    }

    #[test]
    fn auto_color_removes_cast_without_changing_brightness() {
        // grey ramp with a blue cast
        let mut a = ramp(256, 8);
        for p in a.px.chunks_mut(3) {
            let g = p[0];
            p[0] = g * 0.85;
            p[1] = g * 0.9;
            p[2] = (g * 1.1).min(1.0);
        }
        let r = apply(&a, &[color_op(&a)]);
        let mean = |img: &Img, c: usize| img.px.chunks(3).map(|p| p[c]).sum::<f32>() / (img.w * img.h) as f32;
        let spread = |img: &Img| {
            let m = [mean(img, 0), mean(img, 1), mean(img, 2)];
            m.iter().cloned().fold(0f32, f32::max) - m.iter().cloned().fold(1f32, f32::min)
        };
        // most of the cast removed
        assert!(spread(&r) < spread(&a) * 0.6, "cast {} -> {}", spread(&a), spread(&r));
        let ly = |img: &Img| img.px.chunks(3).map(luma).sum::<f32>() / (img.w * img.h) as f32;
        assert!((ly(&r) - ly(&a)).abs() < 0.02);
    }

    #[test]
    fn fill_light_matches_picasa_pixels() {
        // pixels from lab photo base_2, before and after Picasa 3.9's Fill Light (0-255)
        let cases = [
            ([8.0, 1.0, 34.0], 1.0, [98.0, 42.0, 175.0]),
            ([144.0, 73.0, 91.0], 1.0, [255.0, 183.0, 203.0]),
            ([48.0, 1.0, 107.0], 0.3, [69.0, 0.0, 142.0]),
            ([161.0, 35.0, 46.0], 0.3, [197.0, 52.0, 69.0]),
        ];
        for (input, amount, want) in cases {
            let px: Vec<f32> = input.iter().map(|v| v / 255.0).collect();
            let r = apply(&Img { w: 1, h: 1, px }, &[Op::Fill(amount)]);
            for c in 0..3 {
                let got = r.px[c].clamp(0.0, 1.0) * 255.0;
                assert!((got - want[c]).abs() <= 6.0, "{input:?} at {amount}: colour {c} {got:.0}, Picasa {}", want[c]);
            }
        }
        // no change at 0; white stays white
        assert_eq!(fill_value(0.4, 0.4, 0.0), 0.4);
        assert!((fill_value(1.0, 1.0, 1.0) - 1.0).abs() < 1e-6);
    }

    /// Fill Light slider speed on an Edit-view-sized picture (2048 x 1536).
    /// Run: cargo test --release -p casapic-core fill_light_speed -- --ignored --nocapture
    #[test]
    #[ignore]
    fn fill_light_speed() {
        let img = Img { w: 2048, h: 1536, px: (0..2048 * 1536 * 3).map(|i| (i % 251) as f32 / 250.0).collect() };
        let mut times: Vec<f64> = (0..10)
            .map(|_| {
                let t = std::time::Instant::now();
                let _ = apply_one(img.clone(), &Op::Fill(0.5));
                t.elapsed().as_secs_f64() * 1000.0
            })
            .collect();
        times.sort_by(|a, b| a.total_cmp(b));
        println!("fill light 2048x1536: median {:.1} ms, slowest {:.1} ms", times[5], times[9]);
    }
}
