// Window-wide state shared by the views (Svelte 5 runes).
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { api, type Album, type AlbumRow, type Folder, type Options, type PhotoRow } from "./api";

export type FolderSort = "date" | "changed" | "size" | "name";

class Store {
  /** the folders as the index returns them */
  allFolders = $state<Folder[]>([]);
  /** folder list: "year" (flat, grouped by year, as Picasa's default) or "tree" */
  folderView = $state<"year" | "tree">((localStorage.getItem("folderView") as "year" | "tree") ?? "year");
  folderSort = $state<FolderSort>((localStorage.getItem("folderSort") as FolderSort) ?? "date");
  folderReverse = $state(localStorage.getItem("folderReverse") === "1");
  /** every album, by name (left panel, Albums) */
  albums = $state<AlbumRow[]>([]);
  /** the album open in the grid instead of the folders, with its photos */
  album = $state<{ token: string; photos: PhotoRow[] } | null>(null);
  /** the open album as one grid section (id -1; it is not a folder on disk) */
  albumSection = $derived.by((): Folder | null => {
    const a = this.album && this.albums.find((x) => x.token === this.album!.token);
    if (!a || !this.album) return null;
    const day = a.date ? Date.parse(a.date) / 1000 : null;
    return { id: -1, name: a.name, path: "", date: day, first: day, description: a.description, sort: "", root: "", changed: 0, bytes: 0, hidden: false, photos: this.album.photos };
  });
  /** bumped after every reload, so open panels read the photo again (tags, caption) */
  metaV = $state(0);
  /** View > Hidden Pictures: hidden photos and folders are shown (dimmed) */
  showHidden = $state(localStorage.getItem("showHidden") === "1");
  /** View > Timeline (Ctrl+5): every photo by month, across folders */
  timeline = $state(false);
  /** search (Ctrl+F) and its filters; `hits` holds the matching ids while one is active */
  query = $state({ text: "", starred: false, from: "", to: "" });
  hits = $state<Set<number> | null>(null);
  /** photos held in the Photo Tray (Hold, Ctrl+H) while browsing elsewhere */
  held = $state<Set<number>>(new Set());

  /** the folders in the folder list's order (whether or not an album is open); hidden
   *  folders only with View > Hidden Pictures */
  sortedFolders = $derived.by(() => {
    const key: Record<FolderSort, (f: Folder) => number | string> = {
      date: (f) => -(f.date ?? 0),
      changed: (f) => -f.changed,
      size: (f) => -f.bytes,
      name: (f) => f.name.toLowerCase(),
    };
    // the tree is always in name order inside each branch: sort by full path
    const k = this.folderView === "tree" ? (f: Folder) => f.path.toLowerCase() : key[this.folderSort];
    const out = this.allFolders.filter((f) => this.showHidden || !f.hidden).sort((a, b) => {
      const x = k(a), y = k(b);
      return x < y ? -1 : x > y ? 1 : a.name.localeCompare(b.name);
    });
    return this.folderReverse && this.folderView !== "tree" ? out.reverse() : out;
  });
  /** the timeline: one section per month, newest first (ids below -1; not folders on disk) */
  months = $derived.by((): Folder[] => {
    const by = new Map<number, PhotoRow[]>();
    for (const f of this.sortedFolders) for (const p of f.photos) {
      const d = new Date(p[4] * 1000);
      const k = d.getUTCFullYear() * 100 + d.getUTCMonth() + 1;
      if (!by.has(k)) by.set(k, []);
      by.get(k)!.push(p);
    }
    return [...by.entries()].sort((a, b) => b[0] - a[0]).map(([k, photos]) => {
      photos.sort((a, b) => a[4] - b[4]);
      const start = Date.UTC(Math.floor(k / 100), (k % 100) - 1, 1) / 1000;
      const name = new Date(start * 1000).toLocaleDateString(undefined, { month: "long", year: "numeric", timeZone: "UTC" });
      return { id: -(k + 10), name, path: "", date: start, first: start, description: "", sort: "", root: "", changed: 0, bytes: 0, hidden: false, photos };
    });
  });
  /** what the grid shows: the open album, the timeline, or the folders; then without hidden
   *  photos (unless shown) and narrowed to the search hits */
  folders = $derived.by(() => {
    const base = this.albumSection ? [this.albumSection] : this.timeline ? this.months : this.sortedFolders;
    const hits = this.hits;
    if (this.showHidden && !hits) return base;
    const out: Folder[] = [];
    for (const f of base) {
      const photos = f.photos.filter((p) => (this.showHidden || !p[3]) && (!hits || hits.has(p[0])));
      if (photos.length === f.photos.length) out.push(f);
      else if (photos.length) out.push({ ...f, photos });
    }
    return out;
  });
  /** the grid shows sections that are not folders on disk (an album or the timeline) */
  virtual = $derived(!!this.album || this.timeline);
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
    this.albums = await api.albums();
    if (this.album) {
      const token = this.album.token;
      if (this.albums.some((a) => a.token === token)) this.album = { token, photos: await api.albumPhotos(token) };
      else this.album = null;
    }
    for (const p of this.album?.photos ?? []) if (this.thumbV[p[0]] == null) this.thumbV[p[0]] = p[2];
    this.metaV++;
    const shown = new Set(this.folders.flatMap((f) => f.photos.map((p) => p[0])));
    for (const id of shown) known.add(id);
    this.selection = new Set([...this.selection].filter((id) => known.has(id)));
    if ([...this.held].some((id) => !known.has(id))) this.held = new Set([...this.held].filter((id) => known.has(id)));
    if (this.hits) this.runSearch();
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
    this.held = new Set();
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
    const ids = this.order.filter((id) => this.selection.has(id) || this.held.has(id));
    // held photos from folders not shown now come first, as they were picked first
    const shown = new Set(ids);
    const away = [...this.held].filter((id) => !shown.has(id));
    const all = away.concat(ids);
    return all.length ? all : this.focus != null ? [this.focus] : [];
  }

  /** Hold (Ctrl+H): keeps the selection in the tray while you browse; again releases it */
  toggleHold() {
    const sel = [...this.selection];
    if (sel.length && !sel.every((id) => this.held.has(id))) this.held = new Set([...this.held, ...sel]);
    else this.held = new Set();
  }

  // ---- search, timeline, hidden ----
  private searchTimer: ReturnType<typeof setTimeout> | undefined;
  private searchTicket = 0;
  /** runs the search a moment after the last key, as Picasa filters while you type */
  search(change: Partial<{ text: string; starred: boolean; from: string; to: string }> = {}) {
    this.query = { ...this.query, ...change };
    clearTimeout(this.searchTimer);
    this.searchTimer = setTimeout(() => this.runSearch(), 150);
  }
  async runSearch() {
    const q = this.query;
    const day = (s: string) => (s ? Date.parse(`${s}T00:00:00Z`) / 1000 : null);
    const active = q.text.trim() || q.starred || q.from || q.to;
    const t = ++this.searchTicket;
    if (!active) {
      this.hits = null;
      return;
    }
    try {
      const ids = await api.search({ text: q.text, starred: q.starred, from: day(q.from), to: q.to ? day(q.to)! + 86400 : null });
      if (t === this.searchTicket) this.hits = new Set(ids);
    } catch (e) {
      this.message = String(e);
    }
  }
  clearSearch() {
    this.query = { text: "", starred: false, from: "", to: "" };
    this.hits = null;
  }
  toggleTimeline() {
    this.timeline = !this.timeline;
    if (this.timeline) this.album = null;
  }
  toggleShowHidden() {
    this.showHidden = !this.showHidden;
    localStorage.setItem("showHidden", this.showHidden ? "1" : "0");
  }
  hiddenIds = $derived(new Set(this.folders.flatMap((f) => f.photos.filter((p) => p[3]).map((p) => p[0]))));
  async hidePicked(hide: boolean, ids = this.picked()) {
    if (!ids.length) return;
    await this.guard(async () => {
      const n = await api.setHidden(ids, hide);
      this.flash(hide ? `${this.plural(n)} hidden${this.showHidden ? "" : " (View > Hidden Pictures shows them)"}` : `${this.plural(n)} shown again`);
    });
  }
  async hideFolder(f: Folder, hide: boolean) {
    await this.guard(async () => {
      await api.setFolderHidden(f.path, hide);
      this.flash(hide ? `Folder ${f.name} hidden${this.showHidden ? "" : " (View > Hidden Pictures shows it)"}` : `Folder ${f.name} shown again`);
    });
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

  // ---- albums ----
  async openAlbum(token: string) {
    this.timeline = false;
    await this.guard(async () => {
      this.album = { token, photos: await api.albumPhotos(token) };
      for (const p of this.album.photos) if (this.thumbV[p[0]] == null) this.thumbV[p[0]] = p[2];
      this.activeFolder = -1;
      this.selection = new Set();
      this.focus = null;
    });
  }
  closeAlbum() {
    if (!this.album) return;
    this.album = null;
    this.selection = new Set();
    this.focus = null;
  }
  /** the Add to Album chooser (album list + New Album), where it pops up and for which photos */
  albumPicker = $state<{ x: number; y: number; ids: number[] } | null>(null);
  /** the Album Properties dialog: a new album (holding `ids`) or an existing one */
  albumDialog = $state<{ album: Album; ids: number[]; isNew: boolean } | null>(null);
  newAlbum(ids = this.picked()) {
    this.albumDialog = { album: { token: "", name: "", date: new Date(Date.now() - new Date().getTimezoneOffset() * 60000).toISOString().slice(0, 10) + "T00:00:00+00:00", location: "", description: "" }, ids, isNew: true };
  }
  albumProperties(a: Album) {
    this.albumDialog = { album: { ...a }, ids: [], isNew: false };
  }
  async addToAlbum(token: string, ids = this.picked()) {
    if (!ids.length) return;
    await this.guard(async () => {
      const n = await api.albumAdd(token, ids);
      this.flash(`${this.plural(n)} added to ${this.albums.find((a) => a.token === token)?.name ?? "the album"}`);
    });
  }
  async removeFromAlbum(ids = this.picked()) {
    const a = this.album;
    if (!a || !ids.length) return;
    await this.guard(async () => {
      const n = await api.albumRemove(a.token, ids);
      this.flash(`${this.plural(n)} taken out of the album (the files are not touched)`);
    });
  }
  deleteAlbum(a: AlbumRow) {
    this.confirm = {
      title: "Delete Album",
      text: `The album "${a.name}" will be deleted. Its ${this.plural(a.count)} stay where they are, in their folders.`,
      ok: "Delete Album",
      run: async () => {
        await api.albumDelete(a.token);
        if (this.album?.token === a.token) this.closeAlbum();
        this.flash(`Album ${a.name} deleted`);
      },
    };
  }

  // ---- tags ----
  showTags = $state(localStorage.getItem("showTags") === "1");
  toggleTags() {
    this.showTags = !this.showTags;
    localStorage.setItem("showTags", this.showTags ? "1" : "0");
  }
  async changeTags(add: string[], remove: string[], ids = this.picked()) {
    if (!ids.length) return;
    await this.guard(async () => {
      this.flash(`Writing tags into ${this.plural(ids.length)}…`, 0);
      const n = await api.setTags(ids, add, remove);
      this.flash(n ? `Tags written into ${this.plural(n)}` : "No change");
    });
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
