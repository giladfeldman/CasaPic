<script lang="ts">
  // The folder list: Picasa's flat list grouped by year, or a tree of the folders as they sit
  // on disk (View > Folder View). Right-click a folder for its own menu.
  import { store, type FolderSort } from "../lib/store.svelte";
  import { tick } from "svelte";
  import { api, type AlbumRow, type Folder } from "../lib/api";

  let { onpick }: { onpick: (folderId: number) => void } = $props();

  // Picasa groups the folder list under year headings when it is sorted by date.
  const groups = $derived.by(() => {
    if (store.folderSort !== "date") return [{ year: "", folders: store.sortedFolders }];
    const out: { year: string; folders: Folder[] }[] = [];
    for (const f of store.sortedFolders) {
      const year = f.date == null ? "Undated" : String(new Date(f.date * 1000).getUTCFullYear());
      if (out.at(-1)?.year !== year) out.push({ year, folders: [] });
      out.at(-1)!.folders.push(f);
    }
    return out;
  });

  // ---- tree view ----
  interface Node { name: string; path: string; folder?: Folder; children: Node[] }
  const sep = /[\\/]/;
  const tree = $derived.by(() => {
    const byPath = new Map<string, Node>();
    const tops: Node[] = [];
    const get = (path: string, isTop: boolean): Node => {
      const k = path.toLowerCase();
      let n = byPath.get(k);
      if (!n) {
        n = { name: isTop ? path.replace(/[\\/]+$/, "").split(sep).pop() || path : path.split(sep).pop()!, path, children: [] };
        byPath.set(k, n);
        if (isTop) tops.push(n);
      }
      return n;
    };
    for (const f of store.sortedFolders) {
      const root = f.root.replace(/[\\/]+$/, "");
      const top = get(root, true);
      // the folders between the start point and this one (they may hold no photos themselves)
      const rest = f.path.slice(root.length).split(sep).filter(Boolean);
      let parent = top, path = root;
      for (const part of rest) {
        path = `${path}\\${part}`;
        const k = path.toLowerCase();
        let n = byPath.get(k);
        if (!n) {
          n = { name: part, path, children: [] };
          byPath.set(k, n);
          parent.children.push(n);
        }
        parent = n;
      }
      parent.folder = f;
    }
    const sortRec = (ns: Node[]) => { ns.sort((a, b) => a.name.localeCompare(b.name)); ns.forEach((n) => sortRec(n.children)); };
    sortRec(tops);
    return tops;
  });
  let closed = $state<Record<string, boolean>>({});
  const count = (n: Node): number => (n.folder?.photos.length ?? 0) + n.children.reduce((s, c) => s + count(c), 0);

  // ---- right-click menu ----
  let menu = $state<{ x: number; y: number; f: Folder } | null>(null);
  function context(e: MouseEvent, f: Folder | undefined) {
    if (!f) return;
    e.preventDefault();
    menu = { x: e.clientX, y: e.clientY, f };
  }
  async function act(fn: () => unknown) {
    menu = null;
    try {
      await fn();
    } catch (e) {
      store.message = String(e);
    }
  }
  const SORTS: [string, string][] = [["", "Date"], ["name", "Name"], ["size", "Size"]];
  const FOLDER_SORTS: [FolderSort, string][] = [["date", "Creation Date"], ["changed", "Recent Changes"], ["size", "Size"], ["name", "Name"]];

  // photos dragged from the grid: move, or copy with Ctrl held
  let over = $state<string | null>(null);
  const DRAG = "application/x-casapic-photos";
  function dragOver(e: DragEvent, path: string) {
    if (!e.dataTransfer?.types.includes(DRAG)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = e.ctrlKey ? "copy" : "move";
    over = path;
  }
  function drop(e: DragEvent, path: string) {
    over = null;
    const raw = e.dataTransfer?.getData(DRAG);
    if (!raw) return;
    e.preventDefault();
    store.moveTo(JSON.parse(raw), path, e.ctrlKey);
  }

  async function pick(f: Folder | undefined) {
    if (!f) return;
    // leaving an open album or the timeline: the grid shows the folders again first
    if (store.album || store.timeline) {
      store.closeAlbum();
      store.timeline = false;
      await tick();
    }
    store.activeFolder = f.id;
    onpick(f.id);
  }

  // ---- albums ----
  let albumMenu = $state<{ x: number; y: number; a: AlbumRow } | null>(null);
  function dropOnAlbum(e: DragEvent, token: string) {
    over = null;
    const raw = e.dataTransfer?.getData(DRAG);
    if (!raw) return;
    e.preventDefault();
    store.addToAlbum(token, JSON.parse(raw));
  }

  /** the list keeps its scroll position while a photo is open (it is rebuilt on return) */
  function keepScroll(node: HTMLElement) {
    node.scrollTop = store.folderListScroll;
    const save = () => (store.folderListScroll = node.scrollTop);
    node.addEventListener("scroll", save, { passive: true });
    return { destroy: () => node.removeEventListener("scroll", save) };
  }
</script>

<svelte:window onclick={() => { menu = null; albumMenu = null; }} />

{#snippet row(f: Folder, indent: number)}
  <button class="row" style:padding-left="{indent}px" class:sel={store.activeFolder === f.id} class:over={over === f.path} title={f.path}
    onclick={() => pick(f)} oncontextmenu={(e) => context(e, f)}
    ondragover={(e) => dragOver(e, f.path)} ondragleave={() => (over = null)} ondrop={(e) => drop(e, f.path)}>
    <svg viewBox="0 0 16 13" width="16" height="13" aria-hidden="true"
      ><path d="M1 2h5l1.5 1.5H15v8.5H1z" fill="#f1c35c" stroke="#c8962e" /></svg
    >
    <span class:hiddenf={f.hidden}>{f.name} ({f.photos.length}){f.hidden ? " · hidden" : ""}</span>
  </button>
{/snippet}

{#snippet branch(n: Node, depth: number)}
  <div class="trow" style:padding-left="{depth * 14 + 2}px" class:sel={n.folder && store.activeFolder === n.folder.id} class:over={over === n.path}
    role="treeitem" aria-selected={!!n.folder && store.activeFolder === n.folder.id} tabindex="-1"
    ondragover={(e) => dragOver(e, n.path)} ondragleave={() => (over = null)} ondrop={(e) => drop(e, n.path)}>
    <button class="arrow" class:hide={!n.children.length} onclick={() => (closed = { ...closed, [n.path]: !closed[n.path] })}
      aria-label="Show or hide sub-folders">{closed[n.path] ? "▸" : "▾"}</button>
    <button class="tname" title={n.path} onclick={() => pick(n.folder)} oncontextmenu={(e) => context(e, n.folder)}>
      <svg viewBox="0 0 16 13" width="16" height="13" aria-hidden="true"
        ><path d="M1 2h5l1.5 1.5H15v8.5H1z" fill={n.folder ? "#f1c35c" : "#e8dcb8"} stroke="#c8962e" /></svg
      >
      <span>{n.name} ({n.folder ? n.folder.photos.length : count(n)})</span>
    </button>
  </div>
  {#if !closed[n.path]}
    {#each n.children as c (c.path)}{@render branch(c, depth + 1)}{/each}
  {/if}
{/snippet}

<aside class="list" use:keepScroll>
  <div class="cat">
    <span>▾ Albums ({store.albums.length})</span>
    <span class="views"><button title="New Album" onclick={() => store.newAlbum([])}>＋</button></span>
  </div>
  {#each store.albums as a (a.token)}
    <button class="row" style:padding-left="17px" class:sel={store.album?.token === a.token} class:over={over === `album:${a.token}`}
      title={a.description || a.name} onclick={() => store.openAlbum(a.token)}
      oncontextmenu={(e) => { e.preventDefault(); albumMenu = { x: e.clientX, y: e.clientY, a }; }}
      ondragover={(e) => dragOver(e, `album:${a.token}`)} ondragleave={() => (over = null)} ondrop={(e) => dropOnAlbum(e, a.token)}>
      <svg viewBox="0 0 16 13" width="16" height="13" aria-hidden="true"
        ><rect x="1.5" y="1" width="13" height="11" rx="1" fill="#6f9fd8" stroke="#3d6ea8" /><rect x="3.5" y="3" width="9" height="7" fill="#e8f0fa" /></svg
      >
      <span>{a.name} ({a.count})</span>
    </button>
  {:else}
    <p class="empty small">Select photos, then Add to Album in the tray.</p>
  {/each}
  <div class="cat">
    <span>▾ Folders ({store.sortedFolders.length})</span>
    <span class="views">
      <button class:on={store.folderView === "year"} title="Flat list by year" onclick={() => store.setFolderView("year")}>☰</button>
      <button class:on={store.folderView === "tree"} title="Tree of folders on disk" onclick={() => store.setFolderView("tree")}>⊢</button>
      <button class:on={store.timeline} title="Timeline: all photos by month (Ctrl+5)" onclick={() => store.toggleTimeline()}>◷</button>
    </span>
  </div>
  {#if store.folderView === "tree"}
    {#each tree as n (n.path)}{@render branch(n, 0)}{/each}
  {:else}
    {#each groups as g}
      {#if g.year}<div class="year">{g.year}</div>{/if}
      {#each g.folders as f (f.id)}{@render row(f, 17)}{/each}
    {/each}
  {/if}
  {#if store.sortedFolders.length === 0}
    <p class="empty">{store.status || "No photos found yet."}</p>
    <button class="add" onclick={() => (store.folderManager = true)}>Choose folders…</button>
  {/if}
</aside>

{#if albumMenu}
  {@const a = albumMenu.a}
  <div class="menu" style:left="{albumMenu.x}px" style:top="{albumMenu.y}px" role="menu">
    <button onclick={() => act(() => store.openAlbum(a.token))}>Open Album</button>
    {#if store.picked().length && store.album?.token !== a.token}
      <button onclick={() => act(() => store.addToAlbum(a.token))}>Add Selected Photos ({store.picked().length})</button>
    {/if}
    <hr />
    <button onclick={() => act(() => store.albumProperties(a))}>Album Properties…</button>
    <button onclick={() => act(() => store.deleteAlbum(a))}>Delete Album</button>
  </div>
{/if}

{#if menu}
  {@const f = menu.f}
  <div class="menu" style:left="{menu.x}px" style:top="{menu.y}px" role="menu">
    <button onclick={() => act(() => (store.editDescription = f.id))}>Edit Description…</button>
    {#if store.clip}<button onclick={() => act(() => store.paste(f.path))}>Paste ({store.clip.ids.length})</button>{/if}
    <hr />
    <button onclick={() => act(() => store.renameFolder(f))}>Rename Folder…</button>
    <button onclick={() => act(() => store.moveFolder(f))}>Move Folder…</button>
    <button onclick={() => act(() => store.newFolder(f))}>New Folder Inside…</button>
    <button onclick={() => act(() => store.deleteFolder(f))}>Delete Folder</button>
    <button onclick={() => act(() => store.hideFolder(f, !f.hidden))}>{f.hidden ? "Unhide Folder" : "Hide Folder"}</button>
    <hr />
    <div class="sub">Sort Photos By</div>
    {#each SORTS as [k, label]}
      <button class="ind" onclick={() => act(() => api.setFolderSort(f.path, k))}>{f.sort.replace("-", "") === k ? "✓ " : ""}{label}</button>
    {/each}
    <button class="ind" onclick={() => act(() => api.setFolderSort(f.path, f.sort.startsWith("-") ? f.sort.slice(1) : "-" + (f.sort || "date")))}
      >{f.sort.startsWith("-") ? "✓ " : ""}Reverse Order</button>
    <hr />
    <div class="sub">Sort Folders By</div>
    {#each FOLDER_SORTS as [k, label]}
      <button class="ind" onclick={() => act(() => store.setFolderSort(k))}>{store.folderSort === k ? "✓ " : ""}{label}</button>
    {/each}
    <button class="ind" onclick={() => act(() => store.toggleFolderReverse())}>{store.folderReverse ? "✓ " : ""}Reverse Order</button>
    <hr />
    <button onclick={() => act(() => api.openFolder(f.path))}>Locate on Disk</button>
    <button onclick={() => act(() => api.removeFolder(f.path))} title="Leaves the folder out of CasaPic; nothing on disk changes">Remove from CasaPic</button>
  </div>
{/if}

<style>
  .list { width: 236px; flex: none; background: var(--list-bg); border-right: 1px solid var(--chrome-line); overflow-y: auto; padding-bottom: 12px; }
  .cat { background: var(--cat-bg); border-bottom: 1px solid var(--border-mid); font-weight: 600; padding: 3px 8px; position: sticky; top: 0; z-index: 1; display: flex; justify-content: space-between; align-items: center; }
  .views button { border: none; background: none; padding: 0 4px; font-size: 13px; color: var(--muted); }
  .views button.on { color: var(--link); font-weight: 700; }
  .year { color: var(--muted); border-bottom: 1px solid var(--border-soft); margin: 8px 8px 2px; padding-bottom: 1px; }
  .row, .tname { display: flex; gap: 6px; align-items: center; width: 100%; height: 22px; border: none; border-radius: 0; background: none; font-weight: 400; font-size: 13px; color: var(--text); text-align: left; white-space: nowrap; overflow: hidden; }
  .row { padding: 0; }
  .row:hover:not(.sel), .trow:hover:not(.sel) { background: var(--list-hover) !important; }
  .row.sel, .trow.sel { background: var(--list-sel) !important; }
  .row.over, .trow.over { outline: 2px solid var(--list-sel); outline-offset: -2px; background: var(--hilite) !important; }
  .row.sel, .trow.sel .tname { color: #fff; }
  .row span, .tname span { overflow: hidden; text-overflow: ellipsis; }
  .trow { display: flex; align-items: center; height: 22px; }
  .tname { padding: 0 4px 0 0; flex: 1; min-width: 0; }
  .arrow { width: 16px; height: 18px; padding: 0; border: none; background: none; font-size: 10px; color: var(--muted); flex: none; }
  .arrow.hide { visibility: hidden; }
  .empty { color: var(--muted); padding: 8px 12px; }
  .hiddenf { font-style: italic; opacity: 0.7; }
  .empty.small { font-size: 12px; padding: 4px 12px 8px; margin: 0; }
  .add { margin: 0 12px; }
  .menu { position: fixed; z-index: 45; background: var(--surface); border: 1px solid var(--border); box-shadow: 2px 3px 10px #0003; padding: 3px 0; min-width: 210px; display: flex; flex-direction: column; }
  .menu button { border: none; border-radius: 0; background: none; text-align: left; padding: 4px 14px; font-weight: 400; font-size: 13px; }
  .menu button:hover { background: var(--list-sel) !important; color: #fff; }
  .menu .ind { padding-left: 26px; }
  .sub { color: var(--muted); font-size: 12px; padding: 3px 14px 1px; }
  hr { border: none; border-top: 1px solid var(--border-soft); margin: 3px 0; }
</style>
