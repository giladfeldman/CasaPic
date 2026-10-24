<script lang="ts">
  // Picture > Adjust Date and Time. You set the right date for the first chosen photo; every
  // other chosen photo moves by the same amount, so their order and spacing stay as they were
  // (the usual case: a camera clock that was set wrong). The new date is written into the
  // photo's camera data, and the untouched original is kept for Revert.
  import { api, type Photo } from "../lib/api";
  import { store } from "../lib/store.svelte";

  let { ids, onclose }: { ids: number[]; onclose: () => void } = $props();

  let first = $state<Photo | null>(null);
  let value = $state("");
  let busy = $state(false);
  let error = $state<string | null>(null);

  // dates are the camera's wall-clock time with no time zone; shown and typed as such
  const toInput = (secs: number) => new Date(secs * 1000).toISOString().slice(0, 19);
  const fromInput = (v: string) => Math.round(Date.parse(`${v.length === 16 ? `${v}:00` : v}Z`) / 1000);
  const shown = (p: Photo) => p.taken ?? p.mtime;

  $effect(() => {
    api.photo(ids[0]).then((p) => {
      first = p;
      value = toInput(shown(p));
    });
  });

  const shift = $derived(first && value ? fromInput(value) - shown(first) : 0);
  function describe(s: number) {
    if (!Number.isFinite(s)) return "";
    const a = Math.abs(s);
    const d = Math.floor(a / 86400), h = Math.floor((a % 86400) / 3600), m = Math.floor((a % 3600) / 60), sec = a % 60;
    const parts = [d && `${d} day${d === 1 ? "" : "s"}`, h && `${h} hour${h === 1 ? "" : "s"}`, m && `${m} minute${m === 1 ? "" : "s"}`, sec && `${sec} second${sec === 1 ? "" : "s"}`].filter(Boolean);
    return parts.length ? `${s > 0 ? "Later" : "Earlier"} by ${parts.join(", ")}` : "No change";
  }

  async function ok() {
    if (!Number.isFinite(shift)) return;
    busy = true;
    try {
      const n = await api.adjustDates(ids, shift);
      store.flash(`Date changed on ${n} photo${n === 1 ? "" : "s"}`);
      onclose();
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }
</script>

<div class="scrim" role="presentation">
  <div class="dlg" role="dialog" aria-label="Adjust Date and Time">
    <h2>Adjust Date and Time</h2>
    {#if first}
      <p>
        {first.name}{first.taken == null ? " (no camera date; showing its file date)" : ""}:
        {new Date(shown(first) * 1000).toLocaleString(undefined, { timeZone: "UTC", dateStyle: "long", timeStyle: "medium" })}
      </p>
      <label>Correct date and time <input type="datetime-local" step="1" bind:value disabled={busy} /></label>
      <p class="shift">{describe(shift)}</p>
      {#if ids.length > 1}<p class="hint">The other {ids.length - 1} chosen photo{ids.length === 2 ? "" : "s"} move by the same amount.</p>{/if}
      <p class="hint">The new date is written into each photo file's camera data, so other programs see it too. The untouched
        original is kept first; Revert in the Edit view puts it back.</p>
    {/if}
    {#if error}<p class="err">{error}</p>{/if}
    <div class="buttons">
      <button onclick={ok} disabled={busy || !first || !shift || !Number.isFinite(shift)}>OK</button>
      <button onclick={onclose} disabled={busy}>Cancel</button>
    </div>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; background: #0004; display: grid; place-items: center; z-index: 60; }
  .dlg { background: var(--chrome); border: 1px solid #999; box-shadow: 0 6px 24px #0005; padding: 14px 18px; width: min(480px, 92vw); }
  h2 { margin: 0 0 8px; font-size: 16px; }
  p { margin: 0 0 8px; line-height: 1.4; }
  label { display: flex; gap: 10px; align-items: center; }
  input { font-size: 14px; }
  .shift { font-weight: 600; margin-top: 8px; }
  .hint { color: #666; font-size: 12px; }
  .err { color: #b00; }
  .buttons { display: flex; justify-content: flex-end; gap: 8px; margin-top: 12px; }
  .buttons button { min-width: 80px; }
</style>
