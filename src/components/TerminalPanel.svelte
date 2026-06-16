<script lang="ts">
  import {
    emptyWorkspace,
    type Session,
    type Workspace,
    type WsTerminal,
  } from "../lib/api";
  import TerminalInstance from "./TerminalInstance.svelte";
  import MacroPad from "./MacroPad.svelte";
  import { macroRunText, unpinTab, type Macro } from "../lib/macros.svelte";

  let {
    session,
    sessions,
    zoom = 1,
    workspace = emptyWorkspace(),
    onCwd,
  }: {
    session: Session | null; // active session
    sessions: Session[]; // all live sessions (for cleanup)
    zoom?: number; // app zoom; terminals counter-zoom and scale font instead
    workspace?: Workspace; // saved layout to restore on (re)connect
    onCwd?: (path: string) => void; // "⇄ files" → move FILES to terminal cwd
  } = $props();

  interface Tab {
    id: string;
    session: Session;
    name: string;
    cwd?: string | null; // restore target (initial cd), only used at creation
  }

  // a session's workspace storage slot (per-instance: key, or key#2 on collision)
  const slotOf = (s: Session) => s.wsKey ?? s.key;

  let tabs = $state<Tab[]>([]);
  // which terminal tab is active *within* each session
  let activeBySession = $state<Record<string, string>>({});
  let editingId = $state<string | null>(null);
  let editValue = $state("");
  let padOpen = $state(true);
  let counter = 0;
  const instances: Record<string, TerminalInstance> = {};

  // all terminals across sessions (for the macro "pin to terminal" picker)
  let tabsInfo = $derived(
    tabs.map((t) => ({
      id: t.id,
      name: t.name,
      kind: t.session.kind,
      sess: t.session.label,
    })),
  );

  // terminals belonging to the currently active session
  let visibleTabs = $derived(
    session ? tabs.filter((t) => t.session.id === session.id) : [],
  );
  let activeTabId = $derived(
    session
      ? (activeBySession[session.id] ?? visibleTabs[0]?.id ?? null)
      : null,
  );

  function addTab(s: Session, name?: string, cwd?: string | null) {
    const id = `term-${counter++}`;
    const n = tabs.filter((t) => t.session.id === s.id).length + 1;
    tabs = [...tabs, { id, session: s, name: name ?? `${s.kind}#${n}`, cwd }];
    activeBySession = { ...activeBySession, [s.id]: id };
  }

  // session keys whose saved layout we've already restored this run (so closing
  // every tab and re-activating opens a fresh one rather than re-restoring)
  const restored = new Set<string>();

  // the first time a session becomes active: restore its saved terminals (names
  // + last cwd) if any, otherwise open a single default terminal
  $effect(() => {
    if (!session || tabs.some((t) => t.session.id === session.id)) return;
    const slot = slotOf(session);
    const saved = workspace.entries[slot]?.terminals;
    if (saved?.length && !restored.has(slot)) {
      restored.add(slot);
      for (const t of saved) addTab(session, t.name, t.cwd);
    } else {
      addTab(session);
    }
  });

  // dispose terminals whose session was disconnected
  $effect(() => {
    const live = new Set(sessions.map((s) => s.id));
    const orphans = tabs.filter((t) => !live.has(t.session.id));
    if (orphans.length) {
      for (const o of orphans) {
        void instances[o.id]?.dispose();
        delete instances[o.id];
      }
      // forget the restore guard so reconnecting this session (same run)
      // restores its saved layout again rather than opening a blank terminal
      for (const o of orphans) restored.delete(slotOf(o.session));
      tabs = tabs.filter((t) => live.has(t.session.id));
    }
  });

  function selectTab(id: string) {
    if (session) activeBySession = { ...activeBySession, [session.id]: id };
  }

  // send text to the active terminal of the active session (used by the file tree)
  export function sendToActive(text: string) {
    if (activeTabId) instances[activeTabId]?.send(text);
  }

  // "⇄ files": jump FILES to the active terminal's tracked cwd (no injection)
  function syncActiveToFiles() {
    if (!activeTabId) return;
    const c = instances[activeTabId]?.getCwd();
    if (c) onCwd?.(c);
  }

  // snapshot the current layout (per session key) for persistence
  export function snapshot(): Record<string, WsTerminal[]> {
    const out: Record<string, WsTerminal[]> = {};
    for (const t of tabs) {
      (out[slotOf(t.session)] ||= []).push({
        name: t.name,
        cwd: instances[t.id]?.getCwd() ?? t.cwd ?? null,
      });
    }
    return out;
  }

  // run a macro in its pinned terminal, else the focused (active) terminal
  export function runMacro(m: Macro) {
    const text = macroRunText(m);
    if (!text) return;
    const pinnedOk =
      m.pinnedTabId && tabs.some((t) => t.id === m.pinnedTabId);
    const target = pinnedOk ? m.pinnedTabId! : activeTabId;
    if (target && instances[target]) instances[target].send(text);
  }

  async function closeTab(id: string) {
    const t = tabs.find((x) => x.id === id);
    await instances[id]?.dispose();
    delete instances[id];
    unpinTab(id); // macros pinned here fall back to the focused terminal
    tabs = tabs.filter((x) => x.id !== id);
    if (t && activeBySession[t.session.id] === id) {
      const remaining = tabs.filter((x) => x.session.id === t.session.id);
      activeBySession = {
        ...activeBySession,
        [t.session.id]: remaining.at(-1)?.id ?? "",
      };
    }
  }

  // right-click tab menu
  let tabMenu = $state<{ id: string; x: number; y: number } | null>(null);
  function openTabMenu(e: MouseEvent, t: Tab) {
    e.preventDefault();
    tabMenu = { id: t.id, x: e.clientX, y: e.clientY };
  }
  const closeTabMenu = () => (tabMenu = null);

  // clone a terminal: new tab in the same session, seeded with the live cwd
  function duplicateTab(t: Tab) {
    const cwd = instances[t.id]?.getCwd() ?? t.cwd ?? null;
    addTab(t.session, `${t.name} copy`, cwd);
  }

  function startRename(t: Tab) {
    editingId = t.id;
    editValue = t.name;
  }
  function commitRename() {
    const t = tabs.find((x) => x.id === editingId);
    if (t) {
      const v = editValue.trim();
      t.name = v || t.name;
      tabs = [...tabs];
    }
    editingId = null;
  }

  // focus + select the rename field as soon as it appears
  function autofocusSelect(node: HTMLInputElement) {
    node.focus();
    node.select();
  }
</script>

<div class="panel">
  <div class="tabbar">
    <span class="label">terminal</span>
    <div class="tabs">
      {#each visibleTabs as t (t.id)}
        <div
          class="tab"
          class:active={t.id === activeTabId}
          onclick={() => selectTab(t.id)}
          ondblclick={() => startRename(t)}
          onmousedown={(e) => {
            if (e.button === 1) e.preventDefault(); // suppress autoscroll
          }}
          onauxclick={(e) => {
            if (e.button === 1) void closeTab(t.id); // middle press+release on this tab
          }}
          oncontextmenu={(e) => openTabMenu(e, t)}
          role="tab"
          tabindex="0"
          onkeydown={(e) => e.key === "Enter" && selectTab(t.id)}
          title="double-click to rename · middle-click to close · right-click for menu"
        >
          <span class="dot {t.session.kind}"></span>
          {#if editingId === t.id}
            <input
              class="rename"
              bind:value={editValue}
              use:autofocusSelect
              onclick={(e) => e.stopPropagation()}
              onblur={commitRename}
              onkeydown={(e) => {
                if (e.key === "Enter") commitRename();
                else if (e.key === "Escape") editingId = null;
              }}
            />
          {:else}
            <span class="tname">{t.name}</span>
          {/if}
          <button
            class="x"
            title="close"
            onclick={(e) => {
              e.stopPropagation();
              void closeTab(t.id);
            }}>×</button
          >
        </div>
      {/each}
    </div>
    <span class="spacer"></span>
    <button
      class="syncfiles"
      title="sync FILES to this terminal's directory"
      disabled={!activeTabId}
      onclick={syncActiveToFiles}>⇄ files</button
    >
    <button
      class="macrotoggle"
      class:on={padOpen}
      title="macros"
      onclick={() => (padOpen = !padOpen)}>▦ macros</button
    >
    <button
      class="add"
      title="new terminal (active session)"
      disabled={!session}
      onclick={() => session && addTab(session)}>＋</button
    >
  </div>

  <div class="body-row">
    <div class="stack">
      {#if !session}
        <div class="hint dim">connect a session to open a terminal</div>
      {/if}
      <!-- every instance stays mounted (keeps its shell + scrollback);
           only the active tab of the active session is visible -->
      {#each tabs as t (t.id)}
        <TerminalInstance
          bind:this={instances[t.id]}
          session={t.session}
          active={t.id === activeTabId}
          {zoom}
          initialCwd={t.cwd ?? null}
        />
      {/each}
    </div>
    {#if padOpen}
      <div class="paddock">
        <MacroPad tabs={tabsInfo} onRun={runMacro} />
      </div>
    {/if}
  </div>
</div>

{#if tabMenu}
  <div
    class="ctx-backdrop"
    role="presentation"
    onclick={closeTabMenu}
    oncontextmenu={(e) => {
      e.preventDefault();
      closeTabMenu();
    }}
  ></div>
  <div class="ctx" style="left: {tabMenu.x}px; top: {tabMenu.y}px;">
    <button
      onclick={() => {
        const t = tabs.find((x) => x.id === tabMenu!.id);
        if (t) duplicateTab(t);
        closeTabMenu();
      }}>Duplicate</button
    >
    <button
      onclick={() => {
        const t = tabs.find((x) => x.id === tabMenu!.id);
        if (t) startRename(t);
        closeTabMenu();
      }}>Rename</button
    >
    <button
      onclick={() => {
        void closeTab(tabMenu!.id);
        closeTabMenu();
      }}>Close</button
    >
  </div>
{/if}

<style>
  .ctx-backdrop {
    position: fixed;
    inset: 0;
    z-index: 999;
  }
  .ctx {
    position: fixed;
    z-index: 1000;
    display: flex;
    flex-direction: column;
    min-width: 120px;
    background: var(--bg-panel);
    border: 1px solid var(--border);
    box-shadow: 0 4px 14px rgba(0, 0, 0, 0.5);
  }
  .ctx button {
    text-align: left;
    border: none;
    background: transparent;
    color: var(--fg);
    padding: 5px 10px;
    font: inherit;
  }
  .ctx button:hover {
    background: var(--bg-hover);
    color: var(--fg-bright);
  }
  .tabbar {
    flex: 0 0 auto;
    display: flex;
    align-items: stretch;
    gap: 6px;
    height: 24px;
    padding: 0 6px;
    background: var(--bg-soft);
    border-bottom: 1px solid var(--border);
  }
  .label {
    display: flex;
    align-items: center;
    color: var(--fg-dim);
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    padding-right: 6px;
    border-right: 1px solid var(--border);
  }
  .tabs {
    display: flex;
    align-items: stretch;
    gap: 3px;
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 4px 0 8px;
    border: 1px solid transparent;
    border-bottom: none;
    color: var(--fg-dim);
    cursor: pointer;
    max-width: 180px;
  }
  .tab:hover {
    background: var(--bg-hover);
    color: var(--fg);
  }
  .tab.active {
    background: var(--bg-panel);
    border-color: var(--border);
    color: var(--fg-bright);
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex: 0 0 auto;
  }
  .dot.ssh {
    background: var(--accent);
  }
  .dot.adb {
    background: var(--accent-2);
  }
  .dot.local {
    background: var(--warn);
  }
  .tname {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .rename {
    width: 90px;
    padding: 0 2px;
    font-size: 12px;
  }
  .x {
    border: none;
    background: transparent;
    color: var(--fg-dim);
    padding: 0 3px;
  }
  .x:hover {
    color: var(--error);
    background: transparent;
  }
  .add {
    align-self: center;
    border: none;
    background: transparent;
    color: var(--fg-dim);
    font-size: 15px;
    padding: 0 6px;
  }
  .add:hover:not(:disabled) {
    color: var(--fg-bright);
    background: transparent;
  }
  .spacer {
    flex: 1;
  }
  .macrotoggle,
  .syncfiles {
    align-self: center;
    border: 1px solid var(--border);
    background: var(--bg-panel);
    color: var(--fg-dim);
    font-size: 11px;
    line-height: 16px;
    padding: 0 6px;
  }
  .syncfiles {
    margin-right: 4px;
  }
  .syncfiles:hover:not(:disabled) {
    color: var(--fg-bright);
    border-color: var(--accent);
  }
  .syncfiles:disabled {
    opacity: 0.4;
  }
  .macrotoggle.on {
    color: var(--bg);
    background: var(--accent);
    border-color: var(--accent);
    font-weight: 700;
  }
  .body-row {
    flex: 1 1 auto;
    min-height: 0;
    display: flex;
  }
  .paddock {
    flex: 0 0 168px;
    min-width: 0;
    min-height: 0;
  }
  .stack {
    position: relative;
    flex: 1 1 auto;
    min-width: 0;
    min-height: 0;
    overflow: hidden;
  }
  .hint {
    padding: 12px;
  }
</style>
