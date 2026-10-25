<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { api } from "./lib/api";
  import { store } from "./lib/store.svelte";
  import MenuBar, { type Menu } from "./components/MenuBar.svelte";
  import FolderList from "./components/FolderList.svelte";
  import Grid from "./components/Grid.svelte";
  import Properties from "./components/Properties.svelte";
  import Tray from "./components/Tray.svelte";
  import EditView from "./components/EditView.svelte";
  import ExportDialog from "./components/ExportDialog.svelte";
  import FolderManager from "./components/FolderManager.svelte";
  import Ask from "./components/Ask.svelte";
  import OptionsDialog from "./components/OptionsDialog.svelte";
  import DateDialog from "./components/DateDialog.svelte";
  import AlbumPicker from "./components/AlbumPicker.svelte";
  import AlbumDialog from "./components/AlbumDialog.svelte";
  import TagsPanel from "./components/TagsPanel.svelte";
  import FilterBar from "./components/FilterBar.svelte";
  import PicasaImport from "./components/PicasaImport.svelte";
  import { open as pickFolder } from "@tauri-apps/plugin-dialog";
  import type { FolderSort } from "./lib/store.svelte";

  let grid: Grid | undefined = $state();
  let filter: FilterBar | undefined = $state();
  let picasaOpen = $state(false);
  /** the Folder Manager opened by itself on the very first launch (choice 4a) */
  let firstRun = $state(false);

  const frame = () => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));

  /** Speed check (build-1 step 5), only when the app was started with CASAPIC_TIMING. */
  async function timing() {
    const mode = await api.timingMode();
    if (mode === "off") return;
    store.timing = true;
    await frame();
    await api.mark("ui-ready");
    if (mode !== "bench") return;
    const ids = store.folders[0]?.photos.map((p) => p[0]) ?? [];
    const until = async (id: number) => { while (store.drawn !== id) await new Promise((r) => setTimeout(r, 1)); await frame(); };
    if (ids.length < 2) return api.quit();
    openEdit(ids[0]);
    await until(ids[0]);
    // photos 1-20: look at each for half a second before moving on (how people browse; the
    // neighbours are decoded meanwhile). Photos 21-40: press Next as soon as each one shows.
    for (const [i, id] of ids.slice(1, 41).entries()) {
      const rapid = i >= 20;
      if (!rapid) await new Promise((r) => setTimeout(r, 500));
      const t = performance.now();
      store.select(id);
      await until(id);
      await api.mark(`${rapid ? "next-photo-rapid-ms" : "next-photo-ms"} ${(performance.now() - t).toFixed(1)}`);
    }
    await api.quit();
  }

  onMount(() => {
    store.reload().then(timing);
    store.loadSettings();
    api.settings().then((s) => {
      if (!s.folders_confirmed) {
        firstRun = true;
        store.folderManager = true;
      }
    });
    const off = [
      listen("library-changed", () => store.reload()),
      listen<number[]>("thumbs-updated", (e) => store.thumbsChanged(e.payload)),
      listen<string>("status", (e) => store.backendStatus(e.payload)),
    ];
    return () => off.forEach((p) => p.then((f) => f()));
  });

  const one = () => store.picked()[0];
  const has = () => store.picked().length > 0;

  function openEdit(id = one()) {
    if (id == null) return;
    store.select(id);
    store.view = "edit";
  }
  async function addFolder() {
    const dir = await pickFolder({ directory: true, title: "Add Folder to CasaPic" });
    if (typeof dir === "string") await api.addFolder(dir).catch((e) => (store.message = String(e)));
  }
  const FOLDER_SORTS: [FolderSort, string][] = [["date", "Creation Date"], ["changed", "Recent Changes"], ["size", "Size"], ["name", "Name"]];

  function exportPicked() {
    const ids = store.picked();
    if (ids.length) store.exportFor = ids;
  }

  // Picasa's Picture > Batch Edit choices that need no settings
  const BATCH: [string, string][] = [
    ["I'm Feeling Lucky", "lucky"],
    ["Auto Contrast", "autocontrast"],
    ["Auto Color", "autocolor"],
    ["Sharpen", "unsharp2=1,0.6;"],
    ["Sepia", "sepia=1;"],
    ["Black and White", "bw=1;"],
    ["Warmify", "warm=1;"],
  ];

  // Only commands that work are listed (no placeholders).
  const menus: Menu[] = $derived([
    {
      label: "File",
      items: [
        { label: "Add Folder to CasaPic…", keys: "Ctrl+O", run: addFolder },
        { label: "New Album…", run: () => store.newAlbum([]) },
        "-",
        { label: "Save a Copy", run: () => store.saveCopy(), enabled: store.picked().length === 1 },
        { label: "Save As…", run: () => store.saveAs(), enabled: store.picked().length === 1 },
        { label: "Export Picture to Folder…", keys: "Ctrl+Shift+S", run: exportPicked, enabled: has() },
        "-",
        { label: "Rename…", keys: "F2", run: () => store.renamePicked(), enabled: store.picked().length === 1 },
        { label: "Delete from Disk", keys: "Del", run: () => store.deletePicked(), enabled: has() },
        { label: "Move to New Folder…", run: () => store.moveToNewFolder(), enabled: has() },
        { label: "Locate on Disk", keys: "Ctrl+Enter", run: () => api.locate(one()), enabled: has() },
        { label: "Open in an Editor", keys: "Ctrl+Shift+O", run: () => store.openWith("editor"), enabled: has() },
        { label: "Open With…", run: () => store.openWith("choose"), enabled: has() },
        { label: "Copy Full Path", run: () => store.copyPath(), enabled: has() },
        "-",
        { label: "Exit", run: () => api.quit() },
      ],
    },
    {
      label: "Edit",
      items: [
        { label: "Cut", keys: "Ctrl+X", run: () => store.cutOrCopy(true), enabled: has() },
        { label: "Copy", keys: "Ctrl+C", run: () => store.cutOrCopy(false), enabled: has() },
        { label: "Paste into Current Folder", keys: "Ctrl+V", run: () => store.paste(), enabled: !!store.clip && store.activeFolder != null },
        "-",
        { label: "Select All", keys: "Ctrl+A", run: () => store.selectAll() },
        { label: "Select Starred", run: () => store.selectStarred() },
        { label: "Invert Selection", keys: "Ctrl+I", run: () => store.invertSelection() },
        { label: "Clear Selection", keys: "Ctrl+D", run: () => store.clearSelection() },
        { label: "Hold in the Tray", keys: "Ctrl+H", run: () => store.toggleHold(), enabled: has(), checked: store.held.size > 0 },
        "-",
        { label: "Find…", keys: "Ctrl+F", run: () => { store.view = "library"; setTimeout(() => filter?.focus()); } },
      ],
    },
    {
      label: "View",
      items: [
        { label: "Library View", keys: "Esc", run: () => (store.view = "library"), checked: store.view === "library" },
        { label: "Edit View", keys: "Ctrl+3", run: () => openEdit(), enabled: has(), checked: store.view === "edit" },
        "-",
        { label: "Small Thumbnails", keys: "Ctrl+1", run: () => store.setThumbSize(80) },
        { label: "Normal Thumbnails", keys: "Ctrl+2", run: () => store.setThumbSize(140) },
        "-",
        { label: "Properties", keys: "Alt+Enter", run: () => store.toggleProperties(), checked: store.showProperties },
        { label: "Tags", keys: "Ctrl+T", run: () => store.toggleTags(), checked: store.showTags },
        { label: "Timeline", keys: "Ctrl+5", run: () => store.toggleTimeline(), checked: store.timeline },
        { label: "Hidden Pictures", run: () => store.toggleShowHidden(), checked: store.showHidden },
        "-",
        { label: "Folder View: Flat List", run: () => store.setFolderView("year"), checked: store.folderView === "year" },
        { label: "Folder View: Tree", run: () => store.setFolderView("tree"), checked: store.folderView === "tree" },
        ...FOLDER_SORTS.map(([k, label]) => ({ label: `Sort Folders by ${label}`, run: () => store.setFolderSort(k), checked: store.folderSort === k })),
        { label: "Sort Folders in Reverse", run: () => store.toggleFolderReverse(), checked: store.folderReverse },
      ],
    },
    {
      label: "Picture",
      items: [
        { label: "View and Edit", keys: "Ctrl+3", run: () => openEdit(), enabled: has() },
        { label: "Rotate Clockwise", keys: "Ctrl+R", run: () => store.rotate(1), enabled: has() },
        { label: "Rotate Counterclockwise", keys: "Ctrl+Shift+R", run: () => store.rotate(-1), enabled: has() },
        { label: "Star / Unstar", run: () => store.toggleStar(), enabled: has() },
        { label: "Adjust Date and Time…", run: () => (store.dateFor = store.picked()), enabled: has() },
        "-",
        { label: "Add to Album…", run: () => (store.albumPicker = { x: 220, y: 60, ids: store.picked() }), enabled: has() },
        { label: "Remove from Album", run: () => store.removeFromAlbum(), enabled: has() && !!store.album },
        { label: "Hide", run: () => store.hidePicked(true), enabled: has() && !store.picked().every((id) => store.hiddenIds.has(id)) },
        { label: "Unhide", run: () => store.hidePicked(false), enabled: has() && store.picked().some((id) => store.hiddenIds.has(id)) },
        "-",
        { label: "Copy All Effects", keys: "Ctrl+Shift+C", run: () => store.copyEffects(), enabled: store.picked().length === 1 },
        { label: "Paste All Effects", keys: "Ctrl+Shift+V", run: () => store.pasteEffects(), enabled: has() && !!store.effectsClip },
        "-",
        ...BATCH.map(([label, kind]) => ({ label: `Batch Edit: ${label}`, run: () => store.batch(kind, label), enabled: has() })),
      ],
    },
    {
      label: "Tools",
      items: [
        { label: "Folder Manager…", run: () => (store.folderManager = true) },
        { label: "Import Picasa Settings…", run: () => (picasaOpen = true) },
        { label: "Options…", run: () => (store.optionsOpen = true) },
      ],
    },
    {
      label: "Help",
      items: [
        {
          label: "About CasaPic",
          run: () =>
            (store.message =
              "CasaPic 0.1 — a personal photo organizer in the spirit of Picasa 3. Stars, edits and albums are kept beside your photos in a hidden .casapic.ini file. A photo file changes only when you Save it, adjust its date, or give it tags or a caption (written inside the file, as Picasa does); its untouched original is then kept in a hidden .casapicoriginals folder for Revert."),
        },
      ],
    },
  ]);

  function onKey(e: KeyboardEvent) {
    if ((e.target as HTMLElement)?.closest("input, select, textarea, dialog")) return;
    const k = e.key.toLowerCase();
    const ctrl = e.ctrlKey;
    if (store.view === "edit") return; // the Edit view has its own keys
    if (ctrl && k === "a") store.selectAll();
    else if (ctrl && k === "d") store.clearSelection();
    else if (ctrl && k === "i") store.invertSelection();
    else if (ctrl && k === "t") store.toggleTags();
    else if (ctrl && k === "f") filter?.focus();
    else if (ctrl && k === "5") store.toggleTimeline();
    else if (ctrl && k === "h") store.toggleHold();
    else if (ctrl && k === "r") store.rotate(e.shiftKey ? -1 : 1);
    else if (ctrl && e.shiftKey && k === "c") store.copyEffects();
    else if (ctrl && e.shiftKey && k === "v") store.pasteEffects();
    else if (ctrl && e.shiftKey && k === "o") store.openWith("editor");
    else if (ctrl && k === "x") store.cutOrCopy(true);
    else if (ctrl && k === "c") store.cutOrCopy(false);
    else if (ctrl && k === "v") store.paste();
    else if (k === "f2") store.renamePicked();
    else if (k === "delete") store.deletePicked();
    else if (ctrl && e.shiftKey && k === "s") exportPicked();
    else if (ctrl && !e.shiftKey && k === "o") addFolder();
    else if (ctrl && k === "1") store.setThumbSize(80);
    else if (ctrl && k === "2") store.setThumbSize(140);
    else if (ctrl && k === "3") openEdit();
    else if (ctrl && k === "enter") has() && api.locate(one());
    else if (e.altKey && k === "enter") store.toggleProperties();
    else if (k === "enter") openEdit();
    else if (["arrowleft", "arrowright", "arrowup", "arrowdown", "home", "end"].includes(k)) grid?.move(k, e.shiftKey);
    else return;
    e.preventDefault();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="app">
  <MenuBar {menus} />
  {#if store.view === "library"}
    <div class="main">
      <FolderList onpick={(id) => grid?.scrollToFolder(id)} />
      <div class="center">
        <FilterBar bind:this={filter} />
        <Grid bind:this={grid} onopen={openEdit} />
      </div>
      {#if store.showProperties}<Properties />{/if}
      {#if store.showTags}<TagsPanel />{/if}
    </div>
  {:else}
    <div class="main"><EditView /></div>
  {/if}
  <Tray onexport={exportPicked} />
  {#if store.prompt || store.confirm}<Ask />{/if}
  {#if store.albumPicker}<AlbumPicker />{/if}
  {#if store.albumDialog}<AlbumDialog />{/if}
  {#if picasaOpen}<PicasaImport onclose={() => (picasaOpen = false)} />{/if}
  {#if store.optionsOpen}<OptionsDialog onclose={() => (store.optionsOpen = false)} />{/if}
  {#if store.dateFor}<DateDialog ids={store.dateFor} onclose={() => (store.dateFor = null)} />{/if}
  {#if store.folderManager}
    <FolderManager first={firstRun} onclose={() => { store.folderManager = false; firstRun = false; }} />
  {/if}
  {#if store.exportFor}<ExportDialog ids={store.exportFor} onclose={() => (store.exportFor = null)} />{/if}
  {#if store.message}
    <div class="scrim" role="presentation" onclick={() => (store.message = null)}>
      <div class="msg" role="alertdialog">
        <p>{store.message}</p>
        <button onclick={() => (store.message = null)}>OK</button>
      </div>
    </div>
  {/if}
</div>

<style>
  .app { display: flex; flex-direction: column; height: 100%; }
  .main { flex: 1; display: flex; min-height: 0; }
  .center { flex: 1; display: flex; flex-direction: column; min-width: 0; }
  .scrim { position: fixed; inset: 0; background: #0004; display: grid; place-items: center; z-index: 50; }
  .msg { background: var(--chrome); border: 1px solid #999; padding: 16px 20px; max-width: 420px; box-shadow: 0 6px 24px #0005; text-align: right; }
  .msg p { text-align: left; margin: 0 0 14px; line-height: 1.45; }
</style>
