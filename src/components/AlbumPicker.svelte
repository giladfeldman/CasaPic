<script lang="ts">
  // Add to Album: the list Picasa's tray album button shows — New Album, then each album.
  import { store } from "../lib/store.svelte";

  const at = $derived(store.albumPicker!);
  // from the tray (lower half of the window) the list opens upwards, above the button
  const up = $derived(at.y > window.innerHeight / 2);

  function pick(token: string | null) {
    const ids = at.ids;
    store.albumPicker = null;
    if (token) store.addToAlbum(token, ids);
    else store.newAlbum(ids);
  }
</script>

<svelte:window onclick={() => (store.albumPicker = null)} onkeydown={(e) => e.key === "Escape" && (store.albumPicker = null)} />

<div class="menu" style:left="{at.x}px" style:top={up ? null : `${at.y}px`} style:bottom={up ? `${window.innerHeight - at.y}px` : null} role="menu" tabindex="-1" onclick={(e) => e.stopPropagation()} onkeydown={() => {}}>
  <div class="sub">Add {at.ids.length} photo{at.ids.length === 1 ? "" : "s"} to</div>
  <button role="menuitem" onclick={() => pick(null)}>New Album…</button>
  {#if store.albums.length}<hr />{/if}
  {#each store.albums as a (a.token)}
    <button role="menuitem" onclick={() => pick(a.token)}>{a.name} ({a.count})</button>
  {/each}
</div>

<style>
  .menu { position: fixed; z-index: 46; background: var(--surface); border: 1px solid var(--border); box-shadow: 2px 3px 10px #0003; padding: 3px 0; min-width: 200px; max-height: 70vh; overflow-y: auto; display: flex; flex-direction: column; }
  .menu button { border: none; border-radius: 0; background: none; text-align: left; padding: 4px 14px; font-weight: 400; font-size: 13px; }
  .menu button:hover { background: var(--list-sel) !important; color: #fff; }
  .sub { color: var(--muted); font-size: 12px; padding: 3px 14px 1px; }
  hr { border: none; border-top: 1px solid var(--border-soft); margin: 3px 0; }
</style>
