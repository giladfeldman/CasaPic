// Window-wide state shared by the views (Svelte 5 runes).
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { api, type Folder, type Options } from "./api";

export type FolderSort = "date" | "changed" | "size" | "name";

class Store {
  /** the folders as the index returns them */
  allFolders = $state<Folder[]>([]);
  /** folder list: "year" (flat, grouped by year, as Picasa's default) or "tree" */
  folderView = $state<"year" | "tree">((localStorage.getItem("folderView") as "year" | "tree") ?? "year");
  folderSort = $state<FolderSort>((localStorage.getItem("folderSort") as FolderSort) ?? "date");
  folderReverse = $state(localStorage.getItem("folderReverse") === "1");
  /** the folders in the order of the folder list (the grid follows it, as in Picasa) */
  folders = $derived.by(() => {
    const key: Record<FolderSort, (f: Folder) => number | string> = {
      date: (f) => -(f.date ?? 0),
      changed: (f) => -f.changed,
      size: (f) => -f.bytes,
      name: (f) => f.name.toLowerCase(),
    };
    // the tree is always in name order inside each branch: sort by full path
    const k = this.folderView === "tree" ? (f: Folder) => f.path.toLowerCase() : key[this.folderSort];
    const out = [...this.allFolders].sort((a, b) => {
      const x = k(a), y = k(b);
      return x < y ? -1 : x > y ? 1 : a.name.localeCompare(b.name);
    });
    return this.folderReverse && this.folderView !== "tree" ? out.reverse() : out;
  });
  /** every photo id in display order (folders newest first, photos by date) */
  order = $derived(this.folders.flatMap((f) => f.photos.map((p) => p[0])));
  folderOf = $derived(new Map(this.folders.flatMap((f) => f.photos.map((p) => [p[0], f] as const))));
  starred = $derived(new Set(this.folders.flatMap((f) => f.photos.filter((p) => p[1]).map((p) => p[0]))));

  selection = $state<Set<number>>(new Set());
  /** the photo last clicked; anchor for Shift-click and the arrow keys */
  focus = $state<number | null>(null);
  /** bumped per photo whenever its thumbnail changes, so the image reloads */
  thumbV = $state<Record<number, number>>({});

  view = $state<"library" | "edit">("library");
  thumbSize = $state(Number(localStorage.getItem("thumbSize") ?? 140));
  showProperties = $state(localStorage.getItem("showProperties") === "1");
  activeFolder = $state<number | null>(null);
  status = $state("");
  exportFor = $state<number[] | null>(null);
  message = $state<string | null>(null);
  /** speed-measurement mode (CASAPIC_TIMING set): the views report their timings */
  timing = false;
  /** id of the photo last drawn in the Edit view (the speed check waits on it) */
  drawn = $state<number | null>(null);
  /** set while the Edit view is open: rotations go through its undo history */
  editRotate: ((turns: number) => unknown) | null = null;
  /** set while the Edit view is open: pasted effects go through its undo history */
  editAdd: ((text: string) => unknown) | null = null;
  /** effects copied with Copy All Effects (recipe text) */
  effectsClip = $state<string | null>(null);

  async reload() {
    const fresh = await api.library();
    const known = new Set(fresh.flatMap((f) => f.photos.map((p) => p[0])));
    for (const p of fresh.flatMap((f) => f.photos)) {
      if (this.thumbV[p[0]] == null) this.thumbV[p[0]] = p[2];
    }
    this.allFolders = fresh;
    this.selection = new Set([...this.selection].filter((id) => known.has(id)));
    if (this.focus != null && !known.has(this.focus)) this.focus = null;
  }

  thumbsChanged(ids: number[]) {
    for (const id of ids) this.thumbV[id] = (this.thumbV[id] ?? 0) + 1;
  }

  select(id: number, mode: "only" | "toggle" | "range" = "only") {
    if (mode === "toggle") {
      const s = new Set(this.selection);
      if (s.has(id)) s.delete(id);
      else s.add(id);
      this.selection = s;
    } else if (mode === "range" && this.focus != null) {
      const a = this.order.indexOf(this.focus);
      const b = this.order.indexOf(id);
      const [lo, hi] = a < b ? [a, b] : [b, a];
      this.selection = new Set(this.order.slice(lo, hi + 1));
      return; // keep the anchor
    } else {
      this.selection = new Set([id]);
    }
    this.focus = id;
    this.activeFolder = this.folderOf.get(id)?.id ?? this.activeFolder;
  }

  selectAll() {
    this.selection = new Set(this.order);
  }
  clearSelection() {
    this.selection = new Set();
  }
  invertSelection() {
    this.selection = new Set(this.order.filter((id) => !this.selection.has(id)));
  }
  selectStarred() {
    this.selection = new Set(this.order.filter((id) => this.starred.has(id)));
  }

  /** the selection in display order, or the focused photo if nothing is selected */
  picked(): number[] {
    const ids = this.order.filter((id) => this.selection.has(id));
    return ids.length ? ids : this.focus != null ? [this.focus] : [];
  }

  async toggleStar(ids = this.picked()) {
    if (!ids.length) return;
    const on = !ids.every((id) => this.starred.has(id));
    await api.setStar(ids, on);
    for (const f of this.folders) for (const p of f.photos) if (ids.includes(p[0])) p[1] = on ? 1 : 0;
  }

  /** shows `text` in the status bar for `ms` milliseconds (0 = until the next message) */
  private flashTimer: ReturnType<typeof setTimeout> | undefined;
  flash(text: string, ms = 4000) {
    clearTimeout(this.flashTimer);
    this.status = text;
    this.flashing = ms > 0;
    if (ms) this.flashTimer = setTimeout(() => { this.flashing = false; if (this.status === text) this.status = ""; }, ms);
  }
  private flashing = false;
  /** status from the background work; an empty one does not cut a confirmation short */
  backendStatus(text: string) {
    if (text || !this.flashing) this.status = text;
  }

  async copyEffects(id = this.picked()[0]) {
    if (id == null) return;
    try {
      const text = await api.copyEffects(id);
      this.effectsClip = text || null;
      this.flash(text ? "Effects copied" : "This photo has no effects to copy");
    } catch (e) {
      this.message = String(e);
    }
  }

  async pasteEffects(ids = this.picked()) {
    if (!this.effectsClip) return;
    try {
      if (this.view === "edit" && this.editAdd) await this.editAdd(this.effectsClip);
      else if (ids.length) {
        const n = await api.pasteEffects(ids, this.effectsClip);
        this.flash(`Effects pasted on ${n} photo${n === 1 ? "" : "s"}`);
      }
    } catch (e) {
      this.message = String(e);
    }
  }

  /** Batch Edit: one fix on every selected photo (each photo's own recipe, so one undo step each) */
  async batch(kind: string, label: string, ids = this.picked()) {
    if (!ids.length) return;
    this.flash(`${label} on ${ids.length} photo${ids.length === 1 ? "" : "s"}…`, 0);
    try {
      const n = await api.batchEdit(ids, kind);
      this.flash(`${label}: ${n} photo${n === 1 ? "" : "s"} changed`);
    } catch (e) {
      this.message = String(e);
    }
  }

  async rotate(turns: number, ids = this.picked()) {
    if (this.view === "edit" && this.editRotate) await this.editRotate(turns);
    else if (ids.length) await api.rotate(ids, turns);
  }

  setFolderView(v: "year" | "tree") {
    this.folderView = v;
    localStorage.setItem("folderView", v);
  }
  setFolderSort(s: FolderSort) {
    this.folderSort = s;
    localStorage.setItem("folderSort", s);
  }
  toggleFolderReverse() {
    this.folderReverse = !this.folderReverse;
    localStorage.setItem("folderReverse", this.folderReverse ? "1" : "0");
  }

  /** open dialogs */
  folderManager = $state(false);
  /** the Options (from settings.json), loaded at start and after the Options dialog */
  options = $state<Options | null>(null);
  /** a question with a text box (Rename, New Folder…); `run` gets the text on OK */
  prompt = $state<{ title: string; label: string; value: string; ok: string; run: (text: string) => Promise<unknown> } | null>(null);
  /** a yes/no question (Delete…) */
  confirm = $state<{ title: string; text: string; ok: string; run: () => Promise<unknown> } | null>(null);
  /** photos whose date is being adjusted (the Adjust Date and Time dialog is open) */
  dateFor = $state<number[] | null>(null);
  /** photos cut or copied with Ctrl+X / Ctrl+C, waiting for Paste into a folder */
  clip = $state<{ ids: number[]; cut: boolean } | null>(null);

  async loadSettings() {
    this.options = (await api.settings()).options;
  }

  private async guard(fn: () => Promise<unknown>) {
    try {
      await fn();
    } catch (e) {
      this.message = String(e);
    }
  }
  private plural(n: number, word = "photo") {
    return `${n} ${word}${n === 1 ? "" : "s"}`;
  }

  /** Picture > Rename (F2), one photo */
  async renamePicked(id = this.picked()[0]) {
    if (id == null) return;
    const p = await api.photo(id);
    this.prompt = {
      title: "Rename", label: "New file name", value: p.name, ok: "Rename",
      run: async (name) => {
        const n = await api.renamePhoto(id, name);
        this.flash(`Renamed to ${n}`);
      },
    };
  }

  /** Delete from Disk: to the Recycle Bin, asking first unless Options say not to */
  async deletePicked(ids = this.picked()) {
    if (!ids.length) return;
    const go = async () => {
      const n = await api.deletePhotos(ids);
      if (this.view === "edit") this.view = "library";
      this.flash(`${this.plural(n)} moved to the Recycle Bin`);
    };
    if (this.options?.confirm_delete === false) return this.guard(go);
    const one = ids.length === 1 ? (await api.photo(ids[0])).name : null;
    this.confirm = {
      title: "Delete from Disk",
      text: `${one ? `"${one}"` : this.plural(ids.length)} will be moved to the Windows Recycle Bin, together with any saved original. You can restore ${one ? "it" : "them"} from the Recycle Bin.`,
      ok: "Delete", run: go,
    };
  }

  cutOrCopy(cut: boolean, ids = this.picked()) {
    if (!ids.length) return;
    this.clip = { ids, cut };
    this.flash(`${this.plural(ids.length)} ${cut ? "cut" : "copied"}: choose a folder and press Ctrl+V`);
  }

  /** Paste (Ctrl+V) into a folder (default: the folder now active in the list) */
  async paste(dest = this.folders.find((f) => f.id === this.activeFolder)?.path) {
    const c = this.clip;
    if (!c || !dest) return;
    // cut photos pasted back where they already are: nothing to do, keep them on the clipboard
    if (c.cut && c.ids.every((id) => this.folderOf.get(id)?.path === dest)) return;
    await this.moveTo(c.ids, dest, !c.cut);
    if (c.cut) this.clip = null;
  }

  async moveTo(ids: number[], dest: string, copy: boolean) {
    await this.guard(async () => {
      const n = await api.transferPhotos(ids, dest, copy);
      this.flash(`${this.plural(n)} ${copy ? "copied" : "moved"} to ${dest.split(/[\\/]/).pop()}`);
    });
  }

  /** Move to New Folder; Split Folder Here passes the photo and every one after it */
  moveToNewFolder(ids = this.picked(), title = "Move to New Folder") {
    if (!ids.length) return;
    const f = this.folderOf.get(ids[0]);
    this.prompt = {
      title, label: `Name of the new folder (made next to "${f?.name ?? ""}")`, value: f ? `${f.name} (2)` : "", ok: "Move",
      run: async (name) => {
        await api.moveToNewFolder(ids, name);
        this.flash(`${this.plural(ids.length)} moved to ${name}`);
      },
    };
  }
  splitFolderHere(id = this.picked()[0]) {
    const f = id == null ? undefined : this.folderOf.get(id);
    if (!f) return;
    const i = f.photos.findIndex((p) => p[0] === id);
    this.moveToNewFolder(f.photos.slice(i).map((p) => p[0]), "Split Folder Here");
  }

  /** File > Copy Full Path: the photo's file path to the Windows clipboard */
  async copyPath(id = this.picked()[0]) {
    if (id == null) return;
    await this.guard(async () => {
      const p = await api.photo(id);
      await navigator.clipboard.writeText(p.path);
      this.flash(`Copied: ${p.path}`);
    });
  }
  async openWith(how: "editor" | "choose", id = this.picked()[0]) {
    if (id == null) return;
    await this.guard(() => api.openWith(id, how));
  }

  /** File > Save a Copy: the photo with its edits, as a new file beside it */
  async saveCopy(id = this.picked()[0]) {
    if (id == null) return;
    await this.guard(async () => {
      const p = await api.saveCopy(id, null);
      this.flash(`Saved a copy: ${p.split(/[\\/]/).pop()}`);
    });
  }
  /** File > Save As: the photo with its edits, under a name and folder you choose */
  async saveAs(id = this.picked()[0]) {
    if (id == null) return;
    const p = await api.photo(id);
    const dest = await saveDialog({ title: "Save As", defaultPath: p.path.replace(/(\.[^.\\/]+)?$/, "-1.jpg"), filters: [{ name: "JPEG", extensions: ["jpg"] }] });
    if (!dest) return;
    await this.guard(async () => {
      const out = await api.saveCopy(id, dest);
      this.flash(`Saved as ${out}`);
    });
  }

  // ---- folder actions ----
  renameFolder(f: Folder) {
    this.prompt = {
      title: "Rename Folder", label: "New folder name", value: f.name, ok: "Rename",
      run: async (name) => {
        await api.renameFolder(f.path, name);
        this.flash(`Folder renamed to ${name}`);
      },
    };
  }
  newFolder(f: Folder) {
    this.prompt = {
      title: "New Folder", label: `Name of the new folder inside "${f.name}"`, value: "New Folder", ok: "Create",
      run: async (name) => {
        await api.newFolder(f.path, name);
        await this.reload();
        this.flash(`Folder ${name} created`);
      },
    };
  }
  async moveFolder(f: Folder) {
    const into = await openDialog({ directory: true, title: `Move "${f.name}" into…` });
    if (typeof into !== "string") return;
    await this.guard(async () => {
      await api.moveFolder(f.path, into);
      this.flash(`Folder ${f.name} moved`);
    });
  }
  deleteFolder(f: Folder) {
    this.confirm = {
      title: "Delete Folder",
      text: `The folder "${f.name}" and everything in it (${this.plural(f.photos.length)}, and any other files) will be moved to the Windows Recycle Bin. You can restore it from there.`,
      ok: "Delete",
      run: async () => {
        await api.deleteFolder(f.path);
        this.flash(`Folder ${f.name} moved to the Recycle Bin`);
      },
    };
  }
  /** folder whose description is being edited in the grid (Folder > Edit Description) */
  editDescription = $state<number | null>(null);
  optionsOpen = $state(false);

  setThumbSize(v: number) {
    this.thumbSize = v;
    localStorage.setItem("thumbSize", String(v));
  }
  toggleProperties() {
    this.showProperties = !this.showProperties;
    localStorage.setItem("showProperties", this.showProperties ? "1" : "0");
  }
}

export const store = new Store();
