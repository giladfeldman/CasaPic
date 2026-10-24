<script lang="ts">
  // The small questions file actions ask: a name (Rename, New Folder…) or a yes/no (Delete…).
  // An error from the action shows in the dialog, which stays open so the text can be fixed.
  import { store } from "../lib/store.svelte";

  let text = $state(store.prompt?.value ?? "");
  let busy = $state(false);
  let error = $state<string | null>(null);

  function close() {
    store.prompt = null;
    store.confirm = null;
  }
  async function ok() {
    busy = true;
    error = null;
    try {
      if (store.prompt) await store.prompt.run(text);
      else await store.confirm?.run();
      close();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
  function key(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Enter" && !busy) ok();
    if (e.key === "Escape") close();
  }
  /** selects the name without its extension, as Windows does */
  function pick(node: HTMLInputElement) {
    node.focus();
    const dot = node.value.lastIndexOf(".");
    node.setSelectionRange(0, dot > 0 ? dot : node.value.length);
  }
  function focus(node: HTMLElement) {
    node.focus();
  }
</script>

<div class="scrim" role="presentation">
  <div class="dlg" role="dialog" aria-label={store.prompt?.title ?? store.confirm?.title} tabindex="-1" onkeydown={key}>
    <h2>{store.prompt?.title ?? store.confirm?.title}</h2>
    {#if store.prompt}
      <label>{store.prompt.label}<input bind:value={text} use:pick disabled={busy} /></label>
    {:else if store.confirm}
      <p>{store.confirm.text}</p>
    {/if}
    {#if error}<p class="err">{error}</p>{/if}
    <div class="buttons">
      {#if store.confirm}
        <button onclick={ok} disabled={busy} use:focus>{store.confirm.ok}</button>
      {:else}
        <button onclick={ok} disabled={busy || !text.trim()}>{store.prompt?.ok}</button>
      {/if}
      <button onclick={close} disabled={busy}>Cancel</button>
    </div>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; background: #0004; display: grid; place-items: center; z-index: 70; }
  .dlg { background: var(--chrome); border: 1px solid #999; box-shadow: 0 6px 24px #0005; padding: 14px 18px; width: min(460px, 92vw); outline: none; }
  h2 { margin: 0 0 10px; font-size: 16px; }
  label { display: flex; flex-direction: column; gap: 6px; }
  input { font-size: 14px; padding: 3px 6px; }
  p { line-height: 1.45; margin: 0 0 6px; }
  .err { color: #b00; margin-top: 8px; }
  .buttons { display: flex; justify-content: flex-end; gap: 8px; margin-top: 14px; }
  .buttons button { min-width: 80px; }
</style>
