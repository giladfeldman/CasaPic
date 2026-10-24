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

  const picked = $derived(store.order.filter((id) => store.selection.has(id)));
  const position = $derived.by(() => {
    if (store.focus == null) return "";
    const f = store.folderOf.get(store.focus);
    if (!f) return "";
    return `(${f.photos.findIndex((x) => x[0] === store.focus) + 1} of ${f.photos.length})`;
  });
  const allStarred = $derived(picked.length > 0 && picked.every((id) => store.starred.has(id)));
</script>

<div class="status">
  {#if store.status}
    <span>{store.status}</span>
  {:else if p}
    <span>{p.folder} › {p.name}</span><span>{fmtDate(p.taken ?? p.mtime)}</span><span>{p.width}x{p.height} pixels</span><span
      >{fmtSize(p.size)}</span
    ><span>{position}</span>
  {:else}
    <span>{store.order.length} pictures in {store.folders.length} folders</span>
  {/if}
</div>
<div class="tray">
  <div class="sel">
    <div class="thumbs">
      {#each picked.slice(0, 30) as id (id)}
        {#if (store.thumbV[id] ?? 0) > 0}<img src={thumbUrl(id, store.thumbV[id])} alt="" />{/if}
      {/each}
    </div>
    <span class="lbl">{picked.length ? `${picked.length} selected` : "Selection"}</span>
    <button class="clear" title="Clear selection (Ctrl+D)" disabled={!picked.length} onclick={() => store.clearSelection()}>✕</button>
  </div>
  <div class="tools">
    <button title={allStarred ? "Remove star" : "Star"} disabled={!picked.length} onclick={() => store.toggleStar(picked)}
      ><span class="ico" class:on={allStarred}>★</span></button
    >
    <button title="Rotate counterclockwise (Ctrl+Shift+R)" disabled={!picked.length} onclick={() => store.rotate(-1, picked)}><span class="ico">↺</span></button>
    <button title="Rotate clockwise (Ctrl+R)" disabled={!picked.length} onclick={() => store.rotate(1, picked)}><span class="ico">↻</span></button>
    <button class="big" title="Export Picture to Folder (Ctrl+Shift+S)" disabled={!picked.length} onclick={onexport}>Export</button>
  </div>
  {#if store.view === "library"}
    <label class="size" title="Thumbnail size">
      <span>▫</span>
      <input type="range" min="64" max="256" step="4" value={store.thumbSize} oninput={(e) => store.setThumbSize(+e.currentTarget.value)} />
      <span class="bigger">▢</span>
    </label>
  {/if}
</div>

<style>
  .status { height: 20px; background: var(--status); color: #fff; font-size: 12px; font-weight: 600; display: flex; gap: 22px; justify-content: center; align-items: center; white-space: nowrap; overflow: hidden; }
  .tray { height: 84px; display: flex; align-items: center; gap: 14px; padding: 6px 12px; background: var(--chrome); border-top: 1px solid #fff; }
  .sel { width: 520px; height: 72px; display: flex; align-items: center; gap: 8px; background: #fff; border: 1px solid #ccc; padding: 4px; }
  .thumbs { flex: 1; display: flex; gap: 4px; overflow: hidden; height: 60px; align-items: center; }
  .thumbs img { height: 56px; box-shadow: 1px 1px 3px #0005; }
  .lbl { color: #999; white-space: nowrap; }
  .clear { padding: 1px 6px; color: #c00; }
  .tools { display: flex; gap: 6px; align-items: center; }
  .ico { font-size: 17px; line-height: 1; color: #456; }
  .ico.on { color: #f0b400; }
  .big { height: 40px; padding: 0 18px; }
  .size { margin-left: auto; display: flex; align-items: center; gap: 6px; color: #678; }
  .size input { width: 140px; }
  .bigger { font-size: 20px; }
</style>
