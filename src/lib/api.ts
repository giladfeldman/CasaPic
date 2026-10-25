// Every call the window makes to the Rust side (src-tauri/src/lib.rs), typed in one place.
import { invoke } from "@tauri-apps/api/core";

/** one photo in the grid: date = taken, or the file's date (seconds, camera wall-clock) */
export type PhotoRow = [id: number, star: number, thumbLevel: number, hidden: number, date: number];

export interface Folder {
  id: number;
  name: string;
  path: string;
  date: number | null;
  first: number | null;
  /** "Add a description" text */
  description: string;
  /** photo order: "" = date, "name", "size"; "-" prefix = reversed */
  sort: string;
  /** the Folder Manager folder it is scanned from */
  root: string;
  /** newest file change (seconds) */
  changed: number;
  /** total bytes of its photos */
  bytes: number;
  /** Folder > Hide */
  hidden: boolean;
  photos: PhotoRow[];
}

export type Scan = "always" | "once" | "remove";
export interface Rule { path: string; scan: Scan }
export interface Options {
  confirm_delete: boolean;
  single_click_exit_edit: boolean;
  file_types: string[];
  slide_seconds: number;
  slide_transition: string;
}
export interface Settings { folders: Rule[]; folders_confirmed: boolean; options: Options }
export interface FmNode { name: string; path: string; more: boolean }

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
  tags: string[];
  caption: string;
  /** album tokens */
  albums: string[];
  hidden: boolean;
}

export interface Album { token: string; name: string; date: string; location: string; description: string }
export interface AlbumRow extends Album { count: number; cover: number | null }

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
  /** a half-size preview while a slider moves (the full one follows) */
  quick: boolean;
  /** the photo has a saved edit; its original is kept for Revert */
  saved: boolean;
  /** the recipe's last fix as recipe text */
  last: string;
}

export type Fix = "rotate" | "crop" | "straighten" | "autocontrast" | "autocolor" | "lucky" | "fill" | "revert";

export const api = {
  library: () => invoke<Folder[]>("library"),
  photo: (id: number) => invoke<Photo>("photo", { id }),
  setStar: (ids: number[], star: boolean) => invoke<void>("set_star", { ids, star }),
  rotate: (ids: number[], turns: number) => invoke<void>("rotate", { ids, turns }),
  counts: () => invoke<{ folders: number; photos: number }>("counts"),
  locate: (id: number) => invoke<void>("locate_on_disk", { id }),
  settings: () => invoke<Settings>("settings_get"),
  fmList: (path: string | null) => invoke<FmNode[]>("fm_list", { path }),
  fmSave: (rules: Rule[]) => invoke<void>("fm_save", { rules }),
  fmConfirm: () => invoke<void>("fm_confirm"),
  addFolder: (path: string) => invoke<void>("add_folder", { path }),
  removeFolder: (path: string) => invoke<void>("remove_folder", { path }),
  optionsSave: (options: Options) => invoke<void>("options_save", { options }),
  setFolderDescription: (path: string, text: string) => invoke<void>("set_folder_description", { path, text }),
  setFolderSort: (path: string, sort: string) => invoke<void>("set_folder_sort", { path, sort }),
  openFolder: (path: string) => invoke<void>("open_folder", { path }),
  renamePhoto: (id: number, name: string) => invoke<string>("rename_photo", { id, name }),
  deletePhotos: (ids: number[]) => invoke<number>("delete_photos", { ids }),
  transferPhotos: (ids: number[], dest: string, copy: boolean) => invoke<number>("transfer_photos", { ids, dest, copy }),
  moveToNewFolder: (ids: number[], name: string) => invoke<string>("move_to_new_folder", { ids, name }),
  newFolder: (parent: string, name: string) => invoke<string>("new_folder", { parent, name }),
  renameFolder: (path: string, name: string) => invoke<string>("rename_folder", { path, name }),
  moveFolder: (path: string, into: string) => invoke<string>("move_folder", { path, into }),
  deleteFolder: (path: string) => invoke<void>("delete_folder", { path }),
  saveCopy: (id: number, dest: string | null) => invoke<string>("save_copy", { id, dest }),
  adjustDates: (ids: number[], shift: number) => invoke<number>("adjust_dates", { ids, shift }),
  clearThumbCache: () => invoke<number>("clear_thumb_cache"),
  openWith: (id: number, how: "editor" | "choose") => invoke<void>("open_with", { id, how }),
  albums: () => invoke<AlbumRow[]>("albums_list"),
  albumPhotos: (token: string) => invoke<PhotoRow[]>("album_photos", { token }),
  albumCreate: (album: Album, ids: number[]) => invoke<string>("album_create", { album, ids }),
  albumAdd: (token: string, ids: number[]) => invoke<number>("album_add", { token, ids }),
  albumRemove: (token: string, ids: number[]) => invoke<number>("album_remove", { token, ids }),
  albumEdit: (album: Album) => invoke<void>("album_edit", { album }),
  albumDelete: (token: string) => invoke<void>("album_delete", { token }),
  setTags: (ids: number[], add: string[], remove: string[]) => invoke<number>("set_tags", { ids, add, remove }),
  setCaption: (id: number, text: string) => invoke<void>("set_caption", { id, text }),
  allTags: () => invoke<[string, number][]>("all_tags"),
  picasaImport: (apply: boolean) => invoke<PicasaSummary>("picasa_import", { apply }),
  search: (query: { text: string; starred: boolean; from: number | null; to: number | null }) => invoke<number[]>("search", { query }),
  setHidden: (ids: number[], hidden: boolean) => invoke<number>("set_hidden", { ids, hidden }),
  setFolderHidden: (path: string, hidden: boolean) => invoke<void>("set_folder_hidden", { path, hidden }),
  editOpen: (id: number, edge: number) => invoke<EditState>("edit_open", { id, edge }),
  prepare: (ids: number[], edge: number) => invoke<void>("edit_prepare", { ids, edge }),
  editApply: (kind: Fix, params: number[] = []) => invoke<EditState>("edit_apply", { kind, params }),
  fillPreview: (value: number) => invoke<EditState>("edit_fill_preview", { value }),
  undo: (redo = false) => invoke<EditState>("edit_undo", { redo }),
  /** live preview of fixes given as recipe text, not saved */
  editTry: (text: string, replaceLast = false, quick = false) => invoke<EditState>("edit_try", { text, replaceLast, quick }),
  /** adds fixes given as recipe text to the photo and saves */
  editAdd: (text: string, replaceLast = false) => invoke<EditState>("edit_add", { text, replaceLast }),
  editSample: (x: number, y: number, skip: number) => invoke<[number, number, number]>("edit_sample", { x, y, skip }),
  editMagic: (kind: "light" | "color", skip: number) => invoke<number[]>("edit_magic", { kind, skip }),
  fxSheet: (texts: string[], size: number) => invoke<number>("edit_fx_sheet", { texts, size }),
  /** writes the open photo's edits into its file (original kept in .casapicoriginals) */
  editSave: () => invoke<EditState>("edit_save"),
  /** puts the saved photo's original back */
  editRevert: () => invoke<EditState>("edit_revert"),
  copyEffects: (id: number) => invoke<string>("copy_effects", { id }),
  pasteEffects: (ids: number[], text: string) => invoke<number>("paste_effects", { ids, text }),
  /** one fix on many photos: lucky | autocontrast | autocolor | recipe text */
  batchEdit: (ids: number[], kind: string) => invoke<number>("batch_edit", { ids, kind }),
  defaultExportDir: (id: number) => invoke<string>("default_export_dir", { id }),
  timingMode: () => invoke<"off" | "timing" | "bench">("timing_mode"),
  mark: (name: string) => invoke<void>("timing_mark", { name }),
  quit: () => invoke<void>("quit"),
  exportPhotos: (ids: number[], dest: string | null, maxEdge: number | null, quality: number) =>
    invoke<string[]>("export_photos", { ids, dest, maxEdge, quality }),
};

const ORIGIN = "http://casapic.localhost";

export const thumbUrl = (id: number, v: number) => `${ORIGIN}/thumb/${id}?v=${v}`;
export const fxUrl = (v: number) => `${ORIGIN}/fx/${v}`;

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

/** what Tools > Import Picasa Settings found (or brought over) */
export interface PicasaSummary {
  folders: number;
  stars: number;
  hidden_photos: number;
  hidden_folders: number;
  descriptions: number;
  albums: string[];
  album_photos: number;
  edited: number;
  kept_casapic_edits: number;
  captions: number;
  saved_in_picasa: number;
  /** Picasa edits CasaPic cannot draw: name -> photos */
  unknown: Record<string, number>;
}
