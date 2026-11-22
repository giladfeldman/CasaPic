<script lang="ts">
  import { applyTheme } from "../lib/theme";
  // Tools > Options: General, File Types and Slideshow, as in Picasa, and Updates.
  // Nothing is stored until OK; a change of file types reads the library again. The Updates
  // page acts at once (its switch and buttons are not part of OK / Cancel).
  import { api, type Options, type UpdateStatus } from "../lib/api";
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

  let tab = $state<"general" | "types" | "slides" | "updates">("general");
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
  let upd = $state<UpdateStatus | null>(null);
  let updBusy = $state(false);
  let updMsg = $state<string | null>(null);
  const refreshUpdates = () => api.updateStatus().then((s) => (upd = s)).catch((e) => (updMsg = String(e)));
  refreshUpdates();
  async function updRun(action: () => Promise<string>) {
    updBusy = true;
    updMsg = "Checking…";
    try {
      updMsg = await action();
    } catch (e) {
      updMsg = String(e);
    }
    updBusy = false;
    refreshUpdates();
  }
  async function setAuto(on: boolean) {
    try {
      await api.updateSetAuto(on);
    } catch (e) {
      updMsg = String(e);
    }
    refreshUpdates();
  }
  const when = (iso: string | null) => (iso ? new Date(iso).toLocaleString() : "never");

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

<svelte:window onkeydown={(e) => { if (e.key === "Escape" && !busy) { e.preventDefault(); applyTheme(store.options?.theme ?? "system"); onclose(); } }} />

<div class="scrim" role="presentation">
  <div class="dlg" role="dialog" aria-label="Options">
    <h2>Options</h2>
    <div class="tabs">
      <button class:on={tab === "general"} onclick={() => (tab = "general")}>General</button>
      <button class:on={tab === "types"} onclick={() => (tab = "types")}>File Types</button>
      <button class:on={tab === "slides"} onclick={() => (tab = "slides")}>Slideshow</button>
      <button class:on={tab === "updates"} onclick={() => (tab = "updates")}>Updates</button>
    </div>
    {#if o}
      <div class="body">
        {#if tab === "general"}
          <label><input type="checkbox" checked={o.confirm_delete} onchange={(e) => (o!.confirm_delete = e.currentTarget.checked)} />
            Ask before Delete from Disk (the photo goes to the Recycle Bin either way)</label>
          <label><input type="checkbox" checked={o.single_click_exit_edit} onchange={(e) => (o!.single_click_exit_edit = e.currentTarget.checked)} />
            In the Edit view, a single click on the photo goes back to the Library</label>
          <label class="theme">Colours
            <select value={o.theme ?? "system"} onchange={(e) => { o!.theme = e.currentTarget.value as Options["theme"]; applyTheme(o!.theme); }}>
              <option value="system">Same as Windows</option>
              <option value="light">Light</option>
              <option value="dark">Dark</option>
            </select>
          </label>
          <div class="cache">
            <button onclick={clearCache}>Clear Thumbnail Cache</button>
            <span class="hint">{cleared ?? "Makes every thumbnail again from the photo files. Your photos are not touched."}</span>
          </div>
        {:else if tab === "updates"}
          {#if upd}
            <p>CasaPic version {upd.running_version}</p>
            {#if upd.can_update}
              <label><input type="checkbox" checked={upd.auto_update} onchange={(e) => setAuto(e.currentTarget.checked)} />
                Update automatically (checks GitHub when CasaPic starts and once a day)</label>
              <div class="cache">
                <button onclick={() => updRun(api.updateCheck)} disabled={updBusy || upd.checking}>Check now</button>
                {#if upd.pending_version}
                  <button onclick={() => updRun(api.updateInstall)} disabled={updBusy}>Restart and install {upd.pending_version}</button>
                {/if}
              </div>
              <p class="hint">Last check: {when(upd.last_checked)}{upd.last_result ? ` — ${upd.last_result}` : ""}</p>
              {#if updMsg}<p class="hint">{updMsg}</p>{/if}
              <p class="hint">Each update is checked against CasaPic's signature before it is installed. Your library and photos are not touched.</p>
            {:else}
              <p class="hint">{upd.unavailable_reason}</p>
            {/if}
          {:else}
            <p class="hint">{updMsg ?? "Reading…"}</p>
          {/if}
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
      <button onclick={() => { applyTheme(store.options?.theme ?? "system"); onclose(); }} disabled={busy}>Cancel</button>
    </div>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; background: #0004; display: grid; place-items: center; z-index: 60; }
  .dlg { background: var(--chrome); border: 1px solid var(--border); box-shadow: 0 6px 24px #0005; padding: 14px 18px; width: min(520px, 92vw); }
  h2 { margin: 0 0 8px; font-size: 16px; }
  .tabs { display: flex; gap: 2px; border-bottom: 1px solid var(--border-mid); }
  .tabs button { border-radius: 4px 4px 0 0; border-bottom: none; font-weight: 400; }
  .tabs button.on { background: var(--surface); font-weight: 600; }
  .body { background: var(--surface); border: 1px solid var(--border-mid); border-top: none; padding: 12px; display: flex; flex-direction: column; gap: 9px; min-height: 190px; }
  label { display: flex; gap: 8px; align-items: flex-start; line-height: 1.35; }
  label input[type="number"] { width: 60px; }
  .cache { display: flex; gap: 10px; align-items: center; margin-top: 6px; }
  .hint { color: var(--muted); font-size: 12px; margin: 0; }
  .err { color: var(--error); }
  .buttons { display: flex; justify-content: flex-end; gap: 8px; margin-top: 12px; }
  .buttons button { min-width: 80px; }
</style>
