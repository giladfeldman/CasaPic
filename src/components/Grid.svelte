<script lang="ts">
  // The library grid: every folder's photos under a folder header, one long scroll.
  // Only the rows on screen exist in the page, so 50,000 photos scroll as fast as 50.
  import { untrack } from "svelte";
  import { store } from "../lib/store.svelte";
  import { api, thumbUrl, renderUrl, fmtDay, type Folder } from "../lib/api";

  let { onopen }: { onopen: (id: number) => void } = $props();

  const PAD = 24, HEADER = 96, GAPX = 12, GAPY = 22, TAIL = 18, OVERSCAN = 600;

  let el: HTMLDivElement | undefined = $state();
  let width = $state(800);
  let height = $state(600);
  let scrollTop = $state(0);
  let menu = $state<{ x: number; y: number } | null>(null);
  let lastMenu = { x: 0, y: 0 };
  /** the folder whose description is being typed, and the text so far */
  let editing = $state<{ id: number; text: string } | null>(null);

  // Folder > Edit Description (from the folder list) opens the box on that folder's header.
  $effect(() => {
    const id = store.editDescription;
    if (id == null) return;
    store.editDescription = null;
    const f = store.folders.find((f) => f.id === id);
    if (!f) return;
    scrollToFolder(id);
    editing = { id, text: f.description };
  });

  async function saveDescription(f: Folder) {
    if (!editing || editing.id !== f.id) return;
    const text = editing.text.trim();
    editing = null;
    if (text === f.description) return;
    try {
      // an open album's description is the album's own (Album Properties)
      const a = f.id === -1 ? store.albums.find((x) => x.token === store.album?.token) : undefined;
      if (a) await api.albumEdit({ token: a.token, name: a.name, date: a.date, location: a.location, description: text });
      else await api.setFolderDescription(f.path, text);
    } catch (e) {
      store.message = String(e);
    }
  }
  function focusEnd(node: HTMLInputElement) {
    node.focus();
    node.select();
  }

  // the largest thumbnail: one photo across the grid, and no taller than the grid
  $effect(() => {
    store.thumbMax = Math.max(256, Math.floor(Math.min(width - 2 * PAD, height - 40) / 4) * 4);
  });
  const size = $derived(Math.min(store.thumbSize, store.thumbMax));
  /** thumbnails are kept at 256 pixels; larger tiles get a sharp picture (512/1024/2048) */
  const sharpEdge = $derived.by(() => {
    const px = size * devicePixelRatio;
    return px <= 300 ? 0 : px <= 512 ? 512 : px <= 1024 ? 1024 : 2048;
  });
  /** swaps in the sharp picture once the photo has stayed on screen a moment (fast
   *  scrolling asks for nothing) and the picture has arrived */
  function sharp(node: HTMLImageElement, p: { id: number; v: number; edge: number }) {
    let timer: ReturnType<typeof setTimeout> | undefined;
    let want = "";
    const go = () => {
      clearTimeout(timer);
      if (!p.edge) return;
      const url = renderUrl(p.id, p.edge, p.v);
      want = url;
      timer = setTimeout(() => {
        const im = new Image();
        im.src = url;
        im.decode().then(() => { if (want === url) node.src = url; }, () => {});
      }, 250);
    };
    go();
    return {
      update(n: { id: number; v: number; edge: number }) {
        if (n.id === p.id && n.v === p.v && n.edge === p.edge) return;
        p = n;
        node.src = thumbUrl(p.id, p.v);
        go();
      },
      destroy() { clearTimeout(timer); want = ""; },
    };
  }
  const cols = $derived(Math.max(1, Math.floor((width - 2 * PAD + GAPX) / (size + GAPX))));
  const rowH = $derived(size + GAPY);

  interface Block { folder: Folder; top: number; height: number }
  const blocks = $derived.by(() => {
    let y = 8;
    const out: Block[] = [];
    for (const f of store.folders) {
      const rows = Math.ceil(f.photos.length / cols);
      const h = HEADER + rows * rowH + TAIL;
      out.push({ folder: f, top: y, height: h });
      y += h;
    }
    return out;
  });
  const total = $derived((blocks.at(-1)?.top ?? 0) + (blocks.at(-1)?.height ?? 0) + PAD);

  const visible = $derived.by(() => {
    const lo = scrollTop - OVERSCAN, hi = scrollTop + height + OVERSCAN;
    const out: { b: Block; cells: { id: number; star: number; hidden: number; x: number; y: number }[] }[] = [];
    for (const b of blocks) {
      if (b.top + b.height < lo || b.top > hi) continue;
      const cells = [];
      const r0 = Math.max(0, Math.floor((lo - b.top - HEADER) / rowH));
      const r1 = Math.ceil((hi - b.top - HEADER) / rowH);
      for (let r = r0; r <= r1; r++) {
        for (let c = 0; c < cols; c++) {
          const i = r * cols + c;
          const p = b.folder.photos[i];
          if (!p) break;
          cells.push({ id: p[0], star: p[1], hidden: p[3], x: PAD + c * (size + GAPX), y: b.top + HEADER + r * rowH });
        }
      }
      out.push({ b, cells });
    }
    return out;
  });

  // ---- keeping the place (see store.gridPlace) ----
  /** which view the grid shows; each keeps its own place */
  const ctx = $derived(store.album ? "a:" + store.album.token : store.hits ? "s" : store.timeline ? "t" : "f");
  /** the photos have arrived (the first restore waits for them) */
  const ready = $derived(blocks.length > 0);
  /** the view whose place is on screen now (scroll events before a restore are not saved) */
  let shown = "";
  function anchorNow(): { id: number; off: number } | null {
    for (const b of blocks) {
      if (b.top + b.height <= scrollTop) continue;
      const r = Math.max(0, Math.min(Math.floor((scrollTop - b.top - HEADER) / rowH), Math.ceil(b.folder.photos.length / cols) - 1));
      const p = b.folder.photos[r * cols];
      if (p) return { id: p[0], off: b.top + HEADER + r * rowH - scrollTop };
    }
    return null;
  }
  function yOf(id: number): number | null {
    const f = store.folderOf.get(id);
    const b = blocks.find((b) => b.folder === f);
    if (!b || !f) return null;
    const i = f.photos.findIndex((p) => p[0] === id);
    return i < 0 ? null : b.top + HEADER + Math.floor(i / cols) * rowH;
  }
  // back to the saved place when the view changes, the grid comes back (after a photo was
  // open) or the thumbnails change size; then make sure the photo in focus is on screen
  $effect(() => {
    void [ctx, cols, size, el, ready];
    untrack(() => {
      if (!el || !blocks.length) return;
      const place = store.gridPlace[ctx];
      const y = place ? yOf(place.id) : null;
      if (y != null) el.scrollTop = y - place!.off;
      else if (shown !== ctx) el.scrollTop = 0;
      scrollTop = el.scrollTop;
      if (store.focus != null && store.folderOf.has(store.focus)) scrollIntoView(store.focus);
      shown = ctx;
    });
  });

  function onScroll() {
    scrollTop = el!.scrollTop;
    if (shown === ctx) {
      const a = anchorNow();
      if (a) store.saveGridPlace(ctx, a);
    }
    const b = blocks.find((b) => b.top + b.height > scrollTop + 40);
    if (b) store.activeFolder = b.folder.id;
  }

  export function scrollToFolder(id: number) {
    const b = blocks.find((b) => b.folder.id === id);
    if (b && el) el.scrollTop = b.top - 8;
  }

  function scrollIntoView(id: number) {
    const f = store.folderOf.get(id);
    const b = blocks.find((b) => b.folder === f);
    if (!b || !el || !f) return;
    const i = f.photos.findIndex((p) => p[0] === id);
    const y = b.top + HEADER + Math.floor(i / cols) * rowH;
    if (y < el.scrollTop) el.scrollTop = y - (i < cols ? HEADER : 8);
    else if (y + rowH > el.scrollTop + height) el.scrollTop = y + rowH - height + 8;
  }

  /** Arrow keys, Home and End, as in Picasa (Home/End = first/last in the folder). */
  export function move(key: string, extend: boolean) {
    const cur = store.focus ?? store.order[0];
    if (cur == null) return;
    const fi = store.folders.findIndex((f) => f === store.folderOf.get(cur));
    const f = store.folders[fi];
    const i = f.photos.findIndex((p) => p[0] === cur);
    let target: number | undefined;
    const at = (fo: Folder | undefined, j: number) => fo?.photos[Math.max(0, Math.min(j, fo.photos.length - 1))]?.[0];
    if (key === "arrowleft") target = store.order[store.order.indexOf(cur) - 1];
    else if (key === "arrowright") target = store.order[store.order.indexOf(cur) + 1];
    else if (key === "home") target = at(f, 0);
    else if (key === "end") target = at(f, f.photos.length - 1);
    else if (key === "arrowdown")
      target = i + cols < f.photos.length ? at(f, i + cols) : at(store.folders[fi + 1], i % cols);
    else if (key === "arrowup") {
      const prev = store.folders[fi - 1];
      target = i - cols >= 0 ? at(f, i - cols) : prev ? at(prev, Math.floor((prev.photos.length - 1) / cols) * cols + (i % cols)) : undefined;
    }
    if (target == null) return;
    store.select(target, extend ? "range" : "only");
    scrollIntoView(target);
  }

  function click(e: MouseEvent, id: number) {
    store.select(id, e.ctrlKey ? "toggle" : e.shiftKey ? "range" : "only");
  }

  /** dragging photos onto a folder in the folder list moves them (Ctrl: copies) */
  function dragStart(e: DragEvent, id: number) {
    if (!store.selection.has(id)) store.select(id);
    const ids = store.picked();
    e.dataTransfer?.setData("application/x-casapic-photos", JSON.stringify(ids));
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "copyMove";
  }

  function context(e: MouseEvent, id: number) {
    e.preventDefault();
    if (!store.selection.has(id)) store.select(id);
    menu = { x: e.clientX, y: e.clientY };
  }

  const menuItems: [string, string, () => unknown][] = $derived([
    ["View and Edit", "Ctrl+3", () => onopen(store.picked()[0])],
    ["Rotate Clockwise", "Ctrl+R", () => store.rotate(1)],
    ["Rotate Counterclockwise", "Ctrl+Shift+R", () => store.rotate(-1)],
    ["Star / Unstar", "", () => store.toggleStar()],
    ["Rename…", "F2", () => store.renamePicked()],
    ["Delete from Disk", "Del", () => store.deletePicked()],
    ["Cut", "Ctrl+X", () => store.cutOrCopy(true)],
    ["Copy", "Ctrl+C", () => store.cutOrCopy(false)],
    ...(store.album
      ? ([["Remove from Album", "", () => store.removeFromAlbum()]] as [string, string, () => unknown][])
      : store.timeline
      ? []
      : ([
          ["Move to New Folder…", "", () => store.moveToNewFolder()],
          ["Split Folder Here…", "", () => store.splitFolderHere()],
        ] as [string, string, () => unknown][])),
    ["Add to Album…", "", () => (store.albumPicker = { x: lastMenu.x, y: lastMenu.y, ids: store.picked() })],
    ["Tags", "Ctrl+T", () => { if (!store.showTags) store.toggleTags(); }],
    store.picked().every((id) => store.hiddenIds.has(id)) ? ["Unhide", "", () => store.hidePicked(false)] : ["Hide", "", () => store.hidePicked(true)],
    ["Hold in the Tray", "Ctrl+H", () => store.toggleHold()],
    ["Save a Copy", "", () => store.saveCopy()],
    ["Adjust Date and Time…", "", () => (store.dateFor = store.picked())],
    ["Open in an Editor", "Ctrl+Shift+O", () => store.openWith("editor")],
    ["Export Picture to Folder…", "Ctrl+Shift+S", () => (store.exportFor = store.picked())],
    ["Locate on Disk", "Ctrl+Enter", () => api.locate(store.picked()[0])],
    ["Properties", "Alt+Enter", () => { if (!store.showProperties) store.toggleProperties(); }],
  ]);
</script>

<svelte:window onclick={() => (menu = null)} />

<div class="grid" bind:this={el} bind:clientWidth={width} bind:clientHeight={height} onscroll={onScroll}>
  <div class="space" style:height="{total}px">
    {#each visible as { b, cells } (b.folder.id)}
      <div class="head" style:top="{b.top}px">
        {#if b.folder.id < -1}
          <svg viewBox="0 0 16 13" width="34" height="28" aria-hidden="true"
            ><rect x="1.5" y="1.5" width="13" height="11" rx="1" fill="#fff" stroke="#3d6ea8" stroke-width="0.6" /><rect x="1.5" y="1.5" width="13" height="3" fill="#6f9fd8" /><path d="M4 7h2M7 7h2M10 7h2M4 9.5h2M7 9.5h2" stroke="#3d6ea8" stroke-width="0.8" /></svg
          >
        {:else if b.folder.id === -1}
          <svg viewBox="0 0 16 13" width="34" height="28" aria-hidden="true"
            ><rect x="1.5" y="1" width="13" height="11" rx="1" fill="#6f9fd8" stroke="#3d6ea8" stroke-width="0.6" /><rect x="3.5" y="3" width="9" height="7" fill="#e8f0fa" /></svg
          >
        {:else}
          <svg viewBox="0 0 16 13" width="34" height="28" aria-hidden="true"
            ><path d="M1 2h5l1.5 1.5H15v8.5H1z" fill="#f1c35c" stroke="#c8962e" stroke-width="0.6" /></svg
          >
        {/if}
        <div>
          <div class="title georgia">{b.folder.name}
            <button class="play" title="Play a slideshow of these photos (Ctrl+4)" onclick={() => store.startSlideshow(b.folder.photos.map((p) => p[0]))}>▶</button>
            <button class="play coll" title="Make a picture collage of these photos" onclick={() => store.collage(b.folder.photos.map((p) => p[0]))}>▦</button>
          </div>
          <div class="date">{fmtDay(b.folder.first ?? b.folder.date)} · {b.folder.photos.length} picture{b.folder.photos.length === 1 ? "" : "s"}</div>
          {#if b.folder.id < -1}
            <!-- a month of the timeline: no description -->
          {:else if editing?.id === b.folder.id}
            <input class="desc-in" bind:value={editing.text} use:focusEnd placeholder="Add a description"
              onblur={() => saveDescription(b.folder)}
              onkeydown={(e) => { e.stopPropagation(); if (e.key === "Enter") (e.target as HTMLInputElement).blur(); if (e.key === "Escape") editing = null; }} />
          {:else}
            <button class="desc" class:empty={!b.folder.description} title="Click to edit the folder's description"
              onclick={() => (editing = { id: b.folder.id, text: b.folder.description })}>{b.folder.description || "Add a description"}</button>
          {/if}
        </div>
      </div>
      {#each cells as c (c.id)}
        <div
          class="cell"
          class:sel={store.selection.has(c.id) || store.held.has(c.id)}
          class:hid={c.hidden}
          style:left="{c.x}px"
          style:top="{c.y}px"
          style:width="{size}px"
          style:height="{size}px"
          role="button"
          tabindex="-1"
          draggable="true"
          ondragstart={(e) => dragStart(e, c.id)}
          onclick={(e) => click(e, c.id)}
          ondblclick={() => onopen(c.id)}
          oncontextmenu={(e) => context(e, c.id)}
          onkeydown={() => {}}
        >
          <span class="frame">
            {#if (store.thumbV[c.id] ?? 0) > 0}
              <img src={thumbUrl(c.id, store.thumbV[c.id])} alt="" draggable="false" loading="lazy" decoding="async"
                use:sharp={{ id: c.id, v: store.thumbV[c.id], edge: sharpEdge }}
                style:max-width="{size}px" style:max-height="{size}px" />
            {:else}
              <span class="ph" style:width="{size * 0.75}px" style:height="{size * 0.56}px"></span>
            {/if}
            {#if c.star}<span class="star" title="Starred">★</span>{/if}
            {#if c.hidden}<span class="hidmark" title="Hidden (Picture > Unhide shows it always)">hidden</span>{/if}
          </span>
        </div>
      {/each}
    {/each}
  </div>
</div>

{#if menu}
  <div class="ctx" style:left="{menu.x}px" style:top="{menu.y}px" role="menu">
    {#each menuItems as [label, keys, run]}
      <button role="menuitem" onclick={() => { lastMenu = menu!; menu = null; run(); }}><span>{label}</span><span class="k">{keys}</span></button>
    {/each}
  </div>
{/if}

<style>
  .grid { flex: 1; overflow-y: auto; background: var(--surface); position: relative; }
  .space { position: relative; }
  .head { position: absolute; left: 20px; right: 20px; height: 84px; display: flex; gap: 10px; align-items: flex-start; padding-top: 8px; }
  .title { font-size: 20px; color: var(--title); line-height: 1.1; }
  .play.coll { margin-left: 4px; color: var(--link); }
  .play { margin-left: 10px; vertical-align: 3px; font-size: 11px; color: #2a8a2a; padding: 1px 7px; border-radius: 3px; }
  .date { color: var(--text-2); margin-top: 3px; }
  .head > div { flex: 1; min-width: 0; }
  .desc { display: block; border: none; background: none; padding: 1px 0; margin-top: 3px; font-weight: 400; font-size: 13px; color: var(--text); text-align: left; max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .desc.empty { color: var(--muted); font-style: italic; }
  .desc:hover { text-decoration: underline; }
  .desc-in { display: block; margin-top: 2px; width: min(560px, 100%); font-size: 13px; padding: 1px 4px; }
  .cell { position: absolute; display: flex; align-items: center; justify-content: center; outline: none; }
  .frame { position: relative; display: block; line-height: 0; }
  .cell img { display: block; box-shadow: 1px 2px 5px #0006; }
  .cell.sel img, .cell.sel .ph { outline: 3px solid var(--thumb-sel); box-shadow: inset 0 0 0 1px #fff, 1px 2px 5px #0006; outline-offset: 1px; }
  .cell.hid img { opacity: 0.45; }
  .hidmark { position: absolute; right: 3px; bottom: 3px; background: #0008; color: #fff; font-size: 10px; line-height: 1; padding: 2px 4px; border-radius: 2px; }
  .ph { display: block; background: linear-gradient(135deg, #e9e9e9, #d6d6d6); }
  .star { position: absolute; left: 3px; bottom: 1px; color: #f6c200; font-size: 18px; text-shadow: 0 0 2px #000, 0 0 1px #000; }
  .ctx { position: fixed; z-index: 45; background: var(--surface); border: 1px solid var(--border); box-shadow: 2px 3px 10px #0003; padding: 3px 0; min-width: 260px; }
  .ctx button { display: flex; justify-content: space-between; width: 100%; border: none; border-radius: 0; background: none; font-weight: 400; font-size: 13px; padding: 4px 12px; }
  .ctx button:hover { background: var(--list-sel) !important; color: #fff; }
  .k { color: var(--muted); }
</style>
