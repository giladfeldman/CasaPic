<script lang="ts">
  // Help ▸ Keyboard Shortcuts (F1): every menu command that has a key, read from the menus
  // themselves so the list cannot drift from what the keys do, plus the keys no menu shows.
  import type { Menu } from "./MenuBar.svelte";

  let { menus, onclose }: { menus: Menu[]; onclose: () => void } = $props();

  const fromMenus = $derived(
    menus
      .map((m) => ({
        group: m.label,
        rows: m.items.filter((i) => typeof i !== "string" && i.keys).map((i) => (typeof i === "string" ? ["", ""] : [i.keys!, i.label])),
      }))
      .filter((g) => g.rows.length),
  );
  const OTHER: { group: string; rows: string[][] }[] = [
    {
      group: "Library",
      rows: [
        ["Arrow keys", "Move between photos (Shift extends the selection)"],
        ["Home / End", "First / last photo"],
        ["Enter", "Open the photo in focus"],
        ["Ctrl+K", "Tags (as Ctrl+T)"],
        ["Ctrl+8", "Star / Unstar the selected photos"],
        ["Ctrl+Delete", "Delete from Disk (as Delete)"],
        ["Ctrl+Shift+B", "Black and White on the selected photos"],
        ["Ctrl+Shift+E", "I'm Feeling Lucky on the selected photos"],
      ],
    },
    {
      group: "Edit view (one photo)",
      rows: [
        ["← / →, Page Up / Page Down", "Previous / next photo"],
        ["Home / End", "First / last photo of the folder"],
        ["Esc", "Back to the Library (or leave Crop / Straighten)"],
        ["Ctrl+Z / Ctrl+Y", "Undo / Redo"],
        ["Ctrl+S", "Save"],
        ["Ctrl+8", "Star / Unstar"],
        ["Ctrl+Shift+E", "I'm Feeling Lucky"],
        ["Ctrl+Shift+B", "Black and White"],
        ["Delete, F2", "Delete / Rename this photo"],
      ],
    },
    {
      group: "Slideshow",
      rows: [
        ["← / →", "Previous / next"],
        ["Space", "Pause / play"],
        ["Home / End", "First / last"],
        ["Esc", "Exit"],
      ],
    },
  ];
</script>

<svelte:window onkeydown={(e) => { if (e.key === "Escape" || e.key === "F1") { e.preventDefault(); onclose(); } e.stopPropagation(); }} />

<div class="scrim" role="presentation" onclick={(e) => e.target === e.currentTarget && onclose()}>
  <div class="dlg" role="dialog" aria-label="Keyboard Shortcuts">
    <h2>Keyboard Shortcuts</h2>
    <div class="cols">
      {#each [...fromMenus, ...OTHER] as g (g.group)}
        <section>
          <h3>{g.group}</h3>
          <table>
            <tbody>
              {#each g.rows as [k, what] (k + what)}
                <tr><td class="k">{k}</td><td>{what}</td></tr>
              {/each}
            </tbody>
          </table>
        </section>
      {/each}
    </div>
    <div class="btns"><button onclick={onclose}>Close</button></div>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; background: #0004; display: grid; place-items: center; z-index: 60; }
  .dlg { background: var(--chrome); color: var(--text); border: 1px solid var(--border); box-shadow: 0 6px 24px #0005; padding: 14px 18px; width: min(920px, 94vw); max-height: 88vh; display: flex; flex-direction: column; }
  h2 { margin: 0 0 8px; font-size: 16px; }
  .cols { overflow: auto; columns: 2 380px; column-gap: 28px; }
  section { break-inside: avoid; margin-bottom: 12px; }
  h3 { margin: 0 0 4px; font-size: 13px; color: var(--link); }
  table { border-collapse: collapse; width: 100%; font-size: 12.5px; }
  td { padding: 2px 6px 2px 0; vertical-align: top; }
  .k { white-space: nowrap; font-weight: 600; width: 1%; padding-right: 14px; }
  .btns { text-align: right; margin-top: 8px; }
</style>
