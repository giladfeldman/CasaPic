<script lang="ts">
  // Export Picture to Folder: writes new JPEGs with the edits applied. Originals are never
  // touched and an existing file is never overwritten (a number is added instead).
  import { open } from "@tauri-apps/plugin-dialog";
  import { api } from "../lib/api";
  import { store } from "../lib/store.svelte";

  let { ids, onclose }: { ids: number[]; onclose: () => void } = $props();

  let defaultDir = $state("");
  let dest = $state<string | null>(null);
  let resize = $state(false);
  let edge = $state(800);
  let quality = $state(92);
  let running = $state(false);
  let error = $state<string | null>(null);

  $effect(() => {
    api.defaultExportDir(ids[0]).then((d) => (defaultDir = d));
  });

  async function browse() {
    const d = await open({ directory: true, title: "Choose the export folder" });
    if (typeof d === "string") dest = d;
  }

  async function go() {
    running = true;
    error = null;
    try {
      const files = await api.exportPhotos(ids, dest, resize ? edge : null, quality);
      onclose();
      store.message = `Exported ${files.length} picture${files.length === 1 ? "" : "s"} to ${dest ?? defaultDir.replace(/[\\/][^\\/]*$/, "")}.`;
    } catch (e) {
      error = String(e);
    } finally {
      running = false;
    }
  }
</script>

<div class="scrim" role="presentation">
  <div class="dlg" role="dialog" aria-label="Export to Folder">
    <h2>Export to Folder</h2>
    <p class="n">{ids.length} picture{ids.length === 1 ? "" : "s"} selected</p>
    <fieldset>
      <legend>Export location</legend>
      <div class="loc">
        <span title={dest ?? defaultDir}>{dest ?? `${defaultDir} (one folder per source folder)`}</span>
        <button onclick={browse} disabled={running}>Browse…</button>
      </div>
    </fieldset>
    <fieldset>
      <legend>Image size</legend>
      <label><input type="radio" name="size" checked={!resize} onchange={() => (resize = false)} /> Use original size</label>
      <label><input type="radio" name="size" checked={resize} onchange={() => (resize = true)} /> Resize to:</label>
      <div class="slider" class:off={!resize}>
        <input type="range" min="200" max="4000" step="50" bind:value={edge} disabled={!resize} />
        <b>{edge} pixels</b>
      </div>
    </fieldset>
    <fieldset>
      <legend>Image quality</legend>
      <select bind:value={quality}>
        <option value={100}>Maximum (100)</option>
        <option value={92}>High (92)</option>
        <option value={85}>Normal (85)</option>
        <option value={70}>Small files (70)</option>
      </select>
    </fieldset>
    {#if error}<p class="err">{error}</p>{/if}
    <div class="btns">
      <button onclick={go} disabled={running}>{running ? "Exporting…" : "Export"}</button>
      <button onclick={onclose} disabled={running}>Cancel</button>
    </div>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; background: #0004; display: grid; place-items: center; z-index: 50; }
  .dlg { width: 460px; background: var(--chrome); border: 1px solid #999; box-shadow: 0 6px 24px #0005; padding: 14px 18px; }
  h2 { margin: 0 0 4px; font-size: 16px; }
  .n { margin: 0 0 10px; color: #666; }
  fieldset { border: 1px solid #ccc; margin: 0 0 10px; padding: 6px 10px 10px; display: flex; flex-direction: column; gap: 4px; }
  legend { font-weight: 600; }
  .loc { display: flex; gap: 8px; align-items: center; }
  .loc span { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: #333; }
  .slider { display: flex; gap: 10px; align-items: center; padding-left: 22px; }
  .slider.off { opacity: 0.45; }
  .slider input { flex: 1; }
  .btns { display: flex; justify-content: flex-end; gap: 8px; }
  .btns button { min-width: 80px; height: 28px; }
  .err { color: #b00; }
</style>
