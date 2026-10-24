<script lang="ts">
  // Edit view: one photo, Picasa's three editing tabs (Basic Fixes, Tuning, Effects), Undo/Redo
  // that name the action, a filmstrip of the folder. Every fix is rendered by the Rust core and
  // saved to the sidecar at once; sliders preview live without saving.
  import { store } from "../lib/store.svelte";
  import { api, fetchPreview, fxUrl, thumbUrl, type EditState, type Fix, type Photo } from "../lib/api";
  import { EFFECTS, NO_TUNE, defaults, parseTune, toHex, tuneText, type Effect, type Tune, type Values } from "../lib/effects";

  type Mode = "fixes" | "crop" | "straighten";
  type Tab = "basic" | "tune" | "fx";

  let mode = $state<Mode>("fixes");
  let tab = $state<Tab>("basic");
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
    fxOpen = null;
    pick = null;
    tune = { ...NO_TUNE };
    tuneSaved = false;
    fxV = 0;
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
      if (tab === "fx") refreshFx();
      if (store.timing) api.mark(`edit-front call=${(t1 - t0).toFixed(1)} fetch=${fetchMs.toFixed(1)} put=${(performance.now() - t1 - fetchMs).toFixed(1)}`);
      st = s;
      fill = s.fill;
      syncTune(s);
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

  function undo(redo: boolean) {
    fxOpen = null;
    run(() => api.undo(redo));
  }

  // ---- live previews (Tuning sliders, effect panels) ----
  // At most one preview call in flight; the newest request wins. `quick` previews slow effects
  // at half size while a slider moves; the full one ("Refining…") follows on release.
  let refining = $state(false);
  let tryPending: { text: string; replace: boolean; quick: boolean } | null = null;
  let tryBusy = false;
  async function preview(text: string, replace: boolean, quick: boolean) {
    tryPending = { text, replace, quick };
    if (tryBusy) return;
    tryBusy = true;
    while (tryPending) {
      const r = tryPending;
      tryPending = null;
      try {
        const s = await api.editTry(r.text, r.replace, r.quick);
        refining = s.quick;
        await draw(s);
        if (!s.quick) refining = false;
      } catch (e) {
        error = String(e);
      }
    }
    tryBusy = false;
  }
  async function settled() {
    while (tryBusy) await new Promise((r) => setTimeout(r, 10));
  }

  // ---- Tuning tab ----
  let tune = $state<Tune>({ ...NO_TUNE });
  /** whether this visit's Tuning entry is already in the recipe (then it is replaced) */
  let tuneSaved = $state(false);
  let pick = $state<null | "neutral" | "point">(null);

  // the sliders show the recipe's last fix when it is Tuning (moving them then adjusts that
  // entry, as in Picasa); otherwise they start from zero and add a new one
  function syncTune(s: EditState) {
    const t = parseTune(s.last);
    tune = t ?? { ...NO_TUNE };
    tuneSaved = t != null;
  }
  const tuneMoving = () => preview(tuneText(tune), tuneSaved, false);
  async function tuneCommit() {
    await settled();
    const same = JSON.stringify(tune) === JSON.stringify(NO_TUNE);
    if (same && !tuneSaved) return;
    await run(() => api.editAdd(tuneText(tune), tuneSaved));
    tuneSaved = true;
  }
  async function magic(kind: "light" | "color") {
    const skip = tuneSaved ? 1 : 0;
    const v = await api.editMagic(kind, skip);
    if (kind === "light") tune = { ...tune, highlights: +v[0].toFixed(4), shadows: +v[1].toFixed(4) };
    else tune = { ...tune, neutral: toHex(v) };
    await tuneCommit();
  }
  function leaveTab(next: Tab) {
    if (fxOpen) fxCancel();
    pick = null;
    if (st) syncTune(st);
    tab = next;
    if (next === "fx") refreshFx();
  }

  // ---- Effects tab ----
  let fxOpen = $state<Effect | null>(null);
  let fxVals = $state<Values>({});
  let fxV = $state(0);
  const FX_TILE = 72;
  async function refreshFx() {
    if (store.view !== "edit") return;
    try {
      fxV = await api.fxSheet(EFFECTS.map((e) => e.text(defaults(e))), Math.round(FX_TILE * devicePixelRatio));
    } catch (e) {
      error = String(e);
    }
  }
  function fxClick(e: Effect) {
    if (e.oneClick) {
      run(() => api.editAdd(e.text(defaults(e))));
      return;
    }
    fxOpen = e;
    fxVals = defaults(e);
    pick = e.point ? "point" : null;
    preview(e.text(fxVals), false, false);
  }
  const fxMoving = (quick: boolean) => fxOpen && preview(fxOpen.text(fxVals), false, quick);
  async function fxApply() {
    if (!fxOpen) return;
    const text = fxOpen.text(fxVals);
    fxOpen = null;
    pick = null;
    await settled();
    await run(() => api.editAdd(text));
  }
  async function fxCancel() {
    fxOpen = null;
    pick = null;
    refining = false;
    await settled();
    // redraw the photo as its recipe stands (nothing was saved while the panel was open)
    await run(() => api.editTry("", false, false));
  }

  // clicking the photo: neutral colour, or an effect's centre
  async function pickAt(e: PointerEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const x = (e.clientX - r.left) / r.width, y = (e.clientY - r.top) / r.height;
    if (pick === "neutral") {
      const c = await api.editSample(x, y, tuneSaved ? 1 : 0);
      tune = { ...tune, neutral: toHex(c) };
      pick = null;
      await tuneCommit();
    } else if (pick === "point" && fxOpen) {
      fxVals = { ...fxVals, x: +x.toFixed(4), y: +y.toFixed(4) };
      fxMoving(false);
    }
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
    store.editAdd = (text: string) => run(() => api.editAdd(text));
    return () => {
      store.editRotate = null;
      store.editAdd = null;
    };
  });

  // ---- Save / Revert ----
  async function save() {
    if (!st?.edits.length) return;
    leaveTab(tab);
    await run(() => api.editSave());
  }
  async function revertSaved() {
    leaveTab(tab);
    await run(() => api.editRevert());
  }

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
    else if (e.ctrlKey && k === "z" && st?.undo) undo(false);
    else if (e.ctrlKey && k === "y" && st?.redo) undo(true);
    else if (e.ctrlKey && k === "r") apply("rotate", [e.shiftKey ? -1 : 1]);
    else if (e.ctrlKey && e.shiftKey && k === "c" && id != null) store.copyEffects(id);
    else if (e.ctrlKey && e.shiftKey && k === "v") store.pasteEffects();
    else if (e.ctrlKey && !e.shiftKey && k === "s") save();
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
        <div class="tabs">
          <button class="tab" class:on={tab === "basic"} onclick={() => tab !== "basic" && leaveTab("basic")} title="Basic Fixes">🔧 Basic</button>
          <button class="tab" class:on={tab === "tune"} onclick={() => tab !== "tune" && leaveTab("tune")} title="Tuning">☀ Tuning</button>
          <button class="tab" class:on={tab === "fx"} onclick={() => tab !== "fx" && leaveTab("fx")} title="Effects">🖌 Effects</button>
        </div>
        {#if tab === "basic"}
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
        {:else if tab === "tune"}
          <div class="tune">
            <label class="slider"><span>Fill Light</span>
              <input type="range" min="0" max="1" step="0.01" bind:value={tune.fill} oninput={tuneMoving} onchange={tuneCommit} /></label>
            <button class="magic" onclick={() => magic("light")} title="Set Highlights and Shadows automatically">✦ Lighting</button>
            <label class="slider"><span>Highlights</span>
              <input type="range" min="0" max="0.5" step="0.005" bind:value={tune.highlights} oninput={tuneMoving} onchange={tuneCommit} /></label>
            <label class="slider"><span>Shadows</span>
              <input type="range" min="0" max="0.5" step="0.005" bind:value={tune.shadows} oninput={tuneMoving} onchange={tuneCommit} /></label>
            <label class="slider"><span>Color Temperature</span>
              <input type="range" min="-1" max="1" step="0.01" bind:value={tune.temp} oninput={tuneMoving} onchange={tuneCommit} /></label>
            <div class="neutral">
              <button class:armed={pick === "neutral"} onclick={() => (pick = pick === "neutral" ? null : "neutral")}
                title="Click a spot in the photo that should be white or grey">⌖ Neutral Color Picker</button>
              <button class="magic" onclick={() => magic("color")} title="Find the neutral colour automatically">✦ Color</button>
              {#if tune.neutral}<span class="swatch" style:background={tune.neutral} title={tune.neutral}></span>{/if}
            </div>
            {#if pick === "neutral"}<p class="hint">Click a part of the photo that should be white or grey.</p>{/if}
          </div>
        {:else if fxOpen}
          <h3>{fxOpen.label}</h3>
          {#if fxOpen.point}<p class="hint">{fxOpen.point}</p>{/if}
          {#each fxOpen.sliders ?? [] as s (s.key)}
            <label class="slider"><span>{s.label}</span>
              <input type="range" min={s.min} max={s.max} step={s.step} bind:value={fxVals[s.key]}
                oninput={() => fxMoving(true)} onchange={() => fxMoving(false)} /></label>
          {/each}
          {#if fxOpen.color}
            <label class="row">{fxOpen.color.label}
              <input type="color" bind:value={fxVals.color} onchange={() => fxMoving(false)} /></label>
          {/if}
          {#if fxOpen.lighten}
            <label class="row">Lighten <input type="checkbox" checked={!!fxVals.lighten} onchange={(e) => { fxVals = { ...fxVals, lighten: e.currentTarget.checked }; fxMoving(false); }} /></label>
          {/if}
          {#if refining}<p class="hint">Refining…</p>{/if}
          <div class="undo">
            <button onclick={fxApply}>Apply</button>
            <button onclick={fxCancel}>Cancel</button>
          </div>
        {:else}
          <div class="fxgrid">
            {#each EFFECTS as e, i (e.id)}
              <button class="fx" onclick={() => fxClick(e)}>
                <span class="thumb" style:background-image={fxV ? `url(${fxUrl(fxV)})` : "none"}
                  style:background-size="{EFFECTS.length * FX_TILE}px {FX_TILE}px"
                  style:background-position="{-i * FX_TILE}px 0"></span>
                {e.label}
              </button>
            {/each}
          </div>
        {/if}
        {#if !fxOpen}
          <div class="undo">
            <button disabled={!st?.undo || busy} onclick={() => undo(false)} title="Ctrl+Z">{st?.undo ? `Undo ${st.undo}` : "Undo"}</button>
            <button disabled={!st?.redo || busy} onclick={() => undo(true)} title="Ctrl+Y">{st?.redo ? `Redo ${st.redo}` : "Redo"}</button>
          </div>
          <button class="revert" disabled={!st?.edits.length || busy} onclick={() => { leaveTab(tab); apply("revert"); }}>Undo All Edits</button>
          <div class="undo">
            <button disabled={!st?.edits.length || busy} onclick={save} title="Ctrl+S: write the edits into the photo file; the original is kept">Save</button>
            <button disabled={!st?.saved || busy} onclick={revertSaved} title="Put back the original photo as it was before the first Save">Revert</button>
          </div>
          {#if st?.saved}<p class="hint">Saved. The original is kept in a hidden .casapicoriginals folder; Revert puts it back.</p>{/if}
        {/if}
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
        <!-- Options > "single click on the photo goes back to the Library" -->
        <canvas
          bind:this={canvas}
          onclick={() => { if (store.options?.single_click_exit_edit && mode === "fixes" && !pick) store.view = "library"; }}
          style:width="{disp.w}px"
          style:height="{disp.h}px"
          style:transform={mode === "straighten" ? `rotate(${angle}deg) scale(${1 / straightenScale})` : ""}
        ></canvas>
        {#if mode === "straighten"}<div class="grid-overlay"></div>{/if}
        {#if pick && mode === "fixes"}
          <div class="pickzone" role="presentation" onpointerdown={pickAt}></div>
          {#if pick === "point" && fxOpen}
            <div class="marker" style:left="{+fxVals.x * 100}%" style:top="{+fxVals.y * 100}%"></div>
          {/if}
        {/if}
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
  .tab { padding: 6px 10px; font-weight: 600; background: none; border: none; border-right: 1px solid #ccc; border-radius: 0; }
  .tab.on { background: #f6f5f1; }
  .tune { padding: 8px 10px; display: flex; flex-direction: column; gap: 6px; }
  .slider { display: flex; flex-direction: column; gap: 2px; padding: 2px 10px; }
  .tune .slider { padding: 2px 0; }
  .slider input { width: 100%; }
  .magic { align-self: flex-start; }
  .neutral { display: flex; align-items: center; gap: 6px; flex-wrap: wrap; }
  .armed { outline: 2px solid var(--thumb-sel); }
  .swatch { width: 20px; height: 20px; border: 1px solid #888; }
  .fxgrid { display: grid; grid-template-columns: repeat(3, 1fr); gap: 8px 4px; padding: 10px 6px; }
  .fx { display: flex; flex-direction: column; align-items: center; gap: 3px; background: none; border: 1px solid transparent; font-weight: 400; padding: 4px 2px; font-size: 12px; }
  .fx:hover { border-color: #c9c4b4; }
  .thumb { width: 72px; height: 72px; background-color: #ccc; background-repeat: no-repeat; border: 1px solid #999; box-shadow: 1px 1px 2px #0003; }
  .pickzone { position: absolute; inset: 0; cursor: crosshair; }
  .marker { position: absolute; width: 14px; height: 14px; margin: -8px 0 0 -8px; border: 2px solid #fff; border-radius: 50%; box-shadow: 0 0 2px #000; pointer-events: none; }
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
