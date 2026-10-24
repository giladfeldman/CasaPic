//! The Tuning tab and the effects of Picasa's third editing tab, written from scratch to look
//! like Picasa 3.9's (clean room: rules and numbers measured from Picasa's own exports of lab
//! photos, see docs/results-build2.md; no Picasa code).
//!
//! Effects that look at neighbouring pixels come in two kinds. Sharpen, Glow and Film Grain work
//! at the scale of the original photo's pixels (Picasa's radius 3 is 3 pixels of the full-size
//! photo), so they take `scale` = pixels of the picture being rendered per pixel of the
//! original; on a screen-sized preview they shrink as they would when the full-size result is
//! shrunk. Soft Focus, Focal B&W and Graduated Tint are laid out in fractions of the picture
//! and look the same at any size.

use rayon::prelude::*;

use crate::recipe::Rgb;
use crate::render::{Img, luma};

/// Piecewise-linear curve with 17 evenly spaced points over 0–1.
fn curve(t: &[f32; 17], v: f32) -> f32 {
    let x = v.clamp(0.0, 1.0) * 16.0;
    let i = (x as usize).min(15);
    let f = x - i as f32;
    t[i] + f * (t[i + 1] - t[i])
}

fn for_pixels(img: &mut Img, f: impl Fn(&mut [f32]) + Sync) {
    let w = img.w;
    img.px.par_chunks_mut(w * 3).for_each(|row| row.chunks_mut(3).for_each(&f));
}

// ---------- blur ----------

/// Gaussian blur of every colour, `sigma` in pixels. Small radii use the exact bell-shaped
/// weights; larger ones three box blurs in a row, the usual fast stand-in for a Gaussian.
pub fn blur(img: &Img, sigma: f32) -> Img {
    if sigma < 0.2 {
        return img.clone();
    }
    let h = blur_rows(&img.px, img.w, sigma);
    let t = transpose(&h, img.w, img.h);
    let v = blur_rows(&t, img.h, sigma);
    Img { w: img.w, h: img.h, px: transpose(&v, img.h, img.w) }
}

fn transpose(px: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; px.len()];
    out.par_chunks_mut(h * 3).enumerate().for_each(|(x, col)| {
        for y in 0..h {
            col[y * 3..y * 3 + 3].copy_from_slice(&px[(y * w + x) * 3..(y * w + x) * 3 + 3]);
        }
    });
    out
}

fn blur_rows(px: &[f32], w: usize, sigma: f32) -> Vec<f32> {
    let mut out = vec![0.0f32; px.len()];
    if sigma < 3.0 {
        let r = (sigma * 3.0).ceil() as isize;
        let k: Vec<f32> = (-r..=r).map(|i| (-(i * i) as f32 / (2.0 * sigma * sigma)).exp()).collect();
        let sum: f32 = k.iter().sum();
        let k: Vec<f32> = k.iter().map(|v| v / sum).collect();
        out.par_chunks_mut(w * 3).zip(px.par_chunks(w * 3)).for_each(|(o, row)| {
            for x in 0..w as isize {
                let mut acc = [0.0f32; 3];
                for (j, kv) in k.iter().enumerate() {
                    let sx = (x + j as isize - r).clamp(0, w as isize - 1) as usize;
                    for c in 0..3 {
                        acc[c] += kv * row[sx * 3 + c];
                    }
                }
                o[x as usize * 3..x as usize * 3 + 3].copy_from_slice(&acc);
            }
        });
        return out;
    }
    // three box blurs of half-width r give a Gaussian of about this sigma
    let r = (((12.0 * sigma * sigma / 3.0 + 1.0).sqrt() - 1.0) / 2.0).round().max(1.0) as usize;
    out.par_chunks_mut(w * 3).zip(px.par_chunks(w * 3)).for_each(|(o, row)| {
        let mut a = row.to_vec();
        let mut b = vec![0.0f32; row.len()];
        for _ in 0..3 {
            box_row(&a, &mut b, w, r);
            std::mem::swap(&mut a, &mut b);
        }
        o.copy_from_slice(&a);
    });
    out
}

fn box_row(src: &[f32], dst: &mut [f32], w: usize, r: usize) {
    let n = (2 * r + 1) as f32;
    for c in 0..3 {
        let at = |x: isize| src[(x.clamp(0, w as isize - 1) as usize) * 3 + c];
        let mut acc: f32 = (-(r as isize)..=r as isize).map(at).sum();
        for x in 0..w as isize {
            dst[x as usize * 3 + c] = acc / n;
            acc += at(x + r as isize + 1) - at(x - r as isize);
        }
    }
}

// ---------- Tuning ----------

/// Colour temperature gains per colour, measured from Picasa at seven slider values
/// (−1 cool … +1 warm); straight lines between them.
const TEMP_TABLE: [(f32, [f32; 3]); 7] = [
    (-1.0, [0.602, 1.134, 1.881]),
    (-0.5, [0.874, 1.028, 1.184]),
    (-0.25, [0.940, 1.009, 1.059]),
    (0.0, [1.0, 1.0, 1.0]),
    (0.25, [1.0095, 1.0, 0.939]),
    (0.5, [1.035, 0.998, 0.900]),
    (1.0, [1.066, 0.994, 0.848]),
];

pub fn temperature_gains(t: f32) -> [f32; 3] {
    let t = t.clamp(-1.0, 1.0);
    for w in TEMP_TABLE.windows(2) {
        let ((t0, g0), (t1, g1)) = (w[0], w[1]);
        if t <= t1 {
            let f = (t - t0) / (t1 - t0);
            return [0, 1, 2].map(|c| g0[c] + f * (g1[c] - g0[c]));
        }
    }
    TEMP_TABLE[6].1
}

/// Neutral Color Picker: the picked colour is made grey at its own brightness
/// (gains = brightness / colour; matched Picasa at ΔE 0.8–1.7).
pub fn neutral_gains(n: Rgb) -> [f32; 3] {
    if n.is_none() {
        return [1.0; 3];
    }
    let c = n.channels().map(|v| v.max(1.0 / 255.0));
    let y = luma(&c);
    c.map(|v| y / v)
}

pub fn tune(img: &mut Img, fill: f32, highlights: f32, shadows: f32, neutral: Rgb, temp: f32) {
    let ng = neutral_gains(neutral);
    let tg = temperature_gains(temp);
    let g = [0, 1, 2].map(|c| ng[c] * tg[c]);
    if g != [1.0; 3] {
        for_pixels(img, |p| {
            for c in 0..3 {
                p[c] *= g[c];
            }
        });
    }
    if fill > 0.0 {
        crate::render::fill_light(img, fill);
    }
    // Highlights and Shadows are a white point at 1 − highlights and a black point at shadows
    let (lo, hi) = (shadows.clamp(0.0, 0.95), 1.0 - highlights.clamp(0.0, 0.95));
    if lo > 0.0 || hi < 1.0 {
        let k = 1.0 / (hi - lo).max(0.02);
        for_pixels(img, |p| {
            for v in p.iter_mut() {
                *v = (*v - lo) * k;
            }
        });
    }
}

// ---------- one-click looks ----------

/// Sepia: three curves of the grey value (17 points each, fitted on 6 lab photos; ΔE 0.73 on
/// 4 photos left out of the fit).
const SEPIA: [[f32; 17]; 3] = [
    [0.1731, 0.2415, 0.3084, 0.3703, 0.4356, 0.5009, 0.5623, 0.6302, 0.6686, 0.7113, 0.7506, 0.7945, 0.8351, 0.8773, 0.9214, 0.9609, 1.0],
    [0.1409, 0.1927, 0.2470, 0.3004, 0.3495, 0.4028, 0.4549, 0.5172, 0.5707, 0.6227, 0.6770, 0.7293, 0.7900, 0.8398, 0.8939, 0.9486, 1.0],
    [0.1090, 0.1542, 0.1954, 0.2369, 0.2775, 0.3184, 0.3582, 0.4186, 0.4821, 0.5448, 0.6137, 0.6759, 0.7437, 0.8057, 0.8708, 0.9373, 1.0],
];
/// Warmify: one curve per colour (fitted as Sepia; ΔE 0.83 held out).
const WARM: [[f32; 17]; 3] = [
    [0.0005, 0.0793, 0.1590, 0.2349, 0.3074, 0.3757, 0.4369, 0.4934, 0.5436, 0.5924, 0.6386, 0.6845, 0.7328, 0.7826, 0.8358, 0.8929, 0.9487],
    [0.0005, 0.0530, 0.1118, 0.1711, 0.2339, 0.2921, 0.3460, 0.3941, 0.4410, 0.4888, 0.5399, 0.5952, 0.6550, 0.7129, 0.7803, 0.8438, 0.9103],
    [0.0015, 0.0400, 0.0837, 0.1271, 0.1718, 0.2186, 0.2688, 0.3215, 0.3778, 0.4367, 0.4991, 0.5650, 0.6322, 0.7023, 0.7725, 0.8458, 0.9168],
];

pub fn sepia(img: &mut Img) {
    for_pixels(img, |p| {
        let g = luma(p);
        for c in 0..3 {
            p[c] = curve(&SEPIA[c], g);
        }
    });
}

pub fn warmify(img: &mut Img) {
    for_pixels(img, |p| {
        for c in 0..3 {
            p[c] = curve(&WARM[c], p[c]);
        }
    });
}

/// B&W: the grey value (Rec. 601 weights; matched Picasa at ΔE 0.17).
pub fn black_white(img: &mut Img) {
    for_pixels(img, |p| {
        let g = luma(p);
        p.fill(g);
    });
}

/// Filtered B&W: grey made through a coloured filter (each colour weighted by the filter's
/// share of it), then scaled so the picture keeps its average brightness (ΔE 1.2).
pub fn filtered_bw(img: &mut Img, filter: Rgb) {
    let f = filter.channels();
    let sum = (f[0] + f[1] + f[2]).max(1e-3);
    let w = f.map(|v| v / sum);
    let n = (img.w * img.h).max(1) as f64;
    let (sy, sg) = img.px.par_chunks(3).map(|p| (luma(p) as f64, (w[0] * p[0] + w[1] * p[1] + w[2] * p[2]) as f64)).reduce(|| (0.0, 0.0), |a, b| (a.0 + b.0, a.1 + b.1));
    let k = if sg > 1e-6 { (sy / n) / (sg / n) } else { 1.0 } as f32;
    for_pixels(img, |p| {
        let g = (w[0] * p[0] + w[1] * p[1] + w[2] * p[2]) * k;
        p.fill(g);
    });
}

/// Saturation. Less colour: a straight mix toward grey (exact match). More colour: the two
/// colour-difference signals of JPEG's YCbCr are boosted, blue–yellow about twice as much as
/// red–green (fitted; ΔE 1.6 at Picasa's default, larger at strong settings).
pub fn saturation(img: &mut Img, a: f32) {
    let a = a.clamp(-1.0, 1.0);
    if a < 0.0 {
        let k = 1.0 + a;
        for_pixels(img, |p| {
            let y = 0.25 * p[0] + 0.625 * p[1] + 0.125 * p[2];
            for v in p.iter_mut() {
                *v = y + k * (*v - y);
            }
        });
        return;
    }
    let (kb, kr) = (1.0 + 3.4925 * a, 1.0 + 1.7983 * a);
    for_pixels(img, |p| {
        let y = luma(p);
        let cb = (-0.168736 * p[0] - 0.331264 * p[1] + 0.5 * p[2]) * kb;
        let cr = (0.5 * p[0] - 0.418688 * p[1] - 0.081312 * p[2]) * kr;
        p[0] = y + 1.402 * cr;
        p[1] = y - 0.344136 * cb - 0.714136 * cr;
        p[2] = y + 1.772 * cb;
    });
}

/// Tint: each pixel keeps its grey brightness and takes on the tint colour, fading toward
/// black and white the way an HSL "colourize" does (one rule fitted across four colours,
/// ΔE 2.2–3.5). The stored "fade" number is kept in the recipe but not used: in Picasa's own
/// renders it made no visible difference (0, 0.25, 0.75 and 1 gave the same picture), and
/// blending by it put CasaPic 4–16 ΔE away from Picasa (lab round 4).
pub fn tint(img: &mut Img, _fade: f32, color: Rgb) {
    let t = color.channels();
    let tl = luma(&t);
    let d = t.map(|v| v - tl);
    let room = (1.0 - (2.0 * tl - 1.0).abs()).max(1e-3);
    for_pixels(img, |p| {
        let g = luma(p);
        let w = (2.12 * (1.0 - (2.0 * g - 1.0).abs()) / room).clamp(0.0, 1.0);
        for c in 0..3 {
            p[c] = g + w * d[c];
        }
    });
}

// ---------- effects at the scale of the original's pixels ----------

/// Sharpen: an unsharp mask of radius about 1 pixel of the original photo (explains ~90% of
/// Picasa's change on a detailed lab photo).
pub fn sharpen(img: &Img, amount: f32, scale: f32) -> Img {
    let b = blur(img, 1.0 * scale);
    let k = 1.65 * amount;
    let mut out = img.clone();
    out.px.par_iter_mut().zip(b.px.par_iter()).for_each(|(v, bv)| *v += k * (*v - bv));
    out
}

/// Glow: the picture screened over itself through a blur of 1.2 × radius pixels of the
/// original, squared (ΔE 0.5–0.8 against Picasa's full-size exports).
pub fn glow(img: &Img, intensity: f32, radius: f32, scale: f32) -> Img {
    let b = blur(img, 1.2 * radius.max(0.0) * scale);
    let mut out = img.clone();
    out.px.par_iter_mut().zip(b.px.par_iter()).for_each(|(v, bv)| {
        let s = intensity * bv.max(0.0) * bv.max(0.0);
        *v = 1.0 - (1.0 - *v) * (1.0 - s);
    });
    out
}

/// A repeatable random number in 0–1 for pixel (x, y).
fn hash01(x: u32, y: u32, seed: u32) -> f32 {
    let mut h = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Film Grain: black-and-white speckle, independent per pixel of the original photo. Each
/// pixel is multiplied by (1 − u), u spread evenly over 0 … amount/100 (lighten: the same on
/// the inverted picture). Measured on Picasa's full-size exports at amounts 5 and 10. On a
/// smaller picture each pixel stands for several of the original's, so the speckle's spread
/// shrinks by `scale` while its average darkening stays.
pub fn grain(img: &mut Img, amount: f32, lighten: bool, scale: f32) {
    let top = (amount / 100.0).clamp(0.0, 1.0);
    let spread = scale.clamp(0.0, 1.0);
    let w = img.w;
    let seed = (img.w as u32).wrapping_mul(31) ^ img.h as u32;
    img.px.par_chunks_mut(w * 3).enumerate().for_each(|(y, row)| {
        for (x, p) in row.chunks_mut(3).enumerate() {
            let u = top * (0.5 + (hash01(x as u32, y as u32, seed) - 0.5) * spread);
            for v in p.iter_mut() {
                *v = if lighten { 1.0 - (1.0 - *v) * (1.0 - u) } else { *v * (1.0 - u) };
            }
        }
    });
}

// ---------- effects laid out on the picture ----------

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The spot of Soft Focus and Focal B&W: 0 inside (untouched), rising to 1 outside. Distances
/// are in units of the picture's shorter side. At the default sliders the change starts at the
/// centre and is complete half a short side away (Focal B&W ΔE 0.44 against Picasa).
fn spot(img: &Img, cx: f32, cy: f32, r0: f32, r1: f32, floor: f32) -> Vec<f32> {
    let (w, h) = (img.w, img.h);
    let short = w.min(h) as f32;
    let (px, py) = (cx * w as f32, cy * h as f32);
    let mut m = vec![0.0f32; w * h];
    m.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, v) in row.iter_mut().enumerate() {
            let r = ((x as f32 + 0.5 - px).hypot(y as f32 + 0.5 - py)) / short;
            let s = if r1 - r0 < 1e-3 { (r >= r0) as i32 as f32 } else { smoothstep((r - r0) / (r1 - r0)) };
            *v = floor + (1.0 - floor) * s;
        }
    });
    m
}

/// Where the spot's change starts and ends, and how much of the picture's colour survives
/// inside it, for the size and sharpness sliders (−1 to 1, 0 = Picasa's default). Measured one
/// slider at a time: size sets the outer radius, 0.5 × (1 + size) short sides (ΔE 0.3–0.8 at
/// −0.5, 0.5 and 1); positive sharpness narrows the band around its middle (1 = a hard edge
/// at half the radius); negative sharpness greys the inside as well (−0.5 made the whole
/// picture grey).
pub fn spot_radii(size: f32, sharpness: f32) -> (f32, f32, f32) {
    let outer = 0.5 * (1.0 + size.clamp(-0.95, 1.0));
    let mid = outer / 2.0;
    let half = mid * (1.0 - sharpness.clamp(0.0, 1.0));
    let floor = (-2.0 * sharpness).clamp(0.0, 1.0);
    (mid - half, mid + half, floor)
}

/// Soft Focus blur strength, in units of the picture's shorter side, for the amount slider
/// (fitted at −0.5, 0, 0.5 and 1).
pub fn soft_sigma(amount: f32) -> f32 {
    0.0133 * (0.81 * amount.clamp(-1.0, 1.0)).exp()
}

pub fn soft_focus(img: &Img, x: f32, y: f32, size: f32, amount: f32) -> Img {
    let (r0, r1, _) = spot_radii(size, 0.0);
    let m = spot(img, x, y, r0, r1, 0.0);
    let b = blur(img, soft_sigma(amount) * img.w.min(img.h) as f32);
    let mut out = img.clone();
    out.px.par_chunks_mut(3).zip(b.px.par_chunks(3)).zip(m.par_iter()).for_each(|((p, q), &k)| {
        for c in 0..3 {
            p[c] += k * (q[c] - p[c]);
        }
    });
    out
}

pub fn focal_bw(img: &mut Img, x: f32, y: f32, size: f32, sharpness: f32) {
    let (r0, r1, floor) = spot_radii(size, sharpness);
    let m = spot(img, x, y, r0, r1, floor);
    img.px.par_chunks_mut(3).zip(m.par_iter()).for_each(|(p, &k)| {
        let g = luma(p);
        for v in p.iter_mut() {
            *v += k * (g - *v);
        }
    });
}

/// Graduated Tint's extra darkening for the shade slider, per colour (measured at shade 0,
/// 0.25 and 0.75; straight lines between, continued to 1).
/// The shade's darkening of a channel value `c`: Picasa's output in the fully tinted band fits
/// g·c·(1 − k + k·c) in every channel alike (lab: shade 0 is an exact multiply by the colour;
/// shade .25 gives k ≈ .29, g ≈ 1.0; shade .75 gives k ≈ .95, g ≈ .77).
fn shade_curve(c: f32, shade: f32) -> f32 {
    let s = shade.clamp(0.0, 1.0);
    let (k, g) = ((1.2 * s).min(1.0), 1.0 - 0.41 * s * s);
    g * c * (1.0 - k + k * c.clamp(0.0, 1.0))
}

/// Graduated Tint: the part of the picture above an edge is multiplied by the tint colour and
/// darkened by the shade (`shade_curve`). The edge passes through the handle (x, y); its soft band is centred
/// on it with half-width 0.54 × feather × picture height. Measured from single settings (so
/// less certain): the edge tilts about 30° per picture-width the handle sits off-centre, and
/// the last number turns the tint by quarter turns (30 put it at the bottom).
pub fn grad_tint(img: &mut Img, x: f32, y: f32, feather: f32, shade: f32, color: Rgb, turn: f32) {
    let col = color.channels();
    let (w, h) = (img.w as f32, img.h as f32);
    let angle = (0.5 - x) * 30.3f32.to_radians();
    let (sa, ca) = angle.sin_cos();
    let flip = (turn.round() as i64).rem_euclid(4) == 2;
    let half = (0.54 * feather.clamp(0.0, 1.0) * h).max(0.5);
    let (hx, hy) = (x * w, y * h);
    let iw = img.w;
    img.px.par_chunks_mut(iw * 3).enumerate().for_each(|(py, row)| {
        for (px, p) in row.chunks_mut(3).enumerate() {
            // signed distance below the edge, in pixels
            let (dx, dy) = (px as f32 + 0.5 - hx, py as f32 + 0.5 - hy);
            let mut d = dy * ca - dx * sa;
            if flip {
                d = -d;
            }
            let m = 1.0 - smoothstep((d + half) / (2.0 * half));
            for c in 0..3 {
                p[c] += m * (col[c] * shade_curve(p[c], shade) - p[c]);
            }
        }
    });
}
