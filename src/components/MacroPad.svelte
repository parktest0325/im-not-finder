<script lang="ts">
  import {
    macroState,
    isConfigured,
    prettyCombo,
    type Macro,
  } from "../lib/macros.svelte";
  import MacroEditor from "./MacroEditor.svelte";

  let {
    tabs,
    onRun,
  }: {
    tabs: { id: string; name: string; kind: string; sess: string }[];
    onRun: (m: Macro) => void;
  } = $props();

  let editing = $state<number | null>(null);

  function activate(i: number, m: Macro) {
    if (isConfigured(m)) onRun(m);
    else editing = i;
  }

  // hover tooltip: shortcut on the first line, then the commands
  function cellTitle(m: Macro): string {
    if (!isConfigured(m)) return "right-click to edit";
    const sc = m.shortcut ? prettyCombo(m.shortcut) : "no shortcut";
    return `[${sc}]\n${m.commands}`;
  }
</script>

<div class="pad">
  {#each macroState.list as m, i (m.id)}
    <button
      class="cell"
      class:empty={!isConfigured(m)}
      style={m.color ? `background:${m.color}22; border-color:${m.color};` : ""}
      onclick={() => activate(i, m)}
      oncontextmenu={(e) => {
        e.preventDefault();
        editing = i;
      }}
      title={cellTitle(m)}
    >
      {#if isConfigured(m)}
        <span class="lbl">{m.label || `macro ${i + 1}`}</span>
        {#if m.shortcut}<span class="sc">{prettyCombo(m.shortcut)}</span>{/if}
        {#if m.pinnedTabId}<span class="pin" title="pinned terminal">📌</span>{/if}
      {:else}
        <span class="plus">+</span>
      {/if}
    </button>
  {/each}
</div>

{#if editing !== null}
  <MacroEditor index={editing} {tabs} onClose={() => (editing = null)} />
{/if}

<style>
  .pad {
    height: 100%;
    display: grid;
    grid-template-columns: 1fr 1fr;
    grid-template-rows: repeat(5, 1fr);
    gap: 4px;
    padding: 4px;
    background: var(--bg-panel);
    border-left: 1px solid var(--border);
    overflow: auto;
  }
  .cell {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 3px;
    min-height: 44px;
    border: 1px solid var(--border);
    background: var(--bg-soft);
    color: var(--fg);
    padding: 4px;
    overflow: hidden;
  }
  .cell:hover {
    border-color: var(--border-bright);
    color: var(--fg-bright);
  }
  .cell.empty {
    color: var(--fg-dim);
    border-style: dashed;
    opacity: 0.6;
  }
  .lbl {
    font-size: 12px;
    text-align: center;
    line-height: 1.15;
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    word-break: break-word;
  }
  .sc {
    font-size: 10px;
    color: var(--fg-dim);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 2px;
    padding: 0 3px;
    white-space: nowrap;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .plus {
    font-size: 18px;
  }
  .pin {
    position: absolute;
    top: 2px;
    right: 3px;
    font-size: 9px;
  }
</style>
