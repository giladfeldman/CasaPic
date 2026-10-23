// Every call the window makes to the Rust side (src-tauri/src/lib.rs), typed in one place.
import { invoke } from "@tauri-apps/api/core";

export type PhotoRow = [id: number, star: number, thumbLevel: number];

export interface Folder {
  id: number;
  name: string;
  path: string;
  date: number | null;
  first: number | null;
  photos: PhotoRow[];
}

export interface Photo {
  id: number;
  name: string;
  path: string;
  folder: string;
  size: number;
  width: number;
  height: number;
  taken: number | null;
  mtime: number;
  camera: string | null;
  star: boolean;
  edits: string[];
}

export interface EditState {
  id: number;
  version: number;
  w: number;
  h: number;
  undo: string | null;
  redo: string | null;
  fill: number;
  edits: string[];
  /** histogram of the picture shown: 64 bins each for R, G, B */
  hist: number[];
}

export type Fix = "rotate" | "crop" | "straighten" | "autocontrast" | "autocolor" | "lucky" | "fill" | "revert";

export const api = {
  library: () => invoke<Folder[]>("library"),
  photo: (id: number) => invoke<Photo>("photo", { id }),
  setStar: (ids: number[], star: boolean) => invoke<void>("set_star", { ids, star }),
  rotate: (ids: number[], turns: number) => invoke<void>("rotate", { ids, turns }),
  counts: () => invoke<{ folders: number; photos: number }>("counts"),
  locate: (id: number) => invoke<void>("locate_on_disk", { id }),
  editOpen: (id: number, edge: number) => invoke<EditState>("edit_open", { id, edge }),
  prepare: (ids: number[], edge: number) => invoke<void>("edit_prepare", { ids, edge }),
  editApply: (kind: Fix, params: number[] = []) => invoke<EditState>("edit_apply", { kind, params }),
  fillPreview: (value: number) => invoke<EditState>("edit_fill_preview", { value }),
  undo: (redo = false) => invoke<EditState>("edit_undo", { redo }),
  defaultExportDir: (id: number) => invoke<string>("default_export_dir", { id }),
  timingMode: () => invoke<"off" | "timing" | "bench">("timing_mode"),
  mark: (name: string) => invoke<void>("timing_mark", { name }),
  quit: () => invoke<void>("quit"),
  exportPhotos: (ids: number[], dest: string | null, maxEdge: number | null, quality: number) =>
    invoke<string[]>("export_photos", { ids, dest, maxEdge, quality }),
};

const ORIGIN = "http://casapic.localhost";

export const thumbUrl = (id: number, v: number) => `${ORIGIN}/thumb/${id}?v=${v}`;

/** Fetches the Edit-view picture (JPEG) for a state returned by an edit command, decoded. */
export async function fetchPreview(version: number): Promise<ImageBitmap> {
  const r = await fetch(`${ORIGIN}/preview/${version}`);
  if (!r.ok) throw new Error(`preview ${r.status}`);
  return createImageBitmap(await r.blob());
}

export function fmtDate(secs: number | null): string {
  if (secs == null) return "";
  // dates are stored as camera-local wall-clock seconds, so read them back as UTC
  return new Date(secs * 1000).toLocaleString(undefined, { timeZone: "UTC", dateStyle: "medium", timeStyle: "short" });
}

export function fmtDay(secs: number | null): string {
  if (secs == null) return "";
  return new Date(secs * 1000).toLocaleDateString(undefined, { timeZone: "UTC", dateStyle: "medium" });
}

export function fmtSize(bytes: number): string {
  if (bytes >= 1 << 20) return `${(bytes / (1 << 20)).toFixed(1)} MB`;
  return `${Math.max(1, Math.round(bytes / 1024))} KB`;
}
