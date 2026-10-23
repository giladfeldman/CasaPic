<script lang="ts">
  // The library grid: every folder's photos under a folder header, one long scroll.
  // Only the rows on screen exist in the page, so 50,000 photos scroll as fast as 50.
  import { store } from "../lib/store.svelte";
  import { api, thumbUrl, fmtDay, type Folder } from "../lib/api";

  let { onopen }: { onopen: (id: number) => void } = $props();

  const PAD = 24, HEADER = 78, GAPX = 12, GAPY = 22, TAIL = 18, OVERSCAN = 600;

  let el: HTMLDivElement | undefined = $state();
  let width = $state(800);
  let height = $state(600);
  let scrollTop = $state(0);
  let menu = $state<{ x: number; y: number } | null>(null);

  const size = $derived(store.thumbSize);
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
    const out: { b: Block; cells: { id: number; star: number; x: number; y: number }[] }[] = [];
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
          cells.push({ id: p[0], star: p[1], x: PAD + c * (size + GAPX), y: b.top + HEADER + r * rowH });
        }
      }
      out.push({ b, cells });
    }
    return out;
  });

  function onScroll() {
    scrollTop = el!.scrollTop;
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

  function context(e: MouseEvent, id: number) {
    e.preventDefault();
    if (!store.selection.has(id)) store.select(id);
    menu = { x: e.clientX, y: e.clientY };
  }

  const menuItems: [string, string, () => unknown][] = [
    ["View and Edit", "Ctrl+3", () => onopen(store.picked()[0])],
    ["Rotate Clockwise", "Ctrl+R", () => store.rotate(1)],
    ["Rotate Counterclockwise", "Ctrl+Shift+R", () => store.rotate(-1)],
    ["Star / Unstar", "", () => store.toggleStar()],
    ["Export Picture to Folder…", "Ctrl+Shift+S", () => (store.exportFor = store.picked())],
    ["Locate on Disk", "Ctrl+Enter", () => api.locate(store.picked()[0])],
    ["Properties", "Alt+Enter", () => { if (!store.showProperties) store.toggleProperties(); }],
  ];
</script>

<svelte:window onclick={() => (menu = null)} />

<div class="grid" bind:this={el} bind:clientWidth={width} bind:clientHeight={height} onscroll={onScroll}>
  <div class="space" style:height="{total}px">
    {#each visible as { b, cells } (b.folder.id)}
      <div class="head" style:top="{b.top}px">
        <svg viewBox="0 0 16 13" width="34" height="28" aria-hidden="true"
          ><path d="M1 2h5l1.5 1.5H15v8.5H1z" fill="#f1c35c" stroke="#c8962e" stroke-width="0.6" /></svg
        >
        <div>
          <div class="title georgia">{b.folder.name}</div>
          <div class="date">{fmtDay(b.folder.first ?? b.folder.date)} · {b.folder.photos.length} pictures</div>
        </div>
      </div>
      {#each cells as c (c.id)}
        <div
          class="cell"
          class:sel={store.selection.has(c.id)}
          style:left="{c.x}px"
          style:top="{c.y}px"
          style:width="{size}px"
          style:height="{size}px"
          role="button"
          tabindex="-1"
          onclick={(e) => click(e, c.id)}
          ondblclick={() => onopen(c.id)}
          oncontextmenu={(e) => context(e, c.id)}
          onkeydown={() => {}}
        >
          <span class="frame">
            {#if (store.thumbV[c.id] ?? 0) > 0}
              <img src={thumbUrl(c.id, store.thumbV[c.id])} alt="" draggable="false" loading="lazy" decoding="async"
                style:max-width="{size}px" style:max-height="{size}px" />
            {:else}
              <span class="ph" style:width="{size * 0.75}px" style:height="{size * 0.56}px"></span>
            {/if}
            {#if c.star}<span class="star" title="Starred">★</span>{/if}
          </span>
        </div>
      {/each}
    {/each}
  </div>
</div>

{#if menu}
  <div class="ctx" style:left="{menu.x}px" style:top="{menu.y}px" role="menu">
    {#each menuItems as [label, keys, run]}
      <button role="menuitem" onclick={() => { menu = null; run(); }}><span>{label}</span><span class="k">{keys}</span></button>
    {/each}
  </div>
{/if}

<style>
  .grid { flex: 1; overflow-y: auto; background: #fff; position: relative; }
  .space { position: relative; }
  .head { position: absolute; left: 20px; right: 20px; height: 64px; display: flex; gap: 10px; align-items: flex-start; padding-top: 8px; }
  .title { font-size: 20px; color: var(--title); line-height: 1.1; }
  .date { color: #555; margin-top: 3px; }
  .cell { position: absolute; display: flex; align-items: center; justify-content: center; outline: none; }
  .frame { position: relative; display: block; line-height: 0; }
  .cell img { display: block; box-shadow: 1px 2px 5px #0006; }
  .cell.sel img, .cell.sel .ph { outline: 3px solid var(--thumb-sel); box-shadow: inset 0 0 0 1px #fff, 1px 2px 5px #0006; outline-offset: 1px; }
  .ph { display: block; background: linear-gradient(135deg, #e9e9e9, #d6d6d6); }
  .star { position: absolute; left: 3px; bottom: 1px; color: #f6c200; font-size: 18px; text-shadow: 0 0 2px #000, 0 0 1px #000; }
  .ctx { position: fixed; z-index: 45; background: #fff; border: 1px solid #999; box-shadow: 2px 3px 10px #0003; padding: 3px 0; min-width: 260px; }
  .ctx button { display: flex; justify-content: space-between; width: 100%; border: none; border-radius: 0; background: none; font-weight: 400; font-size: 13px; padding: 4px 12px; }
  .ctx button:hover { background: var(--list-sel) !important; color: #fff; }
  .k { color: #888; }
</style>
