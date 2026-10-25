<script lang="ts">
  // Picture Collage (Create menu), with the settings Picasa 3.9's collage screen has (lab
  // check): six layouts, three picture borders, a solid colour or one of the pictures as the
  // background, its page formats (upright or across), shadows and captions; Scramble and
  // Shuffle. Create Collage saves it as a new JPEG (5120 pixels on the long side, as Picasa's
  // 16:10 collage was) in CasaPic's Collages folder; no photo is changed.
  import { store } from "../lib/store.svelte";
  import { api, renderUrl } from "../lib/api";

  let { ids, onclose }: { ids: number[]; onclose: () => void } = $props();

  const LAYOUTS = [
    ["pile", "Picture Pile: looks like a pile of scattered pictures"],
    ["mosaic", "Mosaic: automatically fit pictures into the page"],
    ["frame", "Frame Mosaic: a mosaic with a prominent centre picture"],
    ["grid", "Grid: arrange pictures into regular rows and columns"],
    ["contact", "Contact Sheet: thumbnails with an informative header"],
    ["exposure", "Multiple Exposure: superimpose pictures over one another"],
  ] as const;
  type Layout = (typeof LAYOUTS)[number][0];
  const FORMATS: [string, number][] = [
    ["Current display", screen.width / screen.height],
    ["4 × 6: Small print", 6 / 4],
    ["5 × 7: Large print", 7 / 5],
    ["8.5 × 11: Letter paper", 11 / 8.5],
    ["8 × 10", 10 / 8],
    ["A4: A4 paper", 297 / 210],
    ["Square: CD cover", 1],
    ["4:3: Standard screen", 4 / 3],
    ["16:10: Widescreen monitor", 16 / 10],
    ["16:9: HDTV", 16 / 9],
    ["5:3: Widescreen photo frame", 5 / 3],
  ];

  // svelte-ignore state_referenced_locally (made afresh each time it opens)
  let order = $state([...ids]);
  let layout = $state<Layout>("pile");
  let border = $state<"none" | "white" | "polaroid">("none");
  let bgImage = $state(false);
  let bgColor = $state("#ffffff");
  // svelte-ignore state_referenced_locally
  let bgPhoto = $state(ids[0]);
  let format = $state(8);
  let portrait = $state(false);
  let shadows = $state(true);
  let captions = $state(true);
  let seed = $state(1);
  let busy = $state(false);
  let error = $state("");

  const aspect = $derived(portrait ? 1 / FORMATS[format][1] : FORMATS[format][1]);

  // ---- the pictures ----
  type Pic = { id: number; img: HTMLImageElement; caption: string; name: string };
  let pics = $state<Record<number, Pic>>({});
  async function load(id: number, edge: number): Promise<HTMLImageElement> {
    const img = new Image();
    // the pictures come from the app's photo server, a different origin: without this the
    // canvas counts as "tainted" and refuses to give up the finished collage
    img.crossOrigin = "anonymous";
    img.src = renderUrl(id, edge, store.thumbV[id] ?? 0);
    await img.decode();
    return img;
  }
  /** loads every picture, `edgeOf(id)` pixels on its longer side, a few at a time so a big
   *  folder never decodes dozens of large pictures at once */
  async function loadAll(edgeOf: (id: number) => number, known?: Record<number, Pic>): Promise<Record<number, Pic>> {
    const out: Record<number, Pic> = {};
    const queue = [...ids];
    const worker = async () => {
      for (let id = queue.shift(); id != null; id = queue.shift()) {
        const k = known?.[id];
        if (k) {
          out[id] = { ...k, img: await load(id, edgeOf(id)) };
        } else {
          const [img, p] = await Promise.all([load(id, edgeOf(id)), api.photo(id)]);
          out[id] = { id, img, caption: p.caption, name: p.path.split(/[\\/]/).pop() ?? "" };
        }
      }
    };
    await Promise.all(Array.from({ length: 3 }, worker));
    return out;
  }
  loadAll(() => 1024).then((p) => (pics = p), (e) => (error = String(e)));

  // ---- layouts: boxes in page fractions ----
  type Box = { id: number; x: number; y: number; w: number; h: number; turn: number; cover: boolean };
  function rand(s: number) {
    // small seeded generator, so Scramble gives a new arrangement and redrawing keeps it
    let t = s * 2654435761;
    return () => {
      t = (t + 0x6d2b79f5) | 0;
      let r = Math.imul(t ^ (t >>> 15), 1 | t);
      r = (r + Math.imul(r ^ (r >>> 7), 61 | r)) ^ r;
      return ((r ^ (r >>> 14)) >>> 0) / 4294967296;
    };
  }
  function tile(list: number[], x: number, y: number, w: number, h: number, out: Box[]) {
    if (!list.length) return;
    if (list.length === 1) return void out.push({ id: list[0], x, y, w, h, turn: 0, cover: true });
    const a = Math.ceil(list.length / 2);
    const f = a / list.length;
    // split across the longer side (in page units, allowing for the page shape)
    if (w * aspect >= h) {
      tile(list.slice(0, a), x, y, w * f, h, out);
      tile(list.slice(a), x + w * f, y, w * (1 - f), h, out);
    } else {
      tile(list.slice(0, a), x, y, w, h * f, out);
      tile(list.slice(a), x, y + h * f, w, h * (1 - f), out);
    }
  }
  const boxes = $derived.by((): Box[] => {
    const list = order;
    const n = list.length;
    const out: Box[] = [];
    if (!n) return out;
    const r = rand(seed);
    if (layout === "grid" || layout === "contact") {
      const top = layout === "contact" ? 0.1 : 0;
      const cols = Math.max(1, Math.round(Math.sqrt((n * aspect) / (layout === "contact" ? 1 : 4 / 3))));
      const rows = Math.ceil(n / cols);
      const cw = 1 / cols;
      const ch = (1 - top) / rows;
      list.forEach((id, k) => out.push({ id, x: (k % cols) * cw, y: top + Math.floor(k / cols) * ch, w: cw, h: ch, turn: 0, cover: layout === "grid" }));
    } else if (layout === "mosaic") {
      tile(list, 0, 0, 1, 1, out);
    } else if (layout === "frame") {
      if (n === 1) tile(list, 0, 0, 1, 1, out);
      else {
        out.push({ id: list[0], x: 0.25, y: 0.25, w: 0.5, h: 0.5, turn: 0, cover: true });
        const rest = list.slice(1);
        // the four sides share the other pictures by their size
        const q = Math.ceil(rest.length / 4);
        tile(rest.slice(0, q), 0, 0, 1, 0.25, out);
        tile(rest.slice(q, 2 * q), 0, 0.75, 1, 0.25, out);
        tile(rest.slice(2 * q, 3 * q), 0, 0.25, 0.25, 0.5, out);
        tile(rest.slice(3 * q), 0.75, 0.25, 0.25, 0.5, out);
      }
    } else if (layout === "exposure") {
      list.forEach((id) => out.push({ id, x: 0, y: 0, w: 1, h: 1, turn: 0, cover: true }));
    } else {
      // pile: pictures about a third of the page, scattered and turned a little
      const s = Math.min(0.62, Math.max(0.28, 1.15 / Math.sqrt(n)));
      list.forEach((id) => {
        const p = pics[id];
        const ar = p ? p.img.naturalWidth / p.img.naturalHeight : 4 / 3;
        let w = s, h = (s * aspect) / ar;
        if (h > s) [w, h] = [(s * ar) / aspect, s];
        out.push({ id, x: 0.04 + r() * (0.92 - w), y: 0.04 + r() * (0.92 - h), w, h, turn: (r() - 0.5) * 30, cover: false });
      });
    }
    return out;
  });

  // ---- drawing ----
  function draw(c: HTMLCanvasElement, W: number, set: Record<number, Pic>) {
    const H = Math.round(W / aspect);
    c.width = W;
    c.height = H;
    const g = c.getContext("2d")!;
    g.fillStyle = bgColor;
    g.fillRect(0, 0, W, H);
    if (bgImage && set[bgPhoto]) drawImg(g, set[bgPhoto].img, 0, 0, W, H, true);
    const unit = Math.min(W, H);
    const gap = layout === "pile" || layout === "exposure" ? 0 : border === "none" ? unit * 0.004 : 0;
    if (layout === "contact") {
      g.fillStyle = "#333";
      g.font = `${Math.round(H * 0.035)}px Georgia, serif`;
      const f = store.folderOf.get(order[0]);
      g.fillText(`${f?.name ?? "Pictures"} · ${order.length} picture${order.length === 1 ? "" : "s"}`, W * 0.03, H * 0.065);
    }
    boxes.forEach((b, k) => {
      const p = set[b.id];
      if (!p) return;
      let x = b.x * W + gap, y = b.y * H + gap, w = b.w * W - 2 * gap, h = b.h * H - 2 * gap;
      if (layout === "contact") {
        // room for the file name under each small picture
        const pad = Math.min(w, h) * 0.08;
        x += pad; y += pad; w -= 2 * pad; h -= 2 * pad + unit * 0.03;
      }
      g.save();
      g.translate(x + w / 2, y + h / 2);
      g.rotate((b.turn * Math.PI) / 180);
      if (layout === "exposure") g.globalAlpha = 1 / (k + 1);
      const bw = border === "none" || layout === "exposure" ? 0 : Math.min(w, h) * 0.04;
      const foot = border === "polaroid" && layout !== "exposure" ? Math.min(w, h) * 0.16 : 0;
      // the picture fills the frame less its border
      let iw = w - 2 * bw, ih = h - 2 * bw - foot;
      if (!b.cover) {
        // whole picture: shrink the frame to the picture's shape
        const ar = p.img.naturalWidth / p.img.naturalHeight;
        if (iw / ih > ar) iw = ih * ar;
        else ih = iw / ar;
      }
      const fw = iw + 2 * bw, fh = ih + 2 * bw + foot;
      if (shadows && layout !== "exposure") {
        g.shadowColor = "rgba(0,0,0,0.45)";
        g.shadowBlur = unit * 0.012;
        g.shadowOffsetX = g.shadowOffsetY = unit * 0.004;
      }
      if (bw || foot) {
        g.fillStyle = border === "polaroid" ? "#f4f4f4" : "#ffffff";
        g.fillRect(-fw / 2, -fh / 2, fw, fh);
        g.shadowColor = "transparent";
      }
      drawImg(g, p.img, -fw / 2 + bw, -fh / 2 + bw, iw, ih, b.cover);
      g.shadowColor = "transparent";
      const text = layout === "contact" ? p.name : captions ? p.caption : "";
      if (text) {
        g.globalAlpha = 1;
        g.fillStyle = border === "polaroid" || layout === "contact" ? "#333" : "#fff";
        g.font = `${Math.max(10, Math.round((layout === "contact" ? unit * 0.022 : foot ? foot * 0.4 : Math.min(w, h) * 0.06)))}px "Segoe UI", sans-serif`;
        g.textAlign = "center";
        if (border !== "polaroid" && layout !== "contact") {
          g.shadowColor = "#000";
          g.shadowBlur = 4;
        }
        const ty = layout === "contact" ? fh / 2 + unit * 0.024 : foot ? fh / 2 - foot * 0.38 : fh / 2 - bw - Math.min(w, h) * 0.04;
        g.fillText(text, 0, ty, fw * 0.92);
      }
      g.restore();
    });
  }
  function drawImg(g: CanvasRenderingContext2D, img: HTMLImageElement, x: number, y: number, w: number, h: number, cover: boolean) {
    const ar = img.naturalWidth / img.naturalHeight;
    let sx = 0, sy = 0, sw = img.naturalWidth, sh = img.naturalHeight;
    if (cover) {
      if (w / h > ar) { sh = sw / (w / h); sy = (img.naturalHeight - sh) / 2; }
      else { sw = sh * (w / h); sx = (img.naturalWidth - sw) / 2; }
    }
    g.drawImage(img, sx, sy, sw, sh, x, y, w, h);
  }

  let view: HTMLCanvasElement | undefined = $state();
  let boxW = $state(0);
  let boxH = $state(0);
  $effect(() => {
    // redraw on any setting
    void [boxes, bgColor, bgImage, bgPhoto, border, shadows, captions, pics];
    if (!view || !boxW) return;
    const W = Math.min(boxW - 24, (boxH - 24) * aspect);
    if (W > 0) draw(view, Math.round(W * devicePixelRatio), pics);
  });

  function shuffle() {
    const r = rand(seed * 7 + order.length);
    const a = [...order];
    for (let k = a.length - 1; k > 0; k--) {
      const j = Math.floor(r() * (k + 1));
      [a[k], a[j]] = [a[j], a[k]];
    }
    order = a;
    seed++;
  }

  async function create() {
    busy = true;
    error = "";
    try {
      const W = aspect >= 1 ? 5120 : Math.round(5120 * aspect);
      // pictures at the size they take in the collage (the whole page for a background or
      // multiple exposure)
      const H = W / aspect;
      const need: Record<number, number> = {};
      for (const b of boxes) {
        // a filled box crops the picture, so allow for a picture up to a third longer
        const px = Math.max(b.w * W, b.h * H) * (b.cover ? 1.34 : 1);
        need[b.id] = Math.max(need[b.id] ?? 0, px);
      }
      if (bgImage) need[bgPhoto] = Math.max(W, H) * 1.34;
      const edgeOf = (id: number) => Math.min(4096, Math.max(256, Math.ceil((need[id] ?? 256) / 256) * 256));
      const big = await loadAll(edgeOf, pics);
      const c = document.createElement("canvas");
      draw(c, W, big);
      const blob: Blob = await new Promise((ok, no) => c.toBlob((b) => (b ? ok(b) : no(new Error("the collage could not be encoded"))), "image/jpeg", 0.92));
      const name = store.folderOf.get(order[0])?.name ?? "Collage";
      const path = await api.saveCollage(new Uint8Array(await blob.arrayBuffer()), name);
      store.flash(`Collage saved: ${path}`, 8000);
      onclose();
    } catch (e) {
      error = String(e);
    }
    busy = false;
  }
</script>

<svelte:window onkeydown={(e) => { if (e.key === "Escape" && !busy) onclose(); e.stopPropagation(); }} />

<div class="wrap">
  <aside>
    <h3>Picture Collage <span>· {order.length} picture{order.length === 1 ? "" : "s"}</span></h3>
    <label>Layout
      <select bind:value={layout}>{#each LAYOUTS as [k, l] (k)}<option value={k}>{l}</option>{/each}</select>
    </label>
    <div class="grp">Picture Borders
      <div class="row">
        <button class:on={border === "none"} onclick={() => (border = "none")}>None</button>
        <button class:on={border === "white"} onclick={() => (border = "white")}>White</button>
        <button class:on={border === "polaroid"} onclick={() => (border = "polaroid")}>Polaroid</button>
      </div>
    </div>
    <div class="grp">Background Options
      <label class="row"><input type="radio" bind:group={bgImage} value={false} /> Solid colour <input type="color" bind:value={bgColor} /></label>
      <label class="row"><input type="radio" bind:group={bgImage} value={true} /> Use image
        <select bind:value={bgPhoto} disabled={!bgImage}>{#each order as id (id)}<option value={id}>{pics[id]?.name ?? id}</option>{/each}</select>
      </label>
    </div>
    <label>Page Format
      <select bind:value={format}>{#each FORMATS as [l], k (k)}<option value={k}>{l}</option>{/each}</select>
    </label>
    <div class="row">
      <button class:on={!portrait} onclick={() => (portrait = false)}>Across</button>
      <button class:on={portrait} onclick={() => (portrait = true)}>Upright</button>
    </div>
    <label class="row"><input type="checkbox" bind:checked={shadows} /> Draw shadows</label>
    <label class="row"><input type="checkbox" bind:checked={captions} /> Show captions</label>
    {#if error}<p class="err">{error}</p>{/if}
    <div class="btns">
      <button class="primary" disabled={busy || !Object.keys(pics).length} onclick={create}>{busy ? "Creating…" : "Create Collage"}</button>
      <button disabled={busy} onclick={onclose}>Close</button>
    </div>
    <p class="note">Saved as a new picture in the Collages folder; your photos are not changed.</p>
  </aside>
  <div class="stage">
    <div class="canvasbox" bind:clientWidth={boxW} bind:clientHeight={boxH}>
      {#if !Object.keys(pics).length && !error}<p class="wait">Getting the pictures…</p>{/if}
      <canvas bind:this={view} style:width="{(view?.width ?? 0) / devicePixelRatio}px"></canvas>
    </div>
    <div class="under">
      <button disabled={layout !== "pile"} title="A new arrangement of the pile" onclick={() => seed++}>Scramble Collage</button>
      <button title="The pictures in a new order" onclick={shuffle}>Shuffle Pictures</button>
    </div>
  </div>
</div>

<style>
  .wrap { position: fixed; inset: 0; z-index: 60; display: flex; background: var(--chrome); }
  aside { width: 270px; padding: 12px; display: flex; flex-direction: column; gap: 12px; font-size: 12px; border-right: 1px solid var(--chrome-line); overflow: auto; }
  h3 { margin: 0; font-size: 15px; }
  h3 span { font-weight: 400; color: var(--muted); font-size: 12px; }
  label, .grp { display: flex; flex-direction: column; gap: 4px; font-weight: 600; }
  .row { display: flex; flex-direction: row; align-items: center; gap: 6px; font-weight: 400; }
  .row button { flex: 1; }
  button.on { background: var(--hilite); border-color: #6f9fd8; }
  select { max-width: 100%; }
  .btns { display: flex; gap: 8px; }
  .btns button { flex: 1; padding: 5px; }
  .primary { font-weight: 600; }
  .note { color: var(--muted); margin: 0; }
  .err { color: var(--error); margin: 0; }
  .stage { flex: 1; display: flex; flex-direction: column; min-width: 0; background: var(--photo-bg); }
  .canvasbox { flex: 1; display: grid; place-items: center; min-height: 0; position: relative; }
  canvas { box-shadow: 0 2px 10px #0005; }
  .wait { position: absolute; color: var(--text-2); }
  .under { display: flex; justify-content: center; gap: 8px; padding: 8px; }
</style>
