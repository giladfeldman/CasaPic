<script lang="ts">
  // Edit view: one photo, the Basic Fixes panel, Undo/Redo that name the action, a filmstrip
  // of the folder. Every fix is rendered by the Rust core and saved to the sidecar at once.
  import { store } from "../lib/store.svelte";
  import { api, fetchPreview, thumbUrl, type EditState, type Fix, type Photo } from "../lib/api";

  type Mode = "fixes" | "crop" | "straighten";

  let mode = $state<Mode>("fixes");
  let st = $state<EditState | null>(null);
  let photo = $state<Photo | null>(null);
  let error = $state<string | null>(null);
  let busy = $state(false);
  let fill = $state(0);
  let frameW = $state(0);
  let frameH = $state(0);
  let canvas: HTMLCanvasElement | undefined = $state();
  let histo: HTMLCanvasElement | undefined = $state();

  const id = $derived(store.focus);
  const folder = $derived(id != null ? store.folderOf.get(id) : undefined);
  const index = $derived(folder && id != null ? folder.photos.findIndex((p) => p[0] === id) : -1);

  // ---- loading and drawing ----
  let ticket = 0;
  $effect(() => {
    const target = id;
    if (target == null || frameW === 0) return;
    const t = ++ticket;
    mode = "fixes";
    crop = null;
    angle = 0;
    const edge = Math.min(4096, Math.max(512, Math.ceil((Math.max(frameW, frameH) * devicePixelRatio) / 256) * 256));
    api.photo(target).then((p) => t === ticket && (photo = p));
    run(() => api.editOpen(target, edge), t).then(() => {
      // decode the next photos in the direction of travel (and one behind) now, so moving to
      // them only has to send the picture; three ahead keeps up with Next held down
      const ps = folder?.photos;
      if (t !== ticket || !ps || index < 0) return;
      const dir = index < lastIndex ? -1 : 1;
      lastIndex = index;
      const near = [1, 2, 3, -1].map((k) => ps[index + k * dir]?.[0]).filter((x): x is number => x != null);
      if (near.length) api.prepare(near, edge);
    });
  });
  let lastIndex = -1;

  async function run(call: () => Promise<EditState>, t = ticket) {
    busy = true;
    error = null;
    try {
      const t0 = performance.now();
      const s = await call();
      if (t !== ticket) return;
      const t1 = performance.now();
      await draw(s);
      if (store.timing) api.mark(`edit-front call=${(t1 - t0).toFixed(1)} fetch=${fetchMs.toFixed(1)} put=${(performance.now() - t1 - fetchMs).toFixed(1)}`);
      st = s;
      fill = s.fill;
      store.drawn = s.id;
    } catch (e) {
      if (t === ticket) error = String(e);
    } finally {
      if (t === ticket) busy = false;
    }
  }

  let fetchMs = 0;
  async function draw(s: EditState) {
    const tf = performance.now();
    const img = await fetchPreview(s.version);
    fetchMs = performance.now() - tf;
    if (!canvas) return img.close();
    canvas.width = img.width;
    canvas.height = img.height;
    canvas.getContext("2d")!.drawImage(img, 0, 0);
    img.close();
    drawHistogram(s.hist);
  }

  /** `hist`: 64 bins each for R, G, B, counted by the Rust core. */
  function drawHistogram(hist: number[]) {
    if (!histo) return;
    const bins = [0, 1, 2].map((c) => hist.slice(c * 64, c * 64 + 64));
    const max = Math.max(...bins.flatMap((b) => b.slice(1, 63)), 1);
    const g = histo.getContext("2d")!;
    g.clearRect(0, 0, histo.width, histo.height);
    g.globalCompositeOperation = "lighter";
    ["#c33", "#3a3", "#33c"].forEach((col, c) => {
      g.fillStyle = col;
      for (let x = 0; x < 64; x++) {
        const h = Math.min(1, bins[c][x] / max) * histo!.height;
        g.fillRect(x * 3.5, histo!.height - h, 3.5, h);
      }
    });
    g.globalCompositeOperation = "source-over";
  }

  // displayed size of the photo inside the frame
  const disp = $derived.by(() => {
    if (!st || !frameW) return { w: 0, h: 0 };
    const k = Math.min((frameW - 40) / st.w, (frameH - 40) / st.h);
    return { w: Math.round(st.w * k), h: Math.round(st.h * k) };
  });

  // ---- fixes ----
  const apply = (kind: Fix, params: number[] = []) => run(() => api.editApply(kind, params));

  let fillPending: number | null = null;
  let fillBusy = false;
  async function fillInput(v: number) {
    fill = v;
    fillPending = v;
    if (fillBusy) return;
    fillBusy = true;
    while (fillPending != null) {
      const x = fillPending;
      fillPending = null;
      try {
        const s = await api.fillPreview(x);
        await draw(s);
      } catch (e) {
        error = String(e);
      }
    }
    fillBusy = false;
  }
  async function fillCommit(v: number) {
    while (fillBusy) await new Promise((r) => setTimeout(r, 10));
    apply("fill", [v]);
  }

  // ---- crop ----
  const ASPECTS: [string, number][] = [
    ["Manual", 0],
    ["4 x 6", 3 / 2],
    ["5 x 7", 7 / 5],
    ["8 x 10", 5 / 4],
    ["Square", 1],
    ["4 x 3 Display", 4 / 3],
    ["16 x 9 Display", 16 / 9],
  ];
  let aspect = $state(0);
  let crop = $state<{ x0: number; y0: number; x1: number; y1: number } | null>(null);
  let drag: { kind: "new" | "move"; sx: number; sy: number; start: typeof crop } | null = null;

  const clamp = (v: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, v));
  function local(e: PointerEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    return { x: clamp(e.clientX - r.left, 0, disp.w), y: clamp(e.clientY - r.top, 0, disp.h) };
  }
  function cropDown(e: PointerEvent) {
    const p = local(e);
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    const inside = crop && p.x > crop.x0 && p.x < crop.x1 && p.y > crop.y0 && p.y < crop.y1;
    drag = { kind: inside ? "move" : "new", sx: p.x, sy: p.y, start: crop };
  }
  function cropMove(e: PointerEvent) {
    if (!drag) return;
    const p = local(e);
    if (drag.kind === "move" && drag.start) {
      const s = drag.start, w = s.x1 - s.x0, h = s.y1 - s.y0;
      const x0 = clamp(s.x0 + p.x - drag.sx, 0, disp.w - w), y0 = clamp(s.y0 + p.y - drag.sy, 0, disp.h - h);
      crop = { x0, y0, x1: x0 + w, y1: y0 + h };
      return;
    }
    let dx = p.x - drag.sx, dy = p.y - drag.sy;
    if (aspect) {
      // the picture's long side follows the drag's long side
      const r = Math.abs(dx) >= Math.abs(dy) ? aspect : 1 / aspect;
      const sx = Math.sign(dx) || 1, sy = Math.sign(dy) || 1;
      let w = Math.abs(dx), h = w / r;
      if (h < Math.abs(dy)) { h = Math.abs(dy); w = h * r; }
      // keep inside the photo
      const maxW = sx > 0 ? disp.w - drag.sx : drag.sx, maxH = sy > 0 ? disp.h - drag.sy : drag.sy;
      const k = Math.min(1, maxW / w, maxH / h);
      dx = sx * w * k;
      dy = sy * h * k;
    }
    const x = drag.sx + dx, y = drag.sy + dy;
    crop = { x0: Math.min(drag.sx, x), y0: Math.min(drag.sy, y), x1: Math.max(drag.sx, x), y1: Math.max(drag.sy, y) };
  }
  function cropUp() {
    drag = null;
    if (crop && (crop.x1 - crop.x0 < 4 || crop.y1 - crop.y0 < 4)) crop = null;
  }
  async function cropApply() {
    if (!crop) return;
    const c = crop;
    await apply("crop", [c.x0 / disp.w, c.y0 / disp.h, c.x1 / disp.w, c.y1 / disp.h]);
    crop = null;
    mode = "fixes";
  }
  const cropPx = $derived(
    crop && st ? `${Math.round(((crop.x1 - crop.x0) / disp.w) * (photo?.width ?? st.w))} x ${Math.round(((crop.y1 - crop.y0) / disp.h) * (photo?.height ?? st.h))}` : "",
  );

  // ---- straighten (previewed with CSS, rendered by the core on Apply) ----
  let angle = $state(0);
  const straightenScale = $derived.by(() => {
    if (!st) return 1;
    const a = (Math.abs(angle) * Math.PI) / 180, c = Math.cos(a), s = Math.sin(a);
    return Math.min(st.w / (st.w * c + st.h * s), st.h / (st.w * s + st.h * c));
  });
  async function straightenApply() {
    await apply("straighten", [angle]);
    angle = 0;
    mode = "fixes";
  }

  // the tray's rotate buttons must go through this session, or its next save would undo them
  $effect(() => {
    store.editRotate = (turns: number) => apply("rotate", [turns]);
    return () => (store.editRotate = null);
  });

  // ---- navigation and keys ----
  function go(step: number) {
    if (!folder) return;
    const next = folder.photos[index + step];
    if (next) store.select(next[0]);
  }
  const strip = $derived.by(() => {
    if (!folder) return [];
    const lo = Math.max(0, Math.min(index - 4, folder.photos.length - 9));
    return folder.photos.slice(lo, lo + 9).map((p) => p[0]);
  });

  function onKey(e: KeyboardEvent) {
    if (store.view !== "edit" || (e.target as HTMLElement)?.closest("input, select, dialog")) return;
    const k = e.key.toLowerCase();
    if (k === "escape") {
      if (mode !== "fixes") { mode = "fixes"; crop = null; angle = 0; }
      else store.view = "library";
    } else if (mode !== "fixes") return;
    else if (k === "arrowright" || k === "pagedown") go(1);
    else if (k === "arrowleft" || k === "pageup") go(-1);
    else if (k === "home" && folder) store.select(folder.photos[0][0]);
    else if (k === "end" && folder) store.select(folder.photos[folder.photos.length - 1][0]);
    else if (e.ctrlKey && k === "z" && st?.undo) run(() => api.undo(false));
    else if (e.ctrlKey && k === "y" && st?.redo) run(() => api.undo(true));
    else if (e.ctrlKey && k === "r") apply("rotate", [e.shiftKey ? -1 : 1]);
    else if (e.ctrlKey && e.shiftKey && k === "e") apply("lucky");
    else if (e.ctrlKey && e.shiftKey && k === "s") store.exportFor = store.picked();
    else if (e.ctrlKey && k === "enter" && id != null) api.locate(id);
    else return;
    e.preventDefault();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="edit">
  <div class="topbar">
    <button class="back" onclick={() => (store.view = "library")}>◀ Back to Library</button>
    <div class="strip">
      <button class="arrow" disabled={index <= 0} onclick={() => go(-1)} title="Previous (←)">◀</button>
      {#each strip as sid (sid)}
        <button class="film" class:cur={sid === id} onclick={() => store.select(sid)}>
          {#if (store.thumbV[sid] ?? 0) > 0}<img src={thumbUrl(sid, store.thumbV[sid])} alt="" />{/if}
        </button>
      {/each}
      <button class="arrow" disabled={!folder || index >= folder.photos.length - 1} onclick={() => go(1)} title="Next (→)">▶</button>
    </div>
    <span class="count">{folder ? `${index + 1} of ${folder.photos.length}` : ""}</span>
  </div>

  <div class="body">
    <aside class="panel">
      {#if mode === "fixes"}
        <div class="tabs"><span class="tab on" title="Basic Fixes">🔧 Basic Fixes</span></div>
        <div class="fixes">
          <button class="fix" onclick={() => (mode = "crop")}><span class="i">⛶</span>Crop</button>
          <button class="fix" onclick={() => (mode = "straighten")}><span class="i">⟋</span>Straighten</button>
          <span></span>
          <button class="fix" onclick={() => apply("lucky")} title="Ctrl+Shift+E"><span class="i">✦</span>I'm Feeling Lucky</button>
          <button class="fix" onclick={() => apply("autocontrast")}><span class="i">◐</span>Auto Contrast</button>
          <button class="fix" onclick={() => apply("autocolor")}><span class="i">◑</span>Auto Color</button>
        </div>
        <label class="fill">
          <span>Fill Light</span>
          <input type="range" min="0" max="1" step="0.01" value={fill}
            oninput={(e) => fillInput(+e.currentTarget.value)}
            onchange={(e) => fillCommit(+e.currentTarget.value)} />
        </label>
        <div class="undo">
          <button disabled={!st?.undo || busy} onclick={() => run(() => api.undo(false))} title="Ctrl+Z">{st?.undo ? `Undo ${st.undo}` : "Undo"}</button>
          <button disabled={!st?.redo || busy} onclick={() => run(() => api.undo(true))} title="Ctrl+Y">{st?.redo ? `Redo ${st.redo}` : "Redo"}</button>
        </div>
        <button class="revert" disabled={!st?.edits.length || busy} onclick={() => apply("revert")}>Undo All Edits</button>
      {:else if mode === "crop"}
        <h3>Crop Photo</h3>
        <label class="row">Aspect
          <select bind:value={aspect} onchange={() => (crop = null)}>
            {#each ASPECTS as [label, r]}<option value={r}>{label}</option>{/each}
          </select>
        </label>
        <p class="hint">Drag on the photo to choose the area to keep. Drag inside it to move it.</p>
        {#if cropPx}<p class="hint">Result: {cropPx} pixels</p>{/if}
        <div class="undo">
          <button onclick={() => (crop = null)} disabled={!crop}>Reset</button>
          <button onclick={cropApply} disabled={!crop}>Apply</button>
          <button onclick={() => { crop = null; mode = "fixes"; }}>Cancel</button>
        </div>
      {:else}
        <h3>Straighten</h3>
        <p class="hint">Move the slider until the horizon lines up with the grid.</p>
        <div class="row">Angle <b>{angle.toFixed(1)}°</b></div>
        <input class="wide" type="range" min="-10" max="10" step="0.1" bind:value={angle} />
        <div class="undo">
          <button onclick={straightenApply} disabled={Math.abs(angle) < 0.05}>Apply</button>
          <button onclick={() => { angle = 0; mode = "fixes"; }}>Cancel</button>
        </div>
      {/if}

      <div class="info">
        <div class="ih">Histogram &amp; Camera Information</div>
        <canvas bind:this={histo} width="224" height="60"></canvas>
        <div class="cam">{photo?.camera ?? ""}</div>
        {#if st?.edits.length}<div class="edits">Edits: {st.edits.join(" › ")}</div>{/if}
      </div>
    </aside>

    <div class="frame" bind:clientWidth={frameW} bind:clientHeight={frameH}>
      <div class="pic" style:width="{disp.w}px" style:height="{disp.h}px">
        <canvas
          bind:this={canvas}
          style:width="{disp.w}px"
          style:height="{disp.h}px"
          style:transform={mode === "straighten" ? `rotate(${angle}deg) scale(${1 / straightenScale})` : ""}
        ></canvas>
        {#if mode === "straighten"}<div class="grid-overlay"></div>{/if}
        {#if mode === "crop"}
          <div class="cropzone" role="presentation" onpointerdown={cropDown} onpointermove={cropMove} onpointerup={cropUp}>
            {#if crop}
              <div class="shade" style:left="0" style:top="0" style:width="100%" style:height="{crop.y0}px"></div>
              <div class="shade" style:left="0" style:top="{crop.y1}px" style:width="100%" style:bottom="0"></div>
              <div class="shade" style:left="0" style:top="{crop.y0}px" style:width="{crop.x0}px" style:height="{crop.y1 - crop.y0}px"></div>
              <div class="shade" style:left="{crop.x1}px" style:top="{crop.y0}px" style:right="0" style:height="{crop.y1 - crop.y0}px"></div>
              <div class="box" style:left="{crop.x0}px" style:top="{crop.y0}px" style:width="{crop.x1 - crop.x0}px" style:height="{crop.y1 - crop.y0}px"></div>
            {/if}
          </div>
        {/if}
      </div>
      {#if busy}<div class="busy">Working…</div>{/if}
      {#if error}<div class="err">{error}</div>{/if}
    </div>
  </div>
</div>

<style>
  .edit { flex: 1; display: flex; flex-direction: column; min-width: 0; }
  .topbar { height: 52px; display: flex; align-items: center; gap: 16px; padding: 0 10px; border-bottom: 1px solid var(--chrome-line); }
  .back { height: 30px; }
  .strip { flex: 1; display: flex; justify-content: center; align-items: center; gap: 4px; }
  .film { width: 44px; height: 38px; padding: 1px; background: #333; border: 2px solid transparent; border-radius: 0; display: grid; place-items: center; }
  .film.cur { border-color: var(--thumb-sel); }
  .film img { max-width: 100%; max-height: 100%; }
  .arrow { border-radius: 50%; width: 30px; height: 30px; padding: 0; }
  .count { color: #666; width: 80px; text-align: right; }
  .body { flex: 1; display: flex; min-height: 0; }
  .panel { width: 290px; flex: none; background: #f6f5f1; border-right: 1px solid var(--chrome-line); display: flex; flex-direction: column; padding: 0 0 10px; overflow-y: auto; }
  .tabs { display: flex; border-bottom: 1px solid #ccc; background: #e9e6dd; }
  .tab { padding: 6px 12px; font-weight: 600; }
  .tab.on { background: #f6f5f1; border-right: 1px solid #ccc; }
  .fixes { display: grid; grid-template-columns: repeat(3, 1fr); gap: 10px 4px; padding: 14px 8px 6px; }
  .fix { display: flex; flex-direction: column; align-items: center; gap: 4px; background: none; border: 1px solid transparent; font-weight: 400; padding: 6px 2px; }
  .fix:hover { border-color: #c9c4b4; }
  .i { width: 44px; height: 32px; display: grid; place-items: center; font-size: 20px; color: #3d6b8f; background: linear-gradient(#fafafa, #dcdcdc); border: 1px solid #aaa; box-shadow: 1px 1px 2px #0003; }
  .fill { display: flex; flex-direction: column; align-items: center; gap: 2px; padding: 8px 20px; }
  .fill input { width: 100%; }
  .undo { display: flex; gap: 6px; padding: 8px; }
  .undo button { flex: 1; min-height: 28px; }
  .revert { margin: 0 8px; }
  h3 { margin: 12px 10px 6px; font-size: 15px; }
  .row { display: flex; justify-content: space-between; align-items: center; padding: 4px 10px; }
  .hint { color: #555; padding: 0 10px; margin: 6px 0; line-height: 1.4; }
  .wide { margin: 4px 10px; }
  .info { margin: auto 10px 0; background: #fff; border: 1px solid #ddd; padding: 6px 8px; }
  .ih { font-weight: 600; margin-bottom: 4px; }
  .info canvas { width: 224px; height: 60px; background: #fafafa; border-bottom: 1px solid #ccc; }
  .cam, .edits { color: #666; font-size: 12px; margin-top: 3px; }
  .frame { flex: 1; position: relative; display: grid; place-items: center; background: #d8d8d8; min-width: 0; overflow: hidden; }
  .pic { position: relative; overflow: hidden; background: #3c3c3c; box-shadow: 0 0 0 6px #4a4a4a, 0 4px 14px #0006; }
  .pic canvas { display: block; transform-origin: 50% 50%; }
  .grid-overlay { position: absolute; inset: 0; pointer-events: none;
    background-image: linear-gradient(#fff6 1px, transparent 1px), linear-gradient(90deg, #fff6 1px, transparent 1px);
    background-size: 32px 32px; }
  .cropzone { position: absolute; inset: 0; cursor: crosshair; }
  .shade { position: absolute; background: #000a; pointer-events: none; }
  .box { position: absolute; outline: 1px dashed #fff; cursor: move; pointer-events: none; }
  .busy { position: absolute; right: 12px; bottom: 10px; color: #444; font-size: 12px; }
  .err { position: absolute; left: 12px; bottom: 10px; color: #b00; background: #fff; padding: 4px 8px; }
</style>
