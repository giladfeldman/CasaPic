<script lang="ts">
  // The Tags panel (Ctrl+T), as Picasa's: type a tag and press Enter to add it to every
  // selected photo; the tags they already carry are listed with × to remove; the library's
  // most used tags are offered as quick choices. Tags are written into the photo files.
  import { store } from "../lib/store.svelte";
  import { api, type Photo } from "../lib/api";

  /** at most this many selected photos are read to list their tags (each is one call) */
  const READ_MAX = 400;

  let text = $state("");
  let photos = $state<Photo[]>([]);
  let library = $state<[string, number][]>([]);
  const ids = $derived(store.picked());

  $effect(() => {
    void store.metaV;
    const want = ids.slice(0, READ_MAX);
    Promise.all(want.map((id) => api.photo(id).catch(() => null))).then((ps) => (photos = ps.filter((p): p is Photo => !!p)));
    api.allTags().then((t) => (library = t), () => (library = []));
  });

  /** tags on the selected photos: [tag, how many of them carry it] */
  const onSelection = $derived.by(() => {
    const n = new Map<string, [string, number]>();
    for (const p of photos) for (const t of p.tags) {
      const k = t.toLowerCase();
      n.set(k, [n.get(k)?.[0] ?? t, (n.get(k)?.[1] ?? 0) + 1]);
    }
    return [...n.values()].sort((a, b) => a[0].localeCompare(b[0]));
  });
  const suggestions = $derived(
    library.filter(([t]) => !onSelection.some(([s, c]) => s.toLowerCase() === t.toLowerCase() && c === photos.length)).slice(0, 12),
  );
  const matches = $derived(text.trim() ? library.filter(([t]) => t.toLowerCase().startsWith(text.trim().toLowerCase())).slice(0, 6) : []);

  function add(tag = text) {
    const t = tag.trim();
    if (!t || !ids.length) return;
    text = "";
    store.changeTags([t], []);
  }
</script>

<aside class="tags">
  <header>Tags <button class="x" title="Close (Ctrl+T)" onclick={() => store.toggleTags()}>✕</button></header>
  {#if !ids.length}
    <p class="hint">Select pictures to tag them.</p>
  {:else}
    <form class="add" onsubmit={(e) => { e.preventDefault(); add(); }}>
      <input bind:value={text} placeholder="Add a tag" list="tag-matches" onkeydown={(e) => e.stopPropagation()} />
      <button type="submit" title="Add the tag to the selected pictures" disabled={!text.trim()}>+</button>
      <datalist id="tag-matches">{#each matches as [t]}<option value={t}></option>{/each}</datalist>
    </form>
    <p class="hint">{ids.length} picture{ids.length === 1 ? "" : "s"} selected{ids.length > READ_MAX ? ` (tags shown for the first ${READ_MAX})` : ""}</p>
    <div class="list">
      {#each onSelection as [t, c] (t)}
        <div class="tag">
          <span>{t}{photos.length > 1 ? ` (${c})` : ""}</span>
          <button title="Remove this tag from the selected pictures" onclick={() => store.changeTags([], [t])}>✕</button>
        </div>
      {:else}
        <p class="none">No tags yet.</p>
      {/each}
    </div>
    {#if suggestions.length}
      <div class="sub">Tags in your library</div>
      <div class="quick">
        {#each suggestions as [t, n] (t)}
          <button title="{n} picture{n === 1 ? '' : 's'}; click to add" onclick={() => add(t)}>{t}</button>
        {/each}
      </div>
    {/if}
    <p class="note">Tags are written into the photo files (where Picasa, Windows and other programs read them). The untouched original is kept the first time, as with Save.</p>
  {/if}
</aside>

<style>
  .tags { width: 236px; flex: none; background: var(--surface-2); border-left: 1px solid var(--chrome-line); overflow-y: auto; display: flex; flex-direction: column; }
  header { background: linear-gradient(#9a9a9a, #7c7c7c); color: #fff; font-weight: 600; font-size: 14px; text-align: center; padding: 5px; position: relative; }
  .x { position: absolute; right: 6px; top: 3px; padding: 0 6px; font-size: 11px; }
  .add { display: flex; gap: 4px; padding: 8px; }
  .add input { flex: 1; min-width: 0; padding: 3px 5px; }
  .hint { color: var(--muted); margin: 0; padding: 0 8px 6px; font-size: 12px; }
  .list { border-top: 1px solid var(--border-soft); }
  .tag { display: flex; justify-content: space-between; align-items: center; padding: 3px 8px; border-bottom: 1px solid var(--border-soft); }
  .tag button { border: none; background: none; color: var(--error); padding: 0 4px; font-size: 11px; }
  .none { color: var(--muted); font-style: italic; padding: 6px 8px; margin: 0; }
  .sub { color: var(--muted); font-size: 12px; padding: 10px 8px 4px; }
  .quick { display: flex; flex-wrap: wrap; gap: 4px; padding: 0 8px; }
  .quick button { font-weight: 400; font-size: 12px; padding: 1px 7px; border-radius: 10px; }
  .note { color: var(--muted); font-size: 11px; line-height: 1.35; padding: 12px 8px; margin: 0; }
</style>
