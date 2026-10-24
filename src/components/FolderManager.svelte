<script lang="ts">
  // Tools > Folder Manager, laid out like Picasa's: a tree of your folders and drives on the
  // left; on the right, what CasaPic does with the folder picked in the tree (Scan Always,
  // Scan Once, Remove) and the list of folders it watches. Nothing changes until OK.
  import { api, type FmNode, type Rule, type Scan } from "../lib/api";

  let { first = false, onclose }: { first?: boolean; onclose: () => void } = $props();

  let rules = $state<Rule[]>([]);
  let top = $state<FmNode[]>([]);
  let kids = $state<Record<string, FmNode[]>>({});
  let open = $state<Record<string, boolean>>({});
  let current = $state<string | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);

  const norm = (p: string) => p.replace(/[\\/]+$/, "").replace(/\//g, "\\").toLowerCase();
  const within = (p: string, base: string) => norm(p) === norm(base) || norm(p).startsWith(norm(base) + "\\");
  /** the rule that applies to a folder: the nearest marked folder at or above it */
  function ruleOf(p: string): Rule | undefined {
    return rules.filter((r) => within(p, r.path)).sort((a, b) => b.path.length - a.path.length)[0];
  }
  const own = (p: string) => rules.find((r) => norm(r.path) === norm(p));
  function mark(p: string, scan: Scan | null) {
    rules = rules.filter((r) => norm(r.path) !== norm(p));
    if (scan) rules = [...rules, { path: p, scan }];
  }

  const ICON: Record<Scan, [string, string]> = {
    always: ["◉", "Scan Always: in CasaPic and watched for changes"],
    once: ["✓", "Scan Once: in CasaPic, not watched"],
    remove: ["✕", "Removed from CasaPic"],
  };

  async function load() {
    const s = await api.settings();
    rules = s.folders.map((r) => ({ ...r }));
    top = await api.fmList(null);
  }
  load().catch((e) => (error = String(e)));

  async function toggle(n: FmNode) {
    open = { ...open, [n.path]: !open[n.path] };
    if (open[n.path] && !kids[n.path]) kids = { ...kids, [n.path]: await api.fmList(n.path) };
  }

  /** what the right side shows for the picked folder */
  const pickedRule = $derived(current ? ruleOf(current) : undefined);
  const pickedOwn = $derived(current ? own(current) : undefined);
  const choice = $derived<Scan | "inherit" | "none">(pickedOwn ? pickedOwn.scan : pickedRule ? "inherit" : "none");
  const watched = $derived(rules.filter((r) => r.scan === "always").map((r) => r.path).sort());

  async function ok() {
    busy = true;
    try {
      await api.fmSave(rules);
      onclose();
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }
  async function cancel() {
    if (first) await api.fmConfirm().catch(() => {});
    onclose();
  }
</script>

{#snippet node(n: FmNode, depth: number)}
  {@const r = ruleOf(n.path)}
  {@const mine = own(n.path)}
  <div class="node" style:padding-left="{depth * 16 + 4}px" class:cur={current === n.path}>
    <button class="arrow" class:hide={!n.more} onclick={() => toggle(n)} aria-label="Show sub-folders">{open[n.path] ? "▾" : "▸"}</button>
    <button class="name" onclick={() => (current = n.path)} title={n.path}>
      <span class="st" class:faint={!mine} title={r ? ICON[r.scan][1] : "Not in CasaPic"}>{r ? ICON[r.scan][0] : ""}</span>
      {n.name}
    </button>
  </div>
  {#if open[n.path]}
    {#each kids[n.path] ?? [] as k (k.path)}{@render node(k, depth + 1)}{/each}
  {/if}
{/snippet}

<div class="scrim" role="presentation">
  <div class="dlg" role="dialog" aria-label="Folder Manager">
    <h2>Folder Manager</h2>
    {#if first}
      <p class="intro">
        These are the folders CasaPic shows. It starts with your Pictures folder. Pick any folder on the left to
        add it or leave it out. CasaPic only reads your folders; it never moves files to organize them.
      </p>
    {/if}
    <div class="body">
      <div class="tree">
        {#each top as n (n.path)}{@render node(n, 0)}{/each}
      </div>
      <div class="side">
        <div class="box">
          <div class="h">For the current folder:</div>
          {#if current}
            <div class="path">{current}</div>
            <label><input type="radio" checked={choice === "once"} onchange={() => mark(current!, "once")} /> Scan Once
              <span class="hint">add it now, without watching it for changes</span></label>
            <label><input type="radio" checked={choice === "remove"} onchange={() => mark(current!, "remove")} /> Remove from CasaPic
              <span class="hint">leave it and its sub-folders out (nothing on disk changes)</span></label>
            <label><input type="radio" checked={choice === "always"} onchange={() => mark(current!, "always")} /> Scan Always
              <span class="hint">add it and keep it up to date as files change</span></label>
            {#if pickedOwn && ruleOf(current.replace(/[\\/][^\\/]+[\\/]?$/, ""))}
              <button class="link" onclick={() => mark(current!, null)}>Same as the folder above it</button>
            {:else if choice === "inherit"}
              <p class="hint">Follows {pickedRule?.path} ({pickedRule ? ICON[pickedRule.scan][1] : ""}).</p>
            {/if}
          {:else}
            <p class="hint">Pick a folder in the tree.</p>
          {/if}
        </div>
        <div class="box">
          <div class="h">Watched Folders</div>
          {#each watched as w}<div class="w" title={w}>{w}</div>{:else}<p class="hint">None.</p>{/each}
          {#each rules.filter((r) => r.scan !== "always") as r}
            <div class="w other" title={r.path}>{ICON[r.scan][0]} {r.path}</div>
          {/each}
        </div>
      </div>
    </div>
    {#if error}<p class="err">{error}</p>{/if}
    <div class="buttons">
      <button onclick={ok} disabled={busy}>OK</button>
      <button onclick={cancel} disabled={busy}>Cancel</button>
    </div>
  </div>
</div>

<style>
  .scrim { position: fixed; inset: 0; background: #0004; display: grid; place-items: center; z-index: 60; }
  .dlg { background: #f6f5f1; border: 1px solid #999; box-shadow: 0 8px 30px #0005; width: min(880px, 94vw); height: min(620px, 90vh); display: flex; flex-direction: column; padding: 12px 16px; }
  h2 { margin: 0 0 6px; font-size: 17px; }
  .intro { margin: 0 0 8px; color: #444; line-height: 1.4; }
  .body { flex: 1; display: flex; gap: 12px; min-height: 0; }
  .tree { flex: 1.3; overflow: auto; background: #fff; border: 1px solid #bbb; padding: 4px 0; }
  .node { display: flex; align-items: center; height: 22px; white-space: nowrap; }
  .node.cur { background: var(--list-sel); }
  .node.cur .name { color: #fff; }
  .arrow { width: 18px; height: 18px; padding: 0; border: none; background: none; font-size: 11px; color: #666; }
  .arrow.hide { visibility: hidden; }
  .name { border: none; background: none; padding: 0 4px; font-weight: 400; display: flex; gap: 6px; align-items: center; }
  .st { width: 14px; text-align: center; color: #25648b; font-weight: 700; }
  .st.faint { opacity: 0.45; }
  .side { flex: 1; display: flex; flex-direction: column; gap: 10px; min-width: 0; }
  .box { background: #fff; border: 1px solid #bbb; padding: 8px 10px; display: flex; flex-direction: column; gap: 6px; }
  .box:last-child { flex: 1; overflow: auto; }
  .h { font-weight: 600; }
  .path { color: #555; font-size: 12px; word-break: break-all; }
  label { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; }
  .hint { color: #777; font-size: 12px; width: 100%; padding-left: 22px; margin: 0; }
  .box > .hint { padding-left: 0; }
  .link { align-self: flex-start; background: none; border: none; color: #25648b; text-decoration: underline; padding: 0; font-weight: 400; }
  .w { font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .w.other { color: #777; }
  .err { color: #b00; }
  .buttons { display: flex; justify-content: flex-end; gap: 8px; padding-top: 10px; }
  .buttons button { min-width: 80px; }
</style>
