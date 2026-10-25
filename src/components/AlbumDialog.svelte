<script lang="ts">
  // Album Properties (also used for New Album): Name, Date, Place taken, Description — the
  // fields Picasa's own dialog has, stored under the same names.
  import { store } from "../lib/store.svelte";
  import { api } from "../lib/api";

  const d = store.albumDialog!;
  let name = $state(d.album.name);
  let day = $state(d.album.date.slice(0, 10));
  let location = $state(d.album.location);
  let description = $state(d.album.description);
  let busy = $state(false);
  let error = $state("");

  async function ok() {
    if (!name.trim()) return (error = "Give the album a name.");
    busy = true;
    // Picasa writes the album date as ISO 8601 with a time zone, e.g. 2026-10-24T23:08:56+00:00
    const keepTime = d.album.date.slice(0, 10) === day ? d.album.date : "";
    const date = !day ? "" : keepTime || `${day}T00:00:00+00:00`;
    const album = { token: d.album.token, name: name.trim(), date, location: location.trim(), description: description.trim() };
    try {
      if (d.isNew) {
        const token = await api.albumCreate(album, d.ids);
        store.flash(d.ids.length ? `Album ${album.name} made with ${d.ids.length} photo${d.ids.length === 1 ? "" : "s"}` : `Album ${album.name} made`);
        store.albumDialog = null;
        await store.reload();
        if (!d.ids.length) await store.openAlbum(token);
      } else {
        await api.albumEdit(album);
        store.albumDialog = null;
      }
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }
  function focus(node: HTMLInputElement) {
    node.focus();
    node.select();
  }
</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && (store.albumDialog = null)} />

<div class="scrim" role="presentation">
  <div class="dlg" role="dialog" aria-label="Album Properties">
    <header>{d.isNew ? "New Album" : "Album Properties"}</header>
    <form onsubmit={(e) => { e.preventDefault(); ok(); }}>
      <label>Album Name <input bind:value={name} use:focus maxlength="200" /></label>
      <label>Date <input type="date" bind:value={day} /></label>
      <label>Place taken (optional) <input bind:value={location} maxlength="300" /></label>
      <label>Description (optional) <textarea bind:value={description} rows="4" maxlength="4000"></textarea></label>
      {#if error}<p class="err">{error}</p>{/if}
      <div class="btns">
        <button type="submit" disabled={busy}>OK</button>
        <button type="button" onclick={() => (store.albumDialog = null)}>Cancel</button>
      </div>
    </form>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; background: #0004; display: grid; place-items: center; z-index: 50; }
  .dlg { background: var(--chrome); border: 1px solid #999; width: 420px; box-shadow: 0 6px 24px #0005; }
  header { background: linear-gradient(#9a9a9a, #7c7c7c); color: #fff; font-weight: 600; padding: 5px 10px; }
  form { padding: 12px 16px; display: flex; flex-direction: column; gap: 10px; }
  label { display: flex; flex-direction: column; gap: 3px; font-weight: 600; font-size: 12px; }
  input, textarea { font-weight: 400; font-size: 13px; padding: 3px 5px; }
  textarea { resize: vertical; font-family: inherit; }
  .err { color: #b00; margin: 0; }
  .btns { display: flex; justify-content: flex-end; gap: 8px; }
</style>
