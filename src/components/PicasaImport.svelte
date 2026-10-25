<script lang="ts">
  // Tools > Import Picasa Settings: first shows what Picasa left in the library's folders
  // (its .picasa.ini files, which are only read), then brings it over on Import.
  import { store } from "../lib/store.svelte";
  import { api, type PicasaSummary } from "../lib/api";

  let { onclose }: { onclose: () => void } = $props();
  let s = $state<PicasaSummary | null>(null);
  let done = $state(false);
  let busy = $state(true);
  let error = $state("");

  api.picasaImport(false).then((r) => { s = r; busy = false; }, (e) => { error = String(e); busy = false; });

  const n = (k: number, one: string, many = one + "s") => `${k} ${k === 1 ? one : many}`;
  const unknown = $derived(s ? Object.entries(s.unknown) : []);
  const nothing = $derived(!!s && !s.folders);

  async function go() {
    busy = true;
    error = "";
    try {
      s = await api.picasaImport(true);
      done = true;
      await store.reload();
      store.flash("Picasa settings imported");
    } catch (e) {
      error = String(e);
    }
    busy = false;
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && !busy && onclose()} />

<div class="scrim" role="presentation">
  <div class="dlg" role="dialog" aria-label="Import Picasa Settings">
    <header>Import Picasa Settings</header>
    <div class="body">
      {#if busy && !s}
        <p>Looking for Picasa's settings in your folders…</p>
      {:else if nothing}
        <p>No Picasa settings were found in the folders CasaPic watches.</p>
      {:else if s}
        <p>{done ? "Brought over from" : "Picasa left settings in"} {n(s.folders, "folder")}:</p>
        <ul>
          {#if s.stars}<li>{n(s.stars, "starred photo")}</li>{/if}
          {#if s.edited}<li>{n(s.edited, "edited photo")} (crops, fixes and effects; nothing in the photo files changes)</li>{/if}
          {#if s.albums.length}<li>{n(s.albums.length, "album")} ({s.albums.join(", ")}) with {n(s.album_photos, "photo")}</li>{/if}
          {#if s.hidden_photos}<li>{n(s.hidden_photos, "hidden photo")}</li>{/if}
          {#if s.hidden_folders}<li>{n(s.hidden_folders, "hidden folder")}</li>{/if}
          {#if s.descriptions}<li>{n(s.descriptions, "folder description")}</li>{/if}
          {#if s.captions}<li>{n(s.captions, "photo")} with an old-style caption or tags: these are written into the photo files, the original kept once (as CasaPic does for tags)</li>{/if}
        </ul>
        {#if s.kept_casapic_edits}<p class="note">{n(s.kept_casapic_edits, "photo")} already {s.kept_casapic_edits === 1 ? "has" : "have"} CasaPic edits; {s.kept_casapic_edits === 1 ? "it keeps them" : "they keep them"} and Picasa's edits are not applied.</p>{/if}
        {#if s.saved_in_picasa}<p class="note">{n(s.saved_in_picasa, "photo")} {s.saved_in_picasa === 1 ? "was" : "were"} saved in Picasa: those edits are already in the picture.</p>{/if}
        {#if unknown.length}
          <p class="note">Picasa edits CasaPic cannot show yet (kept, so nothing is lost): {unknown.map(([k, c]) => `${k} (${c})`).join(", ")}.</p>
        {/if}
        <p class="note">Picasa's own files are only read, never changed.</p>
      {/if}
      {#if error}<p class="err">{error}</p>{/if}
    </div>
    <div class="btns">
      {#if s && !nothing && !done}<button disabled={busy} onclick={go}>{busy ? "Importing…" : "Import"}</button>{/if}
      <button disabled={busy && !!s} onclick={onclose}>{done || nothing ? "Close" : "Cancel"}</button>
    </div>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; background: #0004; display: grid; place-items: center; z-index: 50; }
  .dlg { background: var(--chrome); border: 1px solid #999; width: 480px; box-shadow: 0 6px 24px #0005; }
  header { background: linear-gradient(#9a9a9a, #7c7c7c); color: #fff; font-weight: 600; padding: 5px 10px; }
  .body { padding: 10px 16px; font-size: 13px; }
  ul { margin: 4px 0 8px; padding-left: 20px; }
  .note { color: #555; font-size: 12px; }
  .err { color: #b00; }
  .btns { display: flex; justify-content: flex-end; gap: 8px; padding: 0 16px 12px; }
</style>
