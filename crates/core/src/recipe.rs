//! Edit recipes: the list of fixes applied to a photo, in order, stored as text in the
//! sidecar (`filters=` line), in the style of Picasa's `.picasa.ini` (`id=1,param,…;`).
//! Automatic fixes store the numbers they computed, so a recipe always renders the same
//! picture at any size (preview or full-resolution export).
//!
//! The Tuning tab and the effects use Picasa's own entry names and parameter order, read from
//! Picasa 3.9 in the lab (2026-10-24; docs/results-build2.md). Colours are written as Picasa
//! writes them, 8 hex digits AARRGGBB.

use std::fmt::Write as _;

/// A colour as Picasa stores it (AARRGGBB); only R, G and B are used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u32);

impl Rgb {
    pub fn channels(self) -> [f32; 3] {
        let v = self.0;
        [(v >> 16) & 255, (v >> 8) & 255, v & 255].map(|c| c as f32 / 255.0)
    }
    pub fn from_channels(c: [f32; 3]) -> Rgb {
        let b = c.map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u32);
        Rgb(0xff00_0000 | (b[0] << 16) | (b[1] << 8) | b[2])
    }
    /// "no colour chosen" (the Tuning tab's neutral colour before the picker is used)
    pub fn is_none(self) -> bool {
        self.0 & 0x00ff_ffff == 0
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// quarter turns clockwise (1–3)
    Rotate(i32),
    /// mirror: 1 = left-right, 2 = top-bottom, 3 = both (Picasa's Ctrl+Shift+H / V)
    Flip(u8),
    /// crop rectangle as fractions (0–1) of the picture at this point in the recipe
    Crop { x0: f32, y0: f32, x1: f32, y1: f32 },
    /// straighten angle in degrees, positive = clockwise
    Straighten(f32),
    /// black and white points shared by R, G and B (0–1)
    AutoContrast { lo: f32, hi: f32 },
    /// one gain per colour (R, G, B)
    AutoColor { gain: [f32; 3] },
    /// black and white points per colour (R, G, B)
    Lucky { lo: [f32; 3], hi: [f32; 3] },
    /// fill light amount (0–1)
    Fill(f32),
    /// Tuning tab: fill light 0–1, highlights 0–1, shadows 0–1, neutral colour (picker),
    /// colour temperature −1 (cool) to 1 (warm)
    Tune { fill: f32, highlights: f32, shadows: f32, neutral: Rgb, temp: f32 },
    /// Sharpen amount (Picasa's default 0.6)
    Sharpen(f32),
    Sepia,
    BlackWhite,
    Warmify,
    /// Film Grain: strength (Picasa's default 10) and whether the grain lightens
    Grain { amount: f32, lighten: bool },
    /// Tint: Picasa's "fade" number (kept; see render) and the tint colour
    Tint { fade: f32, color: Rgb },
    /// Saturation −1 to 1
    Saturation(f32),
    /// Soft Focus: centre (fractions of the picture), size and amount sliders (−1 to 1, 0 = default)
    SoftFocus { x: f32, y: f32, size: f32, amount: f32 },
    /// Glow: intensity and radius
    Glow { intensity: f32, radius: f32 },
    /// Filtered B&W: the filter colour
    FilteredBw(Rgb),
    /// Focal B&W: centre, size and sharpness sliders (−1 to 1, 0 = default)
    FocalBw { x: f32, y: f32, size: f32, sharpness: f32 },
    /// Graduated Tint: handle position (fractions), feather 0–1, shade 0–1, colour, turn
    GradTint { x: f32, y: f32, feather: f32, shade: f32, color: Rgb, turn: f32 },
    /// Studio tab, Tune Image (Snapseed's ranges, −100 to 100): brightness, contrast,
    /// saturation, ambiance, highlights, shadows, warmth
    TuneImage { brightness: f32, contrast: f32, saturation: f32, ambiance: f32, highlights: f32, shadows: f32, warmth: f32 },
    /// Studio tab, Details: structure −100 to 100, sharpening 0 to 100
    Details { structure: f32, sharpening: f32 },
    /// Studio tab, Vignette: outer and inner brightness −100 to 100, centre size 0 to 100,
    /// centre (fractions of the picture)
    Vignette { outer: f32, inner: f32, size: f32, x: f32, y: f32 },
    /// Studio tab, HDR Scape: strength 0 to 100, brightness and saturation −100 to 100
    HdrScape { strength: f32, brightness: f32, saturation: f32 },
    /// an entry this version does not understand; kept verbatim so it is not lost
    Unknown(String),
}

impl Op {
    /// The name shown on Undo/Redo buttons ("Undo Fill Light"), as in Picasa.
    pub fn label(&self) -> &'static str {
        match self {
            Op::Rotate(_) => "Rotate",
            Op::Flip(_) => "Flip",
            Op::Crop { .. } => "Crop",
            Op::Straighten(_) => "Straighten",
            Op::AutoContrast { .. } => "Auto Contrast",
            Op::AutoColor { .. } => "Auto Color",
            Op::Lucky { .. } => "I'm Feeling Lucky",
            Op::Fill(_) => "Fill Light",
            Op::Tune { .. } => "Tuning",
            Op::Sharpen(_) => "Sharpen",
            Op::Sepia => "Sepia",
            Op::BlackWhite => "B&W",
            Op::Warmify => "Warmify",
            Op::Grain { .. } => "Film Grain",
            Op::Tint { .. } => "Tint",
            Op::Saturation(_) => "Saturation",
            Op::SoftFocus { .. } => "Soft Focus",
            Op::Glow { .. } => "Glow",
            Op::FilteredBw(_) => "Filtered B&W",
            Op::FocalBw { .. } => "Focal B&W",
            Op::GradTint { .. } => "Graduated Tint",
            Op::TuneImage { .. } => "Tune Image",
            Op::Details { .. } => "Details",
            Op::Vignette { .. } => "Vignette",
            Op::HdrScape { .. } => "HDR Scape",
            Op::Unknown(_) => "Edit",
        }
    }

    /// True for fixes that only change colours and tones (no geometry): these can be copied
    /// from one photo to another ("Copy All Effects").
    pub fn is_effect(&self) -> bool {
        !matches!(self, Op::Rotate(_) | Op::Flip(_) | Op::Crop { .. } | Op::Straighten(_) | Op::Unknown(_))
    }
}

fn f(v: f32) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" { "0".into() } else { s.to_string() }
}

fn hex(c: Rgb) -> String {
    format!("{:08x}", c.0)
}

pub fn to_string(ops: &[Op]) -> String {
    let mut s = String::new();
    for op in ops {
        let _ = match op {
            Op::Rotate(q) => write!(s, "rot=1,{q};"),
            Op::Flip(b) => write!(s, "flip=1,{b};"),
            Op::Crop { x0, y0, x1, y1 } => write!(s, "crop=1,{},{},{},{};", f(*x0), f(*y0), f(*x1), f(*y1)),
            Op::Straighten(a) => write!(s, "tilt=1,{};", f(*a)),
            Op::AutoContrast { lo, hi } => write!(s, "autolight=1,{},{};", f(*lo), f(*hi)),
            Op::AutoColor { gain } => write!(s, "autocolor=1,{},{},{};", f(gain[0]), f(gain[1]), f(gain[2])),
            Op::Lucky { lo, hi } => write!(
                s,
                "enhance=1,{},{},{},{},{},{};",
                f(lo[0]), f(lo[1]), f(lo[2]), f(hi[0]), f(hi[1]), f(hi[2])
            ),
            Op::Fill(a) => write!(s, "fill=1,{};", f(*a)),
            Op::Tune { fill, highlights, shadows, neutral, temp } => {
                write!(s, "finetune2=1,{},{},{},{},{};", f(*fill), f(*highlights), f(*shadows), hex(*neutral), f(*temp))
            }
            Op::Sharpen(a) => write!(s, "unsharp2=1,{};", f(*a)),
            Op::Sepia => write!(s, "sepia=1;"),
            Op::BlackWhite => write!(s, "bw=1;"),
            Op::Warmify => write!(s, "warm=1;"),
            Op::Grain { amount, lighten } => write!(s, "PicnikGrain=1,{},{};", f(*amount), *lighten as i32),
            Op::Tint { fade, color } => write!(s, "PicnikTint=1,{},{};", f(*fade), hex(*color)),
            Op::Saturation(a) => write!(s, "sat=1,{};", f(*a)),
            Op::SoftFocus { x, y, size, amount } => write!(s, "radblur=1,{},{},{},{};", f(*x), f(*y), f(*size), f(*amount)),
            Op::Glow { intensity, radius } => write!(s, "glow2=1,{},{};", f(*intensity), f(*radius)),
            Op::FilteredBw(c) => write!(s, "ansel=1,{};", hex(*c)),
            Op::FocalBw { x, y, size, sharpness } => write!(s, "radsat=1,{},{},{},{};", f(*x), f(*y), f(*size), f(*sharpness)),
            Op::GradTint { x, y, feather, shade, color, turn } => {
                write!(s, "dir_tint=1,{},{},{},{},{},{};", f(*x), f(*y), f(*feather), f(*shade), hex(*color), f(*turn))
            }
            Op::TuneImage { brightness, contrast, saturation, ambiance, highlights, shadows, warmth } => write!(
                s,
                "cp_tune=1,{},{},{},{},{},{},{};",
                f(*brightness), f(*contrast), f(*saturation), f(*ambiance), f(*highlights), f(*shadows), f(*warmth)
            ),
            Op::Details { structure, sharpening } => write!(s, "cp_details=1,{},{};", f(*structure), f(*sharpening)),
            Op::Vignette { outer, inner, size, x, y } => write!(s, "cp_vignette=1,{},{},{},{},{};", f(*outer), f(*inner), f(*size), f(*x), f(*y)),
            Op::HdrScape { strength, brightness, saturation } => write!(s, "cp_hdr=1,{},{},{};", f(*strength), f(*brightness), f(*saturation)),
            Op::Unknown(raw) => write!(s, "{raw};"),
        };
    }
    s
}

fn num(p: &str) -> Option<f32> {
    p.trim().parse::<f32>().ok().filter(|v| v.is_finite())
}

/// A colour parameter: exactly 8 hex digits (AARRGGBB).
fn color(p: &str) -> Option<Rgb> {
    let p = p.trim();
    (p.len() == 8 && p.chars().all(|c| c.is_ascii_hexdigit())).then(|| u32::from_str_radix(p, 16).ok()).flatten().map(Rgb)
}

fn parse_one(key: &str, version: &str, r: &[&str]) -> Option<Op> {
    if version != "1" {
        return None;
    }
    let n = |i: usize| r.get(i).and_then(|p| num(p));
    let c = |i: usize| r.get(i).and_then(|p| color(p));
    let len = |k: usize| (r.len() == k).then_some(());
    Some(match key {
        "rot" => {
            len(1)?;
            Op::Rotate(n(0)? as i32)
        }
        "flip" => {
            len(1)?;
            let b = n(0)? as u8;
            (1..=3).contains(&b).then_some(Op::Flip(b))?
        }
        "crop" => {
            len(4)?;
            Op::Crop { x0: n(0)?, y0: n(1)?, x1: n(2)?, y1: n(3)? }
        }
        "tilt" => {
            len(1)?;
            Op::Straighten(n(0)?)
        }
        "autolight" => {
            len(2)?;
            Op::AutoContrast { lo: n(0)?, hi: n(1)? }
        }
        "autocolor" => {
            len(3)?;
            Op::AutoColor { gain: [n(0)?, n(1)?, n(2)?] }
        }
        "enhance" => {
            len(6)?;
            Op::Lucky { lo: [n(0)?, n(1)?, n(2)?], hi: [n(3)?, n(4)?, n(5)?] }
        }
        "fill" => {
            len(1)?;
            Op::Fill(n(0)?)
        }
        "finetune2" => {
            len(5)?;
            Op::Tune { fill: n(0)?, highlights: n(1)?, shadows: n(2)?, neutral: c(3)?, temp: n(4)? }
        }
        "unsharp2" => {
            len(1)?;
            Op::Sharpen(n(0)?)
        }
        "sepia" => {
            len(0)?;
            Op::Sepia
        }
        "bw" => {
            len(0)?;
            Op::BlackWhite
        }
        "warm" => {
            len(0)?;
            Op::Warmify
        }
        "PicnikGrain" => {
            len(2)?;
            Op::Grain { amount: n(0)?, lighten: n(1)? != 0.0 }
        }
        "PicnikTint" => {
            len(2)?;
            Op::Tint { fade: n(0)?, color: c(1)? }
        }
        "sat" => {
            len(1)?;
            Op::Saturation(n(0)?)
        }
        "radblur" => {
            len(4)?;
            Op::SoftFocus { x: n(0)?, y: n(1)?, size: n(2)?, amount: n(3)? }
        }
        "glow2" => {
            len(2)?;
            Op::Glow { intensity: n(0)?, radius: n(1)? }
        }
        "ansel" => {
            len(1)?;
            Op::FilteredBw(c(0)?)
        }
        "radsat" => {
            len(4)?;
            Op::FocalBw { x: n(0)?, y: n(1)?, size: n(2)?, sharpness: n(3)? }
        }
        "dir_tint" => {
            len(6)?;
            Op::GradTint { x: n(0)?, y: n(1)?, feather: n(2)?, shade: n(3)?, color: c(4)?, turn: n(5)? }
        }
        "cp_tune" => {
            len(7)?;
            Op::TuneImage { brightness: n(0)?, contrast: n(1)?, saturation: n(2)?, ambiance: n(3)?, highlights: n(4)?, shadows: n(5)?, warmth: n(6)? }
        }
        "cp_details" => {
            len(2)?;
            Op::Details { structure: n(0)?, sharpening: n(1)? }
        }
        "cp_vignette" => {
            len(5)?;
            Op::Vignette { outer: n(0)?, inner: n(1)?, size: n(2)?, x: n(3)?, y: n(4)? }
        }
        "cp_hdr" => {
            len(3)?;
            Op::HdrScape { strength: n(0)?, brightness: n(1)?, saturation: n(2)? }
        }
        _ => return None,
    })
}

pub fn parse(text: &str) -> Vec<Op> {
    let mut out = Vec::new();
    for item in text.split(';').map(str::trim).filter(|x| !x.is_empty()) {
        let (key, rest) = item.split_once('=').unwrap_or((item, ""));
        let mut parts = rest.split(',');
        let version = parts.next().unwrap_or("");
        let raw: Vec<&str> = parts.filter(|p| !p.trim().is_empty()).collect();
        out.push(parse_one(key, version, &raw).unwrap_or_else(|| Op::Unknown(item.to_string())));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_every_op() {
        let ops = vec![
            Op::Rotate(1),
            Op::Crop { x0: 0.1, y0: 0.05, x1: 0.9, y1: 0.95 },
            Op::Straighten(-2.5),
            Op::AutoContrast { lo: 0.012, hi: 0.985 },
            Op::AutoColor { gain: [1.05, 0.98, 0.91] },
            Op::Lucky { lo: [0.0, 0.01, 0.02], hi: [1.0, 0.99, 0.98] },
            Op::Fill(0.4),
            Op::Tune { fill: 0.1, highlights: 0.2, shadows: 0.05, neutral: Rgb(0x0078_8080), temp: -0.3 },
            Op::Sharpen(0.6),
            Op::Sepia,
            Op::BlackWhite,
            Op::Warmify,
            Op::Grain { amount: 10.0, lighten: true },
            Op::Tint { fade: 0.0, color: Rgb(0x0080_cfff) },
            Op::Saturation(-0.5),
            Op::SoftFocus { x: 0.5, y: 0.5, size: 0.0, amount: 0.0 },
            Op::Glow { intensity: 0.65, radius: 3.0 },
            Op::FilteredBw(Rgb(0xffff_0000)),
            Op::FocalBw { x: 0.3, y: 1.0, size: -0.5, sharpness: 0.5 },
            Op::GradTint { x: 0.5, y: 0.5, feather: 0.25, shade: 0.25, color: Rgb(0xffff_ffff), turn: 0.0 },
            Op::Flip(3),
            Op::TuneImage { brightness: 10.0, contrast: -5.0, saturation: 20.0, ambiance: 30.0, highlights: -40.0, shadows: 15.0, warmth: 5.0 },
            Op::Details { structure: 25.0, sharpening: 10.0 },
            Op::Vignette { outer: -64.4, inner: 14.0, size: 20.8, x: 0.5, y: 0.5 },
            Op::HdrScape { strength: 50.0, brightness: 0.0, saturation: -10.0 },
            Op::Unknown("future=2,1,2".into()),
        ];
        let s = to_string(&ops);
        assert_eq!(parse(&s), ops);
        assert!(s.starts_with("rot=1,1;crop=1,0.1,0.05,0.9,0.95;"), "{s}");
    }

    #[test]
    fn reads_picasas_own_text() {
        // entries exactly as Picasa 3.9 wrote them in the lab
        let ops = parse("PicnikTint=1,0.000000,0080cfff;finetune2=1,0,0.023529,0.019608,00000000,0;ansel=1,ffffffff;dir_tint=1,0.5,0.5,0.25,0.25,ffffffff,0;");
        assert_eq!(ops[0], Op::Tint { fade: 0.0, color: Rgb(0x0080_cfff) });
        assert!(matches!(ops[1], Op::Tune { neutral, .. } if neutral.is_none()));
        assert_eq!(ops[2], Op::FilteredBw(Rgb(0xffff_ffff)));
        assert!(matches!(ops[3], Op::GradTint { .. }));
        assert_eq!(Rgb(0x0080_cfff).channels(), [128.0 / 255.0, 207.0 / 255.0, 1.0]);
    }

    #[test]
    fn empty_and_garbage() {
        assert!(parse("").is_empty());
        assert_eq!(parse("fill=1,abc;"), vec![Op::Unknown("fill=1,abc".into())]);
        assert_eq!(parse("ansel=1,zzzzzzzz;"), vec![Op::Unknown("ansel=1,zzzzzzzz".into())]);
    }
}
