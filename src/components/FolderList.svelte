<script lang="ts">
  import { store } from "../lib/store.svelte";
  import type { Folder } from "../lib/api";

  let { onpick }: { onpick: (folderId: number) => void } = $props();

  // Picasa groups the folder list under year headings.
  const groups = $derived.by(() => {
    const out: { year: string; folders: Folder[] }[] = [];
    for (const f of store.folders) {
      const year = f.date == null ? "Undated" : String(new Date(f.date * 1000).getUTCFullYear());
      if (out.at(-1)?.year !== year) out.push({ year, folders: [] });
      out.at(-1)!.folders.push(f);
    }
    return out;
  });
</script>

<aside class="list">
  <div class="cat">▾ Folders ({store.folders.length})</div>
  {#each groups as g}
    <div class="year">{g.year}</div>
    {#each g.folders as f (f.id)}
      <button class="row" class:sel={store.activeFolder === f.id} title={f.path} onclick={() => { store.activeFolder = f.id; onpick(f.id); }}>
        <svg viewBox="0 0 16 13" width="16" height="13" aria-hidden="true"
          ><path d="M1 2h5l1.5 1.5H15v8.5H1z" fill="#f1c35c" stroke="#c8962e" /></svg
        >
        <span>{f.name} ({f.photos.length})</span>
      </button>
    {/each}
  {/each}
  {#if store.folders.length === 0}
    <p class="empty">{store.status || "No photos found yet."}</p>
  {/if}
</aside>

<style>
  .list { width: 236px; flex: none; background: var(--list-bg); border-right: 1px solid var(--chrome-line); overflow-y: auto; padding-bottom: 12px; }
  .cat { background: var(--cat-bg); border-bottom: 1px solid #d8d4ca; font-weight: 600; padding: 3px 8px; position: sticky; top: 0; }
  .year { color: var(--muted); border-bottom: 1px solid #ddd; margin: 8px 8px 2px; padding-bottom: 1px; }
  .row { display: flex; gap: 6px; align-items: center; width: 100%; height: 22px; padding: 0 0 0 17px; border: none; border-radius: 0; background: none; font-weight: 400; font-size: 13px; color: #444; text-align: left; white-space: nowrap; overflow: hidden; }
  .row:hover:not(.sel) { background: var(--list-hover) !important; }
  .row.sel { background: var(--list-sel) !important; color: #fff; }
  .row span { overflow: hidden; text-overflow: ellipsis; }
  .empty { color: var(--muted); padding: 8px 12px; }
</style>
