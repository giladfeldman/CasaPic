// The Tuning tab and Picasa's classic effects tab: their controls, Picasa's defaults, and the
// recipe text each setting is saved as (Picasa's own .picasa.ini entry names and order, read
// from Picasa 3.9 in the lab; the Rust core renders them, crates/core/src/effects.rs).

export interface Slider {
  key: string;
  label: string;
  min: number;
  max: number;
  step: number;
  def: number;
}

export interface Effect {
  id: string;
  label: string;
  /** no settings: applied with one click */
  oneClick?: boolean;
  sliders?: Slider[];
  /** a colour setting, as "#rrggbb" */
  color?: { label: string; def: string };
  /** a Lighten checkbox (Film Grain) */
  lighten?: boolean;
  /** the effect is centred on a point chosen by clicking the photo */
  point?: string;
  text: (v: Values) => string;
}

export type Values = Record<string, number | string | boolean>;

const n = (v: unknown) => Number(v).toFixed(4).replace(/\.?0+$/, "") || "0";
/** "#rrggbb" -> Picasa's AARRGGBB */
export const argb = (hex: unknown) => "ff" + String(hex).replace("#", "").toLowerCase().padStart(6, "0");

export const EFFECTS: Effect[] = [
  { id: "sharpen", label: "Sharpen", sliders: [{ key: "amount", label: "Amount", min: 0, max: 2, step: 0.01, def: 0.6 }],
    text: (v) => `unsharp2=1,${n(v.amount)};` },
  { id: "sepia", label: "Sepia", oneClick: true, text: () => "sepia=1;" },
  { id: "bw", label: "B&W", oneClick: true, text: () => "bw=1;" },
  { id: "warm", label: "Warmify", oneClick: true, text: () => "warm=1;" },
  { id: "grain", label: "Film Grain", sliders: [{ key: "amount", label: "Grain", min: 0, max: 30, step: 0.5, def: 10 }], lighten: true,
    text: (v) => `PicnikGrain=1,${n(v.amount)},${v.lighten ? 1 : 0};` },
  // Picasa's Tint also has a Fade slider, but its stored value changed nothing in Picasa's own
  // renders (lab round 4), so CasaPic leaves it out rather than show a slider that does nothing
  { id: "tint", label: "Tint", color: { label: "Pick Color", def: "#80cfff" },
    text: (v) => `PicnikTint=1,0,${argb(v.color).replace(/^ff/, "00")};` },
  { id: "sat", label: "Saturation", sliders: [{ key: "amount", label: "Saturation", min: -1, max: 1, step: 0.01, def: 0.1618 }],
    text: (v) => `sat=1,${n(v.amount)};` },
  { id: "soft", label: "Soft Focus", point: "Click the photo to set the sharp centre.",
    sliders: [
      { key: "size", label: "Size", min: -1, max: 1, step: 0.01, def: 0 },
      { key: "amount", label: "Amount", min: -1, max: 1, step: 0.01, def: 0 },
    ],
    text: (v) => `radblur=1,${n(v.x)},${n(v.y)},${n(v.size)},${n(v.amount)};` },
  { id: "glow", label: "Glow",
    sliders: [
      { key: "intensity", label: "Intensity", min: 0, max: 1, step: 0.01, def: 0.65 },
      { key: "radius", label: "Radius", min: 0, max: 20, step: 0.1, def: 3 },
    ],
    text: (v) => `glow2=1,${n(v.intensity)},${n(v.radius)};` },
  { id: "fbw", label: "Filtered B&W", color: { label: "Filter Color", def: "#ffffff" },
    text: (v) => `ansel=1,${argb(v.color)};` },
  { id: "focal", label: "Focal B&W", point: "Click the photo to set the centre that keeps its colour.",
    sliders: [
      { key: "size", label: "Size", min: -1, max: 1, step: 0.01, def: 0 },
      { key: "sharpness", label: "Sharpness", min: -1, max: 1, step: 0.01, def: 0 },
    ],
    text: (v) => `radsat=1,${n(v.x)},${n(v.y)},${n(v.size)},${n(v.sharpness)};` },
  { id: "grad", label: "Graduated Tint", point: "Click the photo to place the edge of the tint.",
    sliders: [
      { key: "feather", label: "Feather", min: 0, max: 1, step: 0.01, def: 0.25 },
      { key: "shade", label: "Shade", min: 0, max: 1, step: 0.01, def: 0.25 },
    ],
    color: { label: "Pick Color", def: "#ffffff" },
    text: (v) => `dir_tint=1,${n(v.x)},${n(v.y)},${n(v.feather)},${n(v.shade)},${argb(v.color)},0;` },
];

// The Studio tab: CasaPic's own tools in the style of the Snapseed app, with Snapseed's slider
// ranges (crates/core/src/studio.rs renders them). Starting values are CasaPic's choice.
const sl = (key: string, label: string, min: number, max: number, def: number): Slider => ({ key, label, min, max, step: 1, def });
export const STUDIO: Effect[] = [
  { id: "tuneimage", label: "Tune Image",
    sliders: [
      sl("brightness", "Brightness", -100, 100, 0), sl("contrast", "Contrast", -100, 100, 0),
      sl("saturation", "Saturation", -100, 100, 0), sl("ambiance", "Ambiance", -100, 100, 30),
      sl("highlights", "Highlights", -100, 100, 0), sl("shadows", "Shadows", -100, 100, 0),
      sl("warmth", "Warmth", -100, 100, 0),
    ],
    text: (v) => `cp_tune=1,${n(v.brightness)},${n(v.contrast)},${n(v.saturation)},${n(v.ambiance)},${n(v.highlights)},${n(v.shadows)},${n(v.warmth)};` },
  { id: "details", label: "Details",
    sliders: [sl("structure", "Structure", -100, 100, 30), sl("sharpening", "Sharpening", 0, 100, 15)],
    text: (v) => `cp_details=1,${n(v.structure)},${n(v.sharpening)};` },
  { id: "vignette", label: "Vignette", point: "Click the photo to move the centre (optional).",
    sliders: [sl("outer", "Outer Brightness", -100, 100, -50), sl("inner", "Inner Brightness", -100, 100, 0), sl("size", "Centre Size", 0, 100, 25)],
    text: (v) => `cp_vignette=1,${n(v.outer)},${n(v.inner)},${n(v.size)},${n(v.x)},${n(v.y)};` },
  { id: "hdr", label: "HDR Scape",
    sliders: [sl("strength", "Filter Strength", 0, 100, 50), sl("brightness", "Brightness", -100, 100, 0), sl("saturation", "Saturation", -100, 100, 0)],
    text: (v) => `cp_hdr=1,${n(v.strength)},${n(v.brightness)},${n(v.saturation)};` },
];

export function defaults(e: Effect): Values {
  const v: Values = { x: 0.5, y: 0.5 };
  for (const s of e.sliders ?? []) v[s.key] = s.def;
  if (e.color) v.color = e.color.def;
  if (e.lighten) v.lighten = false;
  return v;
}

export interface Tune {
  fill: number;
  highlights: number;
  shadows: number;
  /** neutral colour "#rrggbb", or "" for none */
  neutral: string;
  temp: number;
}

export const NO_TUNE: Tune = { fill: 0, highlights: 0, shadows: 0, neutral: "", temp: 0 };

export const tuneText = (t: Tune) =>
  `finetune2=1,${n(t.fill)},${n(t.highlights)},${n(t.shadows)},${t.neutral ? argb(t.neutral).replace(/^ff/, "00") : "00000000"},${n(t.temp)};`;

/** the Tuning values of a `finetune2=` entry, or null for anything else */
export function parseTune(text: string): Tune | null {
  const m = /^finetune2=1,([^,]+),([^,]+),([^,]+),([0-9a-fA-F]{8}),([^;]+);$/.exec(text);
  if (!m) return null;
  const rgb = m[4].slice(2).toLowerCase();
  return { fill: +m[1], highlights: +m[2], shadows: +m[3], neutral: rgb === "000000" ? "" : "#" + rgb, temp: +m[5] };
}

export const toHex = (c: number[]) =>
  "#" + c.map((v) => Math.round(Math.min(1, Math.max(0, v)) * 255).toString(16).padStart(2, "0")).join("");
