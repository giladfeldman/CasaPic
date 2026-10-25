<script lang="ts" module>
  export interface Item {
    label: string;
    keys?: string;
    run: () => unknown;
    enabled?: boolean;
    checked?: boolean;
  }
  export interface Menu {
    label: string;
    items: (Item | "-")[];
  }
</script>

<script lang="ts">
  let { menus }: { menus: Menu[] } = $props();
  let open = $state<number | null>(null);

  function pick(it: Item) {
    open = null;
    if (it.enabled !== false) it.run();
  }
</script>

<svelte:window onclick={() => (open = null)} />

<nav class="bar">
  {#each menus as m, i}
    <div class="top">
      <button
        class="title"
        class:open={open === i}
        onclick={(e) => { e.stopPropagation(); open = open === i ? null : i; }}
        onmouseenter={() => { if (open != null) open = i; }}>{m.label}</button
      >
      {#if open === i}
        <div class="drop" role="menu">
          {#each m.items as it}
            {#if it === "-"}
              <hr />
            {:else}
              <button class="item" role="menuitem" disabled={it.enabled === false} onclick={(e) => { e.stopPropagation(); pick(it); }}>
                <span class="check">{it.checked ? "✓" : ""}</span>
                <span class="lbl">{it.label}</span>
                <span class="keys">{it.keys ?? ""}</span>
              </button>
            {/if}
          {/each}
        </div>
      {/if}
    </div>
  {/each}
</nav>

<style>
  .bar { display: flex; height: 24px; background: var(--surface); border-bottom: 1px solid var(--chrome-line); padding-left: 4px; }
  .top { position: relative; }
  .title { background: none; border: none; border-radius: 0; font-weight: 400; font-size: 13px; height: 24px; padding: 0 9px; }
  .title:hover:not(:disabled), .title.open { background: var(--hilite) !important; }
  .drop { position: absolute; top: 24px; left: 0; z-index: 40; min-width: 250px; background: var(--surface); border: 1px solid var(--border); box-shadow: 2px 3px 10px #0003; padding: 3px 0; }
  .item { display: flex; width: 100%; border: none; border-radius: 0; background: none; font-weight: 400; font-size: 13px; padding: 4px 12px 4px 4px; text-align: left; }
  .item:hover:not(:disabled) { background: var(--list-sel) !important; color: #fff; }
  .check { width: 20px; text-align: center; }
  .lbl { flex: 1; }
  .keys { color: var(--muted); padding-left: 24px; }
  .item:hover:not(:disabled) .keys { color: #dde; }
  hr { border: none; border-top: 1px solid var(--border-soft); margin: 3px 0; }
</style>
