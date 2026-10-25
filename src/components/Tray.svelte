<script lang="ts">
  // Bottom of the window: the blue status line and the Photo Tray (selection, star,
  // rotate, export, thumbnail size), laid out like Picasa's.
  import { store } from "../lib/store.svelte";
  import { api, thumbUrl, fmtDate, fmtSize, type Photo } from "../lib/api";

  let { onexport }: { onexport: () => void } = $props();

  let p = $state<Photo | null>(null);
  $effect(() => {
    const id = store.focus;
    void store.thumbV[id ?? -1]; // refresh after an edit (a rotate swaps width and height)
    if (id == null) p = null;
    else api.photo(id).then((x) => (p = x), () => (p = null));
  });

  // the tray shows the selection and the held photos (Hold keeps them while you browse)
  const picked = $derived.by(() => {
    void store.selection;
    void store.held;
    return store.selection.size || store.held.size ? store.picked() : [];
  });
  const position = $derived.by(() => {
    if (store.focus == null) return "";
    const f = store.folderOf.get(store.focus);
    if (!f) return "";
    return `(${f.photos.findIndex((x) => x[0] === store.focus) + 1} of ${f.photos.length})`;
  });
  const allStarred = $derived(picked.length > 0 && picked.every((id) => store.starred.has(id)));
  const n = (k: number, one: string) => `${k} ${one}${k === 1 ? "" : "s"}`;
</script>

<div class="status">
  {#if store.status}
    <span>{store.status}</span>
  {:else if p}
    <span>{p.folder} › {p.name}</span><span>{fmtDate(p.taken ?? p.mtime)}</span><span>{p.width}x{p.height} pixels</span><span
      >{fmtSize(p.size)}</span
    ><span>{position}</span>
  {:else}
    <span>{store.album ? `${n(store.order.length, "picture")} in this album` : `${n(store.order.length, "picture")} in ${n(store.folders.length, store.timeline ? "month" : "folder")}`}</span>
  {/if}
</div>
<div class="tray">
  <div class="sel">
    <div class="thumbs">
      {#each picked.slice(0, 30) as id (id)}
        {#if (store.thumbV[id] ?? 0) > 0}<span class="t"><img src={thumbUrl(id, store.thumbV[id])} alt="" />{#if store.held.has(id)}<span class="pin" title="Held">📌</span>{/if}</span>{/if}
      {/each}
    </div>
    <span class="lbl">{picked.length ? store.held.size ? `${n(picked.length, "photo")} in the tray, ${store.held.size} held` : `${picked.length} selected` : "Selection"}</span>
    <button class="hold" class:on={store.held.size > 0} title="Hold: keep these photos in the tray while you look elsewhere (Ctrl+H)" disabled={!picked.length}
      onclick={() => store.toggleHold()}>📌</button>
    <button class="clear" title="Clear selection (Ctrl+D)" disabled={!picked.length} onclick={() => store.clearSelection()}>✕</button>
  </div>
  <div class="tools">
    <button title={allStarred ? "Remove star" : "Star"} disabled={!picked.length} onclick={() => store.toggleStar(picked)}
      ><span class="ico" class:on={allStarred}>★</span></button
    >
    <button title="Rotate counterclockwise (Ctrl+Shift+R)" disabled={!picked.length} onclick={() => store.rotate(-1, picked)}><span class="ico">↺</span></button>
    <button title="Rotate clockwise (Ctrl+R)" disabled={!picked.length} onclick={() => store.rotate(1, picked)}><span class="ico">↻</span></button>
    <button class="big" title="Add the selected pictures to an album" disabled={!picked.length}
      onclick={(e) => { e.stopPropagation(); const r = e.currentTarget.getBoundingClientRect(); store.albumPicker = { x: r.left, y: r.top - 8, ids: picked }; }}
      >Add to Album ▴</button>
    <button class="big" title="Print (Ctrl+P)" disabled={!picked.length} onclick={() => store.print()}>Print</button>
    <button class="big" title="Export Picture to Folder (Ctrl+Shift+S)" disabled={!picked.length} onclick={onexport}>Export</button>
  </div>
  {#if store.view === "library"}
    <label class="size" title="Thumbnail size">
      <span>▫</span>
      <input type="range" min="64" max={store.thumbMax} step="4" value={Math.min(store.thumbSize, store.thumbMax)} oninput={(e) => store.setThumbSize(+e.currentTarget.value)} />
      <span class="bigger">▢</span>
    </label>
  {/if}
</div>

<style>
  .status { height: 20px; background: var(--status); color: #fff; font-size: 12px; font-weight: 600; display: flex; gap: 22px; justify-content: center; align-items: center; white-space: nowrap; overflow: hidden; }
  .tray { height: 84px; display: flex; align-items: center; gap: 14px; padding: 6px 12px; background: var(--chrome); border-top: 1px solid #fff; }
  .sel { width: 520px; height: 72px; display: flex; align-items: center; gap: 8px; background: var(--surface); border: 1px solid var(--border-mid); padding: 4px; }
  .thumbs { flex: 1; display: flex; gap: 4px; overflow: hidden; height: 60px; align-items: center; }
  .thumbs img { height: 56px; box-shadow: 1px 1px 3px #0005; }
  .lbl { color: var(--muted); white-space: nowrap; }
  .clear { padding: 1px 6px; color: var(--error); }
  .hold { padding: 1px 5px; filter: grayscale(1); opacity: 0.7; }
  .hold.on { filter: none; opacity: 1; background: var(--note-bg); }
  .t { position: relative; display: block; line-height: 0; }
  .pin { position: absolute; right: -2px; top: -4px; font-size: 12px; line-height: 1; }
  .tools { display: flex; gap: 6px; align-items: center; }
  .ico { font-size: 17px; line-height: 1; color: var(--link); }
  .ico.on { color: #f0b400; }
  .big { height: 40px; padding: 0 18px; }
  .size { margin-left: auto; display: flex; align-items: center; gap: 6px; color: var(--muted); }
  .size input { width: 140px; }
  .bigger { font-size: 20px; }
</style>
