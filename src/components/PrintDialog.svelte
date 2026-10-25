<script lang="ts">
  // Print (Ctrl+P) and Print Contact Sheet (Ctrl+Shift+P): lays the photos out on pages (a
  // preview here), then hands the pages to Windows' print dialog, where the printer, paper
  // and copies are chosen. Photos print with their edits, rendered by the core.
  import { onDestroy } from "svelte";
  import { store } from "../lib/store.svelte";
  import { api, fmtDay, renderUrl } from "../lib/api";

  let { ids, contact = false, onclose }: { ids: number[]; contact?: boolean; onclose: () => void } = $props();

  type Size = { label: string; w: number; h: number };
  const IN = 25.4;
  // print sizes in millimetres (Picasa's list: full page and the usual photo sizes)
  const SIZES: Size[] = [
    { label: "Full page", w: 0, h: 0 },
    { label: "8 × 10 in", w: 8 * IN, h: 10 * IN },
    { label: "5 × 7 in", w: 5 * IN, h: 7 * IN },
    { label: "4 × 6 in", w: 4 * IN, h: 6 * IN },
    { label: "3.5 × 5 in", w: 3.5 * IN, h: 5 * IN },
    { label: "Wallet (2.5 × 3.5 in)", w: 2.5 * IN, h: 3.5 * IN },
  ];
  const PAPERS = { A4: { w: 210, h: 297 }, Letter: { w: 8.5 * IN, h: 11 * IN } } as const;
  type Paper = keyof typeof PAPERS;

  // svelte-ignore state_referenced_locally (the dialog is made afresh each time it opens)
  let layout = $state(contact ? -1 : 0); // index into SIZES; -1 = contact sheet
  let paper = $state<Paper>(navigator.language.startsWith("en-US") ? "Letter" : "A4");
  let crop = $state(false);
  let copies = $state(1);
  // svelte-ignore state_referenced_locally (the dialog is made afresh each time it opens)
  let captions = $state(contact);
  let columns = $state(5);
  let busy = $state(false);
  const MARGIN = 8;
  const GAP = 4;

  let info = $state<Record<number, { name: string; caption: string }>>({});
  // svelte-ignore state_referenced_locally (the dialog is made afresh each time it opens)
  Promise.all(ids.map((id) => api.photo(id).then((p) => [id, { name: p.path.split(/[\\/]/).pop() ?? "", caption: p.caption }] as const)))
    .then((rows) => (info = Object.fromEntries(rows)), (e) => (store.message = String(e)));

  type Cell = { id: number; x: number; y: number; w: number; h: number };
  type Page = { cells: Cell[]; header?: string };

  /** the pages, in millimetres from the paper's corner */
  const pages = $derived.by((): Page[] => {
    const P = PAPERS[paper];
    const W = P.w - 2 * MARGIN;
    const H = P.h - 2 * MARGIN;
    const list = ids.flatMap((id) => Array(layout === -1 ? 1 : Math.max(1, copies)).fill(id) as number[]);
    if (layout === -1) {
      // contact sheet: a header, then a grid of small pictures with their names
      const head = 12;
      const cw = (W - (columns - 1) * GAP) / columns;
      const ch = cw + (captions ? 6 : 0);
      const rows = Math.max(1, Math.floor((H - head + GAP) / (ch + GAP)));
      const per = rows * columns;
      const out: Page[] = [];
      const title = store.folderOf.get(ids[0])?.name ?? "Photos";
      for (let k = 0; k < list.length; k += per) {
        const cells = list.slice(k, k + per).map((id, j) => ({
          id,
          x: MARGIN + (j % columns) * (cw + GAP),
          y: MARGIN + head + Math.floor(j / columns) * (ch + GAP),
          w: cw,
          h: cw,
        }));
        out.push({ cells, header: `${title} · ${ids.length} picture${ids.length === 1 ? "" : "s"} · page ${out.length + 1}` });
      }
      return out;
    }
    const s = SIZES[layout];
    let bw = s.w, bh = s.h, cols = 1, rows = 1;
    if (!s.w) {
      bw = W;
      bh = H;
    } else {
      // as many as fit, with the box upright or turned, whichever fits more
      const fit = (w: number, h: number) => [Math.floor((W + GAP) / (w + GAP)), Math.floor((H + GAP) / (h + GAP))];
      const [c1, r1] = fit(s.w, s.h);
      const [c2, r2] = fit(s.h, s.w);
      if (c2 * r2 > c1 * r1) [bw, bh, cols, rows] = [s.h, s.w, c2, r2];
      else [cols, rows] = [c1, r1];
      cols = Math.max(1, cols);
      rows = Math.max(1, rows);
    }
    const per = cols * rows;
    const ox = MARGIN + (W - (cols * bw + (cols - 1) * GAP)) / 2;
    const oy = MARGIN + (H - (rows * bh + (rows - 1) * GAP)) / 2;
    const out: Page[] = [];
    for (let k = 0; k < list.length; k += per) {
      out.push({
        cells: list.slice(k, k + per).map((id, j) => ({ id, x: ox + (j % cols) * (bw + GAP), y: oy + Math.floor(j / cols) * (bh + GAP), w: bw, h: bh })),
      });
    }
    return out;
  });

  const P = $derived(PAPERS[paper]);
  // preview scale: pixels per millimetre
  const k = $derived(300 / P.w);
  // rendered size per picture: enough for about 300 dpi at the box size, within limits
  const edgeFor = (c: Cell) => Math.min(4096, Math.max(512, Math.ceil(((Math.max(c.w, c.h) / IN) * 300) / 256) * 256));
  const src = (c: Cell, preview: boolean) => renderUrl(c.id, preview ? 512 : edgeFor(c), store.thumbV[c.id] ?? 0);

  // the pages for the printer: a copy placed beside the app only while printing (moving the
  // dialog's own element out would upset Svelte's clean-up when the dialog closes)
  let sheet: HTMLDivElement | undefined = $state();
  let pageStyle: HTMLStyleElement | null = null;
  async function print() {
    if (!sheet) return;
    busy = true;
    const copy = sheet.cloneNode(true) as HTMLDivElement;
    copy.classList.add("print-copy");
    document.body.appendChild(copy);
    try {
      await Promise.all([...copy.querySelectorAll("img")].map((img) => img.decode().catch(() => {})));
      pageStyle?.remove();
      pageStyle = document.createElement("style");
      pageStyle.textContent = `@page { size: ${paper === "A4" ? "A4" : "letter"} portrait; margin: 0; }`;
      document.head.appendChild(pageStyle);
      document.body.classList.add("printing");
      window.print();
    } finally {
      document.body.classList.remove("printing");
      copy.remove();
      busy = false;
    }
  }
  onDestroy(() => pageStyle?.remove());
</script>

<svelte:window onkeydown={(e) => { if (e.key === "Escape") onclose(); e.stopPropagation(); }} />

<div class="scrim" role="presentation">
  <div class="dlg" role="dialog" aria-label={contact ? "Print Contact Sheet" : "Print"}>
    <header>{layout === -1 ? "Print Contact Sheet" : "Print"} · {ids.length} picture{ids.length === 1 ? "" : "s"}</header>
    <div class="body">
      <div class="side">
        <label>Layout
          <select bind:value={layout}>
            {#each SIZES as s, i (i)}<option value={i}>{s.label}</option>{/each}
            <option value={-1}>Contact sheet</option>
          </select>
        </label>
        <label>Paper
          <select bind:value={paper}><option>A4</option><option>Letter</option></select>
        </label>
        {#if layout === -1}
          <label>Pictures per row <input type="number" min="2" max="10" bind:value={columns} /></label>
          <label class="row"><input type="checkbox" bind:checked={captions} /> File names under the pictures</label>
        {:else}
          <label class="row"><input type="radio" bind:group={crop} value={false} /> Shrink to fit</label>
          <label class="row"><input type="radio" bind:group={crop} value={true} /> Crop to fit</label>
          <label>Copies of each picture <input type="number" min="1" max="20" bind:value={copies} /></label>
          <label class="row"><input type="checkbox" bind:checked={captions} /> Captions under the pictures</label>
        {/if}
        <p class="note">{pages.length} page{pages.length === 1 ? "" : "s"}. The printer, paper tray and number of sheets are chosen in the Windows print dialog that opens next.</p>
      </div>
      <div class="previews">
        {#each pages as pg, pi (pi)}
          <div class="page" style:width="{P.w * k}px" style:height="{P.h * k}px">
            {#if pg.header}<div class="ph" style:left="{MARGIN * k}px" style:top="{MARGIN * k}px" style:font-size="{3.2 * k}px">{pg.header}</div>{/if}
            {#each pg.cells as c, ci (ci)}
              <img src={src(c, true)} alt="" class:cover={crop && layout !== -1} class:contact={layout === -1}
                style:left="{c.x * k}px" style:top="{c.y * k}px" style:width="{c.w * k}px" style:height="{c.h * k}px" />
              {#if captions}
                <div class="cap" style:left="{c.x * k}px" style:top="{(c.y + c.h + 0.5) * k}px" style:width="{c.w * k}px" style:font-size="{2.4 * k}px">
                  {layout === -1 ? info[c.id]?.name ?? "" : info[c.id]?.caption ?? ""}
                </div>
              {/if}
            {/each}
          </div>
        {/each}
      </div>
    </div>
    <div class="btns">
      <button disabled={busy || !pages.length} onclick={print}>{busy ? "Preparing…" : "Print…"}</button>
      <button onclick={onclose}>Close</button>
    </div>
  </div>
</div>

<!-- what goes to the printer: real sizes in millimetres -->
<div class="sheet" bind:this={sheet}>
  {#each pages as pg, pi (pi)}
    <div class="ppage" style:width="{P.w}mm" style:height="{P.h}mm">
      {#if pg.header}<div class="ph" style:left="{MARGIN}mm" style:top="{MARGIN}mm" style:font-size="3.2mm">{pg.header} · {fmtDay(Date.now() / 1000)}</div>{/if}
      {#each pg.cells as c, ci (ci)}
        <img src={src(c, false)} alt="" class:cover={crop && layout !== -1} class:contact={layout === -1}
          style:left="{c.x}mm" style:top="{c.y}mm" style:width="{c.w}mm" style:height="{c.h}mm" />
        {#if captions}
          <div class="cap" style:left="{c.x}mm" style:top="{c.y + c.h + 0.5}mm" style:width="{c.w}mm" style:font-size="2.4mm">
            {layout === -1 ? info[c.id]?.name ?? "" : info[c.id]?.caption ?? ""}
          </div>
        {/if}
      {/each}
    </div>
  {/each}
</div>

<style>
  .scrim { position: fixed; inset: 0; background: #0004; display: grid; place-items: center; z-index: 50; }
  .dlg { background: var(--chrome); border: 1px solid var(--border); width: min(980px, 94vw); height: min(720px, 90vh); box-shadow: 0 6px 24px #0005; display: flex; flex-direction: column; }
  header { background: linear-gradient(#9a9a9a, #7c7c7c); color: #fff; font-weight: 600; padding: 5px 10px; }
  .body { flex: 1; display: flex; min-height: 0; }
  .side { width: 230px; padding: 12px; display: flex; flex-direction: column; gap: 10px; font-size: 12px; border-right: 1px solid var(--chrome-line); }
  .side label { display: flex; flex-direction: column; gap: 3px; font-weight: 600; }
  .side label.row { flex-direction: row; align-items: center; gap: 6px; font-weight: 400; }
  .side input[type="number"] { width: 70px; }
  .note { color: var(--text-2); font-weight: 400; }
  .previews { flex: 1; overflow: auto; padding: 16px; display: flex; flex-wrap: wrap; gap: 16px; align-content: flex-start; background: var(--photo-bg-2); }
  .page { position: relative; background: var(--surface); box-shadow: 0 2px 6px #0004; flex: none; overflow: hidden; }
  .page img, .ppage img { position: absolute; object-fit: contain; }
  .page img.cover, .ppage img.cover { object-fit: cover; }
  /* contact sheet: each picture sits just above its file name */
  .page img.contact, .ppage img.contact { object-position: center bottom; }
  .ph { position: absolute; color: #333; white-space: nowrap; }
  .cap { position: absolute; text-align: center; color: #333; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .btns { display: flex; justify-content: flex-end; gap: 8px; padding: 10px 16px; border-top: 1px solid var(--chrome-line); }
  .sheet { display: none; }
  .ppage { position: relative; overflow: hidden; break-after: page; }
  @media print {
    /* the app fills one window and does not scroll; the printout must run over many pages */
    :global(html), :global(body.printing) { height: auto !important; overflow: visible !important; background: #fff !important; }
    :global(body.printing > #app) { display: none !important; }
    :global(body.printing > .print-copy) { display: block !important; }
  }
</style>
