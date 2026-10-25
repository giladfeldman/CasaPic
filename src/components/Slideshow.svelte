<script lang="ts">
  // Slideshow (Ctrl+4): full screen, the photos one after another with their edits, as
  // Picasa's. The bar at the bottom shows while the mouse moves: previous, play/pause, next,
  // star, seconds per photo, exit. Keys: arrows / Page Up/Down, Space (pause), Home/End, Esc.
  import { onDestroy, onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { store } from "../lib/store.svelte";
  import { api, renderUrl } from "../lib/api";

  const show = store.slideshow!;
  const ids = show.ids;
  let i = $state(Math.max(0, Math.min(show.start, ids.length - 1)));
  let playing = $state(true);
  let seconds = $state(Math.max(1, store.options?.slide_seconds ?? 3));
  const fade = (store.options?.slide_transition ?? "fade") !== "none";
  // two layers, so the next photo can fade in over the last
  let urls = $state<[string, string]>(["", ""]);
  let front = $state(0);
  let caption = $state("");
  let name = $state("");
  let bar = $state(true);
  let failed = $state("");

  const edge = Math.min(4096, Math.ceil((Math.max(screen.width, screen.height) * devicePixelRatio) / 256) * 256);
  const url = (id: number) => renderUrl(id, edge, store.thumbV[id] ?? 0);

  let timer: ReturnType<typeof setTimeout> | undefined;
  let ticket = 0;
  async function showAt(k: number) {
    if (!ids.length) return;
    i = ((k % ids.length) + ids.length) % ids.length;
    const t = ++ticket;
    clearTimeout(timer);
    const id = ids[i];
    const img = new Image();
    img.src = url(id);
    try {
      await img.decode();
      failed = "";
    } catch {
      failed = "This photo could not be shown";
    }
    if (t !== ticket) return;
    const back = 1 - front;
    urls[back] = img.src;
    front = back;
    api.photo(id).then((p) => {
      if (t !== ticket) return;
      caption = p.caption;
      name = p.path.split(/[\\/]/).pop() ?? "";
    }, () => {});
    // the next photo is fetched while this one shows
    const next = ids[(i + 1) % ids.length];
    if (next !== id) new Image().src = url(next);
    if (playing) timer = setTimeout(() => showAt(i + 1), seconds * 1000);
  }
  function toggle() {
    playing = !playing;
    clearTimeout(timer);
    if (playing) timer = setTimeout(() => showAt(i + 1), seconds * 1000);
  }
  function setSeconds(s: number) {
    seconds = Math.max(1, Math.min(60, s));
    if (playing) {
      clearTimeout(timer);
      timer = setTimeout(() => showAt(i + 1), seconds * 1000);
    }
  }

  let wasFull = false;
  onMount(async () => {
    showAt(i);
    wake();
    try {
      const w = getCurrentWindow();
      wasFull = await w.isFullscreen();
      if (!wasFull) await w.setFullscreen(true);
    } catch (e) {
      store.message = String(e);
    }
  });
  onDestroy(() => {
    clearTimeout(timer);
    clearTimeout(barTimer);
    if (!wasFull) getCurrentWindow().setFullscreen(false).catch(() => {});
  });
  function close() {
    store.select(ids[i]);
    store.slideshow = null;
  }

  let barTimer: ReturnType<typeof setTimeout> | undefined;
  function wake() {
    bar = true;
    clearTimeout(barTimer);
    barTimer = setTimeout(() => (bar = false), 2500);
  }

  function onKey(e: KeyboardEvent) {
    const k = e.key;
    if (k === "Escape") close();
    else if (k === "ArrowRight" || k === "ArrowDown" || k === "PageDown") showAt(i + 1);
    else if (k === "ArrowLeft" || k === "ArrowUp" || k === "PageUp") showAt(i - 1);
    else if (k === " ") toggle();
    else if (k === "Home") showAt(0);
    else if (k === "End") showAt(ids.length - 1);
    else return;
    e.preventDefault();
    e.stopPropagation();
  }
  const starred = $derived(store.starred.has(ids[i]));
</script>

<svelte:window onkeydown={onKey} />

<div class="show" class:nocursor={!bar} role="presentation" onpointermove={wake}>
  {#each [0, 1] as l (l)}
    {#if urls[l]}<img src={urls[l]} alt="" class:on={front === l} class:fade />{/if}
  {/each}
  {#if failed}<p class="failed">{failed}</p>{/if}
  {#if caption}<p class="caption">{caption}</p>{/if}
  <div class="bar" class:hidden={!bar}>
    <button title="Previous (←)" onclick={() => showAt(i - 1)}>◀</button>
    <button class="play" title={playing ? "Pause (Space)" : "Play (Space)"} onclick={toggle}>{playing ? "❚❚" : "▶"}</button>
    <button title="Next (→)" onclick={() => showAt(i + 1)}>▶</button>
    <span class="sep"></span>
    <button class="star" class:on={starred} title="Star / Unstar this photo" onclick={() => store.toggleStar([ids[i]])}>★</button>
    <span class="sep"></span>
    <button title="Shorter" onclick={() => setSeconds(seconds - 1)}>−</button>
    <span class="secs" title="Seconds per photo">{seconds} s</span>
    <button title="Longer" onclick={() => setSeconds(seconds + 1)}>+</button>
    <span class="sep"></span>
    <span class="where">{i + 1} of {ids.length} · {name}</span>
    <button class="exit" title="Exit the slideshow (Esc)" onclick={close}>Exit</button>
  </div>
</div>

<style>
  .show { position: fixed; inset: 0; background: #000; z-index: 100; overflow: hidden; }
  .show.nocursor { cursor: none; }
  img { position: absolute; inset: 0; width: 100%; height: 100%; object-fit: contain; opacity: 0; }
  img.fade { transition: opacity 0.6s ease; }
  img.on { opacity: 1; }
  .caption { position: absolute; left: 0; right: 0; bottom: 64px; margin: 0; text-align: center; color: #fff; font-size: 20px; text-shadow: 0 1px 4px #000; }
  .failed { position: absolute; top: 45%; width: 100%; text-align: center; color: #bbb; }
  .bar { position: absolute; left: 50%; bottom: 14px; transform: translateX(-50%); display: flex; align-items: center; gap: 6px; padding: 6px 12px; background: #222c; border-radius: 6px; color: #ddd; transition: opacity 0.3s; font-size: 13px; }
  .bar.hidden { opacity: 0; pointer-events: none; }
  .bar button { background: #444; color: #eee; border: 1px solid #666; border-radius: 3px; min-width: 30px; padding: 3px 8px; }
  .bar button:hover { background: #555; }
  .star.on { color: #f0b400; }
  .sep { width: 10px; }
  .secs { min-width: 34px; text-align: center; }
  .where { max-width: 360px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: #aaa; }
  .exit { margin-left: 8px; }
</style>
