// Window-wide state shared by the views (Svelte 5 runes).
import { api, type Folder } from "./api";

class Store {
  folders = $state<Folder[]>([]);
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
    this.folders = fresh;
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
