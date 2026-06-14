<script lang="ts">
  import type { Session } from "../lib/api";
  import TerminalInstance from "./TerminalInstance.svelte";
  import MacroPad from "./MacroPad.svelte";
  import { macroRunText, unpinTab, type Macro } from "../lib/macros.svelte";

  let {
    session,
    sessions,
    zoom = 1,
  }: {
    session: Session | null; // active session
    sessions: Session[]; // all live sessions (for cleanup)
    zoom?: number; // app zoom; terminals counter-zoom and scale font instead
  } = $props();

  interface Tab {
    id: string;
    session: Session;
    name: string;
  }

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

  function addTab(s: Session) {
    const id = `term-${counter++}`;
    const n = tabs.filter((t) => t.session.id === s.id).length + 1;
    tabs = [...tabs, { id, session: s, name: `${s.kind}#${n}` }];
    activeBySession = { ...activeBySession, [s.id]: id };
  }

  // auto-open one terminal the first time a session becomes active
  $effect(() => {
    if (session && !tabs.some((t) => t.session.id === session.id)) {
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
          role="tab"
          tabindex="0"
          onkeydown={(e) => e.key === "Enter" && selectTab(t.id)}
          title="double-click to rename"
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

<style>
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
  .macrotoggle {
    align-self: center;
    border: 1px solid var(--border);
    background: var(--bg-panel);
    color: var(--fg-dim);
    font-size: 11px;
    line-height: 16px;
    padding: 0 6px;
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
