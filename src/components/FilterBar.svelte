<script lang="ts">
  // Above the grid, as Picasa's search box and filter buttons: search as you type (Ctrl+F)
  // across file names, folders, tags, captions and album names; starred only; a date range.
  import { store } from "../lib/store.svelte";

  let input: HTMLInputElement | undefined = $state();
  export function focus() {
    input?.focus();
    input?.select();
  }
  const active = $derived(!!store.hits);
  const shown = $derived(store.order.length);
</script>

<div class="bar">
  {#if store.timeline}<span class="mode">Timeline</span>{/if}
  <button class="star" class:on={store.query.starred} title="Show only starred photos" onclick={() => store.search({ starred: !store.query.starred })}>★</button>
  <label class="dates" title="Show only photos taken in these days">
    From <input type="date" value={store.query.from} onchange={(e) => store.search({ from: e.currentTarget.value })} />
    to <input type="date" value={store.query.to} onchange={(e) => store.search({ to: e.currentTarget.value })} />
  </label>
  {#if active}<span class="count">{shown} picture{shown === 1 ? "" : "s"} match</span>{/if}
  <div class="search">
    <span class="glass" aria-hidden="true">⌕</span>
    <input bind:this={input} value={store.query.text} placeholder="Search: names, folders, tags, captions, albums"
      oninput={(e) => store.search({ text: e.currentTarget.value })}
      onkeydown={(e) => { e.stopPropagation(); if (e.key === "Escape") { store.clearSearch(); input?.blur(); } }} />
    {#if active}<button class="clear" title="Clear the search and filters (Esc)" onclick={() => store.clearSearch()}>✕</button>{/if}
  </div>
</div>

<style>
  .bar { height: 34px; flex: none; display: flex; align-items: center; gap: 12px; padding: 0 12px; background: var(--chrome); border-bottom: 1px solid var(--chrome-line); }
  .mode { font-weight: 600; color: var(--link); }
  .star { padding: 0 8px; height: 24px; color: #889; font-size: 15px; }
  .star.on { color: #f0b400; background: #fff7d6; }
  .dates { display: flex; align-items: center; gap: 4px; color: var(--muted); font-size: 12px; }
  .dates input { font-size: 12px; padding: 1px 3px; }
  .count { color: var(--link); font-weight: 600; font-size: 12px; }
  .search { margin-left: auto; display: flex; align-items: center; background: var(--surface); border: 1px solid var(--border); border-radius: 3px; padding: 0 4px; width: 340px; }
  .glass { color: var(--muted); font-size: 15px; }
  .search input { border: none; outline: none; flex: 1; min-width: 0; padding: 3px 4px; font-size: 13px; }
  .clear { border: none; background: none; color: var(--error); padding: 0 4px; font-size: 11px; }
</style>
