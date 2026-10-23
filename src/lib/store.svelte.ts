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
