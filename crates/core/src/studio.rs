//! The Studio tab: tools in the style of the Snapseed mobile app (Tune Image, Details, Vignette,
//! HDR Scape). CasaPic's own versions, written from scratch; Snapseed's code is not public and
//! none of it is used. Slider values use Snapseed's ranges (−100 to +100, or 0 to 100), so a
//! setting remembered from the phone means roughly the same thing here.
//!
//! The Vignette was measured: 111 of the owner's Snapseed edits were matched to their camera
//! originals and aligned. The darkening is a circle centred on the picture, sized by the half
//! diagonal (not stretched to the frame), multiplying the light (linear values) by
//! exp(inner + (outer − inner)·r^p). Fitted per photo, it brought the median colour difference
//! to the Snapseed result from 5.5 to 2.1 ΔE.
//!
//! Tools that look at neighbouring pixels are laid out in fractions of the picture, so a
//! preview and the full-size result look alike; Sharpening alone works in pixels of the original
//! photo (`scale` = rendered pixels per original pixel), like Picasa's Sharpen.

use rayon::prelude::*;

use crate::effects::{blur, temperature_gains};
use crate::render::{luma, Img};

fn to_linear(v: f32) -> f32 {
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn to_srgb(v: f32) -> f32 {
    let v = v.max(0.0);
    if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Grey brightness of every pixel, as its own one-colour picture (for blurring).
fn luma_plane(img: &Img) -> Vec<f32> {
    img.px.par_chunks(3).map(luma).collect()
}

/// Blurs a one-colour plane by `sigma` pixels (uses the colour blur on a grey picture).
fn blur_plane(plane: &[f32], w: usize, h: usize, sigma: f32) -> Vec<f32> {
    let grey = Img { w, h, px: plane.iter().flat_map(|&v| [v, v, v]).collect() };
    blur(&grey, sigma).px.par_chunks(3).map(|p| p[0]).collect()
}

fn diag(img: &Img) -> f32 {
    ((img.w * img.w + img.h * img.h) as f32).sqrt()
}

/// Moves a pixel to grey brightness `y1` from `y0`, keeping its colour (ratios kept, so the
/// hue holds; very dark pixels are lifted with their colour, not toward grey).
fn set_luma(p: &mut [f32], y0: f32, y1: f32) {
    if y0 > 1e-4 {
        let k = y1 / y0;
        for v in p.iter_mut() {
            *v *= k;
        }
    } else {
        for v in p.iter_mut() {
            *v += y1 - y0;
        }
    }
}

fn saturate(p: &mut [f32], k: f32) {
    let y = luma(p);
    for v in p.iter_mut() {
        *v = y + k * (*v - y);
    }
}

// ---------- Tune Image ----------

/// Tune Image: brightness, contrast, saturation, ambiance, highlights, shadows and warmth,
/// each −100 to +100 (0 = unchanged).
#[allow(clippy::too_many_arguments)] // one argument per slider of the Tune Image tool
pub fn tune_image(img: &mut Img, brightness: f32, contrast: f32, saturation: f32, ambiance: f32, highlights: f32, shadows: f32, warmth: f32) {
    let (b, c, s, a, hi, sh, wa) = [brightness, contrast, saturation, ambiance, highlights, shadows, warmth].map(|v| v.clamp(-100.0, 100.0) / 100.0).into();
    // brightness: a midtone curve (black and white stay put)
    let gamma = (-0.75 * b).exp();
    // highlights and shadows (and ambiance) look at the brightness of the surroundings, so
    // a bright sky is darkened as a whole and texture inside it is kept
    let need_base = hi != 0.0 || sh != 0.0 || a != 0.0;
    let base = if need_base { blur_plane(&luma_plane(img), img.w, img.h, 0.01 * diag(img)) } else { Vec::new() };
    let warm = temperature_gains(0.6 * wa);
    let w = img.w;
    img.px.par_chunks_mut(w * 3).enumerate().for_each(|(row, line)| {
        for (i, p) in line.chunks_mut(3).enumerate() {
            let y0 = luma(p).max(0.0);
            let mut y = y0.min(1.0).powf(gamma) + (y0 - y0.min(1.0));
            if c > 0.0 {
                // blend toward an S-curve (black and white stay put)
                let s_curve = 0.5 + 0.5 * (2.2 * (2.0 * y.min(1.0) - 1.0)).tanh() / 2.2f32.tanh();
                y += c * (s_curve - y.min(1.0));
            } else if c < 0.0 {
                y = 0.5 + (y - 0.5) * (1.0 + 0.6 * c);
            }
            if need_base {
                let bv = base[row * w + i];
                // highlights: only where the surroundings are bright
                y += hi * 0.35 * smoothstep(0.45, 1.0, bv) * if hi < 0.0 { y } else { 1.0 - y };
                // shadows: only where the surroundings are dark
                y += sh * 0.40 * (1.0 - smoothstep(0.0, 0.55, bv)) * if sh > 0.0 { 1.0 - y } else { y };
                // ambiance: evens out the light (dark areas up, bright down) and adds detail and
                // colour in the middle tones
                y += a * 0.30 * (0.5 - bv) * (1.0 - (2.0 * y - 1.0).abs()) + a * 0.25 * (y - bv);
            }
            set_luma(p, y0, y.max(0.0));
            let sat = 1.0 + s * if s > 0.0 { 0.9 } else { 1.0 } + 0.25 * a;
            if sat != 1.0 {
                saturate(p, sat.max(0.0));
            }
            if wa != 0.0 {
                for c in 0..3 {
                    p[c] *= warm[c];
                }
            }
        }
    });
}

// ---------- Details ----------

/// Details: Structure (−100 to +100) brings out (or smooths) texture; Sharpening (0 to 100).
pub fn details(img: &Img, structure: f32, sharpening: f32, scale: f32) -> Img {
    let mut out = img.clone();
    let st = structure.clamp(-100.0, 100.0) / 100.0;
    if st != 0.0 {
        let y = luma_plane(img);
        let b = blur_plane(&y, img.w, img.h, 0.0025 * diag(img));
        out.px.par_chunks_mut(3).zip(y.par_iter().zip(b.par_iter())).for_each(|(p, (&y0, &bv))| {
            let d = y0 - bv;
            // texture is boosted most in the middle tones, little in deep shadow and highlight
            let wgt = 1.0 - (2.0 * y0.clamp(0.0, 1.0) - 1.0).powi(4);
            let y1 = y0 + st * 1.4 * d * wgt;
            set_luma(p, y0, y1.max(0.0));
        });
    }
    let sp = sharpening.clamp(0.0, 100.0) / 100.0;
    if sp > 0.0 {
        let b = blur(&out, 1.0 * scale);
        let k = 1.5 * sp;
        out.px.par_iter_mut().zip(b.px.par_iter()).for_each(|(v, bv)| *v += k * (*v - bv));
    }
    out
}

// ---------- Vignette ----------

/// The vignette's light factor at distance `r` from the centre (in half diagonals):
/// outer and inner brightness −100 to +100, centre size 0 to 100.
pub fn vignette_factor(r: f32, outer: f32, inner: f32, size: f32) -> f32 {
    let o = outer.clamp(-100.0, 100.0) / 100.0 * 1.5;
    let i = inner.clamp(-100.0, 100.0) / 100.0 * 0.5;
    let p = 1.0 + size.clamp(0.0, 100.0) / 25.0;
    (i + (o - i) * r.max(0.0).powf(p)).exp()
}

/// Vignette: darkens (or lightens) toward the edges around a centre point (fractions of the
/// picture, 0.5 = middle). Works on the light itself, as the measured Snapseed results do.
pub fn vignette(img: &mut Img, outer: f32, inner: f32, size: f32, cx: f32, cy: f32) {
    let (w, h) = (img.w as f32, img.h as f32);
    let half = 0.5 * (w * w + h * h).sqrt();
    let (x0, y0) = (cx.clamp(0.0, 1.0) * w, cy.clamp(0.0, 1.0) * h);
    let wi = img.w;
    img.px.par_chunks_mut(wi * 3).enumerate().for_each(|(row, line)| {
        let dy = row as f32 + 0.5 - y0;
        for (i, p) in line.chunks_mut(3).enumerate() {
            let dx = i as f32 + 0.5 - x0;
            let f = vignette_factor((dx * dx + dy * dy).sqrt() / half, outer, inner, size);
            for v in p.iter_mut() {
                *v = to_srgb(to_linear(*v) * f);
            }
        }
    });
}

// ---------- HDR Scape ----------

/// HDR Scape: strength 0 to 100, brightness and saturation −100 to +100. Flattens the large
/// light differences (bright sky, dark foreground) and strengthens the detail in between, on a
/// log scale of brightness in two sizes of surroundings.
pub fn hdr_scape(img: &mut Img, strength: f32, brightness: f32, saturation: f32) {
    let k = strength.clamp(0.0, 100.0) / 100.0;
    let br = brightness.clamp(-100.0, 100.0) / 100.0;
    let sa = saturation.clamp(-100.0, 100.0) / 100.0;
    let d = diag(img);
    let ly: Vec<f32> = luma_plane(img).par_iter().map(|&y| (y.max(0.0) + 0.01).ln()).collect();
    let big = blur_plane(&ly, img.w, img.h, 0.04 * d);
    let mid = blur_plane(&ly, img.w, img.h, 0.008 * d);
    let mean = big.iter().sum::<f32>() / big.len().max(1) as f32;
    img.px.par_chunks_mut(3).enumerate().for_each(|(n, p)| {
        let y0 = luma(p).max(0.0);
        let (l, b, m) = (ly[n], big[n], mid[n]);
        // large-scale light pulled toward the picture's average, middle-scale and fine detail
        // strengthened
        let l1 = mean + (b - mean) * (1.0 - 0.55 * k) + (m - b) * (1.0 + 0.9 * k) + (l - m) * (1.0 + 0.6 * k) + 0.35 * br;
        let y1 = (l1.exp() - 0.01).max(0.0);
        set_luma(p, y0, y1);
        saturate(p, (1.0 + 0.35 * k + 0.8 * sa).max(0.0));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(w: usize, h: usize, v: f32) -> Img {
        Img { w, h, px: vec![v; w * h * 3] }
    }

    #[test]
    fn zero_settings_change_nothing() {
        let mut img = Img { w: 4, h: 2, px: (0..24).map(|i| i as f32 / 24.0).collect() };
        let orig = img.clone();
        tune_image(&mut img, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        for (a, b) in img.px.iter().zip(&orig.px) {
            assert!((a - b).abs() < 1e-5);
        }
        let d = details(&orig, 0.0, 0.0, 1.0);
        assert_eq!(d.px, orig.px);
        let mut v = orig.clone();
        vignette(&mut v, 0.0, 0.0, 30.0, 0.5, 0.5);
        for (a, b) in v.px.iter().zip(&orig.px) {
            assert!((a - b).abs() < 1e-4, "{a} {b}");
        }
    }

    #[test]
    fn vignette_darkens_the_corners_only() {
        let mut img = flat(201, 101, 0.6);
        vignette(&mut img, -64.0, 0.0, 21.0, 0.5, 0.5);
        let at = |x: usize, y: usize| img.px[(y * 201 + x) * 3];
        assert!((at(100, 50) - 0.6).abs() < 0.01, "centre {}", at(100, 50));
        assert!(at(0, 0) < 0.45, "corner {}", at(0, 0));
        // a circle, not stretched to the frame: the side edge is darker than the top edge
        assert!(at(0, 50) < at(100, 0));
    }

    #[test]
    fn my_snapseed_look_matches_the_measured_factor() {
        // the fitted look (inner 0.07, corners −0.966 log units, power 1.83) in slider units
        let f = vignette_factor(1.0, -64.4, 14.0, 20.8);
        assert!((f.ln() + 0.966).abs() < 0.01, "{}", f.ln());
        let c = vignette_factor(0.0, -64.4, 14.0, 20.8);
        assert!((c.ln() - 0.07).abs() < 0.001);
    }

    #[test]
    fn tools_move_the_right_way() {
        let grad = Img { w: 64, h: 1, px: (0..64).flat_map(|i| { let v = i as f32 / 63.0; [v, v * 0.8, v * 0.6] }).collect() };
        let mean = |i: &Img| i.px.iter().sum::<f32>() / i.px.len() as f32;
        let mut b = grad.clone();
        tune_image(&mut b, 50.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        assert!(mean(&b) > mean(&grad));
        let mut s = grad.clone();
        tune_image(&mut s, 0.0, 0.0, -100.0, 0.0, 0.0, 0.0, 0.0);
        assert!(s.px.chunks(3).all(|p| (p[0] - p[2]).abs() < 1e-4), "saturation −100 is grey");
        let mut h = grad.clone();
        hdr_scape(&mut h, 80.0, 0.0, 0.0);
        assert!(h.px.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn every_studio_op_in_a_recipe_changes_the_picture() {
        // through the recipe text and the renderer, as Save and the Edit view use them: a tool
        // that parses but is not drawn (or is drawn as another tool) would leave the picture alone
        // big enough that Structure's surroundings (0.25% of the diagonal) span a pixel or more
        let (w, h) = (400usize, 300usize);
        let img = Img { w, h, px: (0..w * h).flat_map(|i| { let (x, y) = (i % w, i / w); let t = if (x / 2 + y / 2) % 2 == 0 { 0.08 } else { -0.08 }; let v = x as f32 / w as f32; [v + t, v * 0.7 + t, v * 0.5 + t] }).collect() };
        for text in [
            "cp_tune=1,40,0,0,0,0,0,0;",
            "cp_details=1,60,0;",
            "cp_details=1,0,60;",
            "cp_vignette=1,-60,0,25,0.5,0.5;",
            "cp_hdr=1,70,0,0;",
        ] {
            let ops = crate::recipe::parse(text);
            assert!(matches!(ops[..], [ref op] if !matches!(op, crate::recipe::Op::Unknown(_))), "{text} did not parse");
            let out = crate::render::apply(&img, &ops);
            let moved = out.px.iter().zip(&img.px).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
            assert!(moved > 0.01, "{text} left the picture unchanged ({moved})");
        }
    }
}
