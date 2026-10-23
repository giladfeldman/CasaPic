//! Edit recipes: the list of fixes applied to a photo, in order, stored as text in the
//! sidecar (`filters=` line), in the style of Picasa's `.picasa.ini` (`id=1,param,…;`).
//! Automatic fixes store the numbers they computed, so a recipe always renders the same
//! picture at any size (preview or full-resolution export).

use std::fmt::Write as _;

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// quarter turns clockwise (1–3)
    Rotate(i32),
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
    /// an entry this version does not understand; kept verbatim so it is not lost
    Unknown(String),
}

impl Op {
    /// The name shown on Undo/Redo buttons ("Undo Fill Light"), as in Picasa.
    pub fn label(&self) -> &'static str {
        match self {
            Op::Rotate(_) => "Rotate",
            Op::Crop { .. } => "Crop",
            Op::Straighten(_) => "Straighten",
            Op::AutoContrast { .. } => "Auto Contrast",
            Op::AutoColor { .. } => "Auto Color",
            Op::Lucky { .. } => "I'm Feeling Lucky",
            Op::Fill(_) => "Fill Light",
            Op::Unknown(_) => "Edit",
        }
    }
}

fn f(v: f32) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" { "0".into() } else { s.to_string() }
}

pub fn to_string(ops: &[Op]) -> String {
    let mut s = String::new();
    for op in ops {
        let _ = match op {
            Op::Rotate(q) => write!(s, "rot=1,{q};"),
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
            Op::Unknown(raw) => write!(s, "{raw};"),
        };
    }
    s
}

pub fn parse(text: &str) -> Vec<Op> {
    let mut out = Vec::new();
    for item in text.split(';').map(str::trim).filter(|x| !x.is_empty()) {
        let (key, rest) = item.split_once('=').unwrap_or((item, ""));
        let mut parts = rest.split(',');
        let version = parts.next().unwrap_or("");
        let nums: Vec<f32> = parts.filter_map(|p| p.trim().parse().ok()).collect();
        let op = match (key, version, nums.as_slice()) {
            ("rot", "1", [q]) => Some(Op::Rotate(*q as i32)),
            ("crop", "1", [a, b, c, d]) => Some(Op::Crop { x0: *a, y0: *b, x1: *c, y1: *d }),
            ("tilt", "1", [a]) => Some(Op::Straighten(*a)),
            ("autolight", "1", [lo, hi]) => Some(Op::AutoContrast { lo: *lo, hi: *hi }),
            ("autocolor", "1", [a, b, c]) => Some(Op::AutoColor { gain: [*a, *b, *c] }),
            ("enhance", "1", [a, b, c, d, e, g]) => Some(Op::Lucky { lo: [*a, *b, *c], hi: [*d, *e, *g] }),
            ("fill", "1", [a]) => Some(Op::Fill(*a)),
            _ => None,
        };
        out.push(op.unwrap_or_else(|| Op::Unknown(item.to_string())));
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
            Op::Unknown("future=2,1,2".into()),
        ];
        let s = to_string(&ops);
        assert_eq!(parse(&s), ops);
        assert!(s.starts_with("rot=1,1;crop=1,0.1,0.05,0.9,0.95;"), "{s}");
    }

    #[test]
    fn empty_and_garbage() {
        assert!(parse("").is_empty());
        assert_eq!(parse("fill=1,abc;"), vec![Op::Unknown("fill=1,abc".into())]);
    }
}
