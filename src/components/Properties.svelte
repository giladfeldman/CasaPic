<script lang="ts">
  import { store } from "../lib/store.svelte";
  import { api, fmtDate, fmtSize, type Photo } from "../lib/api";

  let p = $state<Photo | null>(null);

  $effect(() => {
    const id = store.focus;
    void store.thumbV[id ?? -1]; // refresh after an edit
    if (id == null) p = null;
    else api.photo(id).then((x) => (p = x), () => (p = null));
  });

  const rows = $derived(
    p
      ? ([
          ["Location", p.path],
          ["File Size", fmtSize(p.size)],
          ["Dimensions", `${p.width} x ${p.height} pixels`],
          ["Camera", p.camera ?? "—"],
          ["Camera Date", p.taken != null ? fmtDate(p.taken) : "—"],
          ["File Date", new Date(p.mtime * 1000).toLocaleString()],
          ["Starred", p.star ? "Yes" : "No"],
          ["Edits", p.edits.length ? p.edits.join(", ") : "None"],
        ] as [string, string][])
      : [],
  );
</script>

<aside class="props">
  <header>Properties <button class="x" title="Close" onclick={() => store.toggleProperties()}>✕</button></header>
  {#if p}
    <p class="of">Properties of {p.name}:</p>
    {#each rows as [k, v]}
      <div class="row"><b>{k}</b><span title={v}>{v}</span></div>
    {/each}
  {:else}
    <p class="of">Select a picture.</p>
  {/if}
</aside>

<style>
  .props { width: 276px; flex: none; background: #f3f3f3; border-left: 1px solid var(--chrome-line); overflow-y: auto; }
  header { background: linear-gradient(#9a9a9a, #7c7c7c); color: #fff; font-weight: 600; font-size: 14px; text-align: center; padding: 5px; position: relative; }
  .x { position: absolute; right: 6px; top: 3px; padding: 0 6px; font-size: 11px; }
  .of { padding: 8px 8px 12px; margin: 0; font-weight: 600; border-bottom: 1px solid #ddd; }
  .row { display: flex; flex-direction: column; padding: 4px 8px; border-bottom: 1px solid #ddd; }
  .row b { font-size: 12px; }
  .row span { color: #555; padding-left: 8px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; user-select: text; }
</style>
