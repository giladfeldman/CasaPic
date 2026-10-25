<script lang="ts">
  // Tools > Options: General, File Types and Slideshow, as in Picasa.
  // Nothing is stored until OK; a change of file types reads the library again.
  import { api, type Options } from "../lib/api";
  import { store } from "../lib/store.svelte";

  let { onclose }: { onclose: () => void } = $props();

  const TYPES: [string, string][] = [
    ["jpg", "JPEG (.jpg, .jpeg, .jpe)"],
    ["png", "PNG (.png)"],
    ["gif", "GIF (.gif)"],
    ["bmp", "Bitmap (.bmp)"],
    ["tiff", "TIFF (.tif, .tiff)"],
    ["webp", "WebP (.webp)"],
    ["heic", "HEIC (.heic, iPhone)"],
  ];
  // the stored list spells out every extension; the dialog shows one box per kind
  const SPELLED: Record<string, string[]> = { jpg: ["jpg", "jpeg", "jpe"], tiff: ["tif", "tiff"] };

  let tab = $state<"general" | "types" | "slides">("general");
  let o = $state<Options | null>(null);
  let kinds = $state<Record<string, boolean>>({});
  let busy = $state(false);
  let error = $state<string | null>(null);
  let cleared = $state<string | null>(null);

  api.settings().then((s) => {
    o = { ...s.options };
    kinds = Object.fromEntries(TYPES.map(([k]) => [k, s.options.file_types.includes(k)]));
  });

  async function ok() {
    if (!o) return;
    busy = true;
    try {
      const file_types = TYPES.filter(([k]) => kinds[k]).flatMap(([k]) => SPELLED[k] ?? [k]);
      if (!file_types.length) throw new Error("Choose at least one kind of file.");
      await api.optionsSave({ ...o, file_types });
      await store.loadSettings();
      onclose();
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }
  async function clearCache() {
    cleared = "Clearing…";
    try {
      const n = await api.clearThumbCache();
      store.thumbV = {};
      await store.reload();
      cleared = `Cleared ${n} thumbnails; new ones are being made.`;
    } catch (e) {
      cleared = String(e);
    }
  }
</script>

<div class="scrim" role="presentation">
  <div class="dlg" role="dialog" aria-label="Options">
    <h2>Options</h2>
    <div class="tabs">
      <button class:on={tab === "general"} onclick={() => (tab = "general")}>General</button>
      <button class:on={tab === "types"} onclick={() => (tab = "types")}>File Types</button>
      <button class:on={tab === "slides"} onclick={() => (tab = "slides")}>Slideshow</button>
    </div>
    {#if o}
      <div class="body">
        {#if tab === "general"}
          <label><input type="checkbox" checked={o.confirm_delete} onchange={(e) => (o!.confirm_delete = e.currentTarget.checked)} />
            Ask before Delete from Disk (the photo goes to the Recycle Bin either way)</label>
          <label><input type="checkbox" checked={o.single_click_exit_edit} onchange={(e) => (o!.single_click_exit_edit = e.currentTarget.checked)} />
            In the Edit view, a single click on the photo goes back to the Library</label>
          <div class="cache">
            <button onclick={clearCache}>Clear Thumbnail Cache</button>
            <span class="hint">{cleared ?? "Makes every thumbnail again from the photo files. Your photos are not touched."}</span>
          </div>
        {:else if tab === "slides"}
          <label>Display time
            <input type="number" min="1" max="60" step="1" value={o.slide_seconds}
              onchange={(e) => (o!.slide_seconds = Math.max(1, Math.min(60, Number(e.currentTarget.value) || 3)))} /> seconds per photo</label>
          <label><input type="checkbox" checked={o.slide_transition !== "none"} onchange={(e) => (o!.slide_transition = e.currentTarget.checked ? "fade" : "none")} />
            Fade from one photo to the next</label>
          <p class="hint">The slideshow (Ctrl+4) fills the screen; the bar at the bottom also changes the time while it plays.</p>
        {:else}
          <p class="hint">Which kinds of files CasaPic shows. Unticking a kind only hides those files here.</p>
          {#each TYPES as [k, label]}
            <label><input type="checkbox" checked={kinds[k]} onchange={(e) => (kinds = { ...kinds, [k]: e.currentTarget.checked })} /> {label}</label>
          {/each}
        {/if}
      </div>
    {/if}
    {#if error}<p class="err">{error}</p>{/if}
    <div class="buttons">
      <button onclick={ok} disabled={busy || !o}>OK</button>
      <button onclick={onclose} disabled={busy}>Cancel</button>
    </div>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; background: #0004; display: grid; place-items: center; z-index: 60; }
  .dlg { background: var(--chrome); border: 1px solid #999; box-shadow: 0 6px 24px #0005; padding: 14px 18px; width: min(520px, 92vw); }
  h2 { margin: 0 0 8px; font-size: 16px; }
  .tabs { display: flex; gap: 2px; border-bottom: 1px solid #bbb; }
  .tabs button { border-radius: 4px 4px 0 0; border-bottom: none; font-weight: 400; }
  .tabs button.on { background: #fff; font-weight: 600; }
  .body { background: #fff; border: 1px solid #bbb; border-top: none; padding: 12px; display: flex; flex-direction: column; gap: 9px; min-height: 190px; }
  label { display: flex; gap: 8px; align-items: flex-start; line-height: 1.35; }
  label input[type="number"] { width: 60px; }
  .cache { display: flex; gap: 10px; align-items: center; margin-top: 6px; }
  .hint { color: #777; font-size: 12px; margin: 0; }
  .err { color: #b00; }
  .buttons { display: flex; justify-content: flex-end; gap: 8px; margin-top: 12px; }
  .buttons button { min-width: 80px; }
</style>
