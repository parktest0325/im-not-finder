<script lang="ts">
  import type { Session } from "../lib/api";

  let {
    sessions,
    activeId,
    names = {},
    onSelect,
    onAdd,
    onClose,
    onRename,
    onDuplicate,
    onForget,
  }: {
    sessions: Session[];
    activeId: string | null;
    names?: Record<string, string>; // custom session label, keyed by Session.key
    onSelect: (id: string) => void;
    onAdd: () => void;
    onClose: (id: string) => void;
    onRename?: (id: string, name: string) => void;
    onDuplicate?: (id: string) => void;
    onForget?: (id: string) => void;
  } = $props();

  let editingId = $state<string | null>(null);
  let editValue = $state("");
  let menu = $state<{ id: string; x: number; y: number } | null>(null);

  function openMenu(e: MouseEvent, s: Session) {
    e.preventDefault();
    menu = { id: s.id, x: e.clientX, y: e.clientY };
  }
  const closeMenu = () => (menu = null);

  const slotOf = (s: Session) => s.wsKey ?? s.key;
  function startRename(s: Session) {
    editingId = s.id;
    editValue = names[slotOf(s)] ?? s.label;
  }
  function commitRename() {
    if (editingId) onRename?.(editingId, editValue.trim());
    editingId = null;
  }
  function autofocusSelect(node: HTMLInputElement) {
    node.focus();
    node.select();
  }
</script>

<div class="bar">
  <div class="brand">im-not-finder</div>
  <div class="tabs">
    {#each sessions as s (s.id)}
      <div
        class="tab"
        class:active={s.id === activeId}
        onclick={() => onSelect(s.id)}
        onmousedown={(e) => {
          if (e.button === 1) e.preventDefault(); // suppress autoscroll
        }}
        onauxclick={(e) => {
          if (e.button === 1) onClose(s.id); // middle press+release on this tab
        }}
        role="tab"
        tabindex="0"
        ondblclick={() => startRename(s)}
        oncontextmenu={(e) => openMenu(e, s)}
        onkeydown={(e) => e.key === "Enter" && onSelect(s.id)}
        title="double-click to rename · right-click for menu"
      >
        <span class="badge {s.kind}">{s.kind}</span>
        {#if editingId === s.id}
          <input
            class="rename"
            bind:value={editValue}
            use:autofocusSelect
            onclick={(e) => e.stopPropagation()}
            ondblclick={(e) => e.stopPropagation()}
            onblur={commitRename}
            onkeydown={(e) => {
              if (e.key === "Enter") commitRename();
              else if (e.key === "Escape") editingId = null;
            }}
          />
        {:else}
          <span class="label">{names[slotOf(s)] ?? s.label}</span>
        {/if}
        <button
          class="x"
          title="disconnect"
          onclick={(e) => {
            e.stopPropagation();
            onClose(s.id);
          }}>×</button
        >
      </div>
    {/each}
  </div>
  <div class="spacer"></div>
  <button class="add" onclick={onAdd}>+ connect</button>
</div>

{#if menu}
  <div
    class="ctx-backdrop"
    role="presentation"
    onclick={closeMenu}
    oncontextmenu={(e) => {
      e.preventDefault();
      closeMenu();
    }}
  ></div>
  <div class="ctx" style="left: {menu.x}px; top: {menu.y}px;">
    <button
      onclick={() => {
        onDuplicate?.(menu!.id);
        closeMenu();
      }}>Duplicate</button
    >
    <button
      onclick={() => {
        startRename(sessions.find((s) => s.id === menu!.id)!);
        closeMenu();
      }}>Rename</button
    >
    <button
      onclick={() => {
        onClose(menu!.id);
        closeMenu();
      }}>Close</button
    >
    <button
      class="danger"
      onclick={() => {
        onForget?.(menu!.id);
        closeMenu();
      }}>Close &amp; forget</button
    >
  </div>
{/if}

<style>
  .bar {
    display: flex;
    align-items: stretch;
    gap: 8px;
    height: 30px;
    padding: 0 8px;
    background: var(--bg-soft);
    border-bottom: 1px solid var(--border);
  }
  .brand {
    display: flex;
    align-items: center;
    color: var(--fg-dim);
    font-weight: 600;
    letter-spacing: 0.03em;
    padding-right: 6px;
    border-right: 1px solid var(--border);
  }
  .tabs {
    display: flex;
    align-items: stretch;
    gap: 4px;
  }
  .tab {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 4px 0 8px;
    border: 1px solid transparent;
    border-bottom: none;
    cursor: pointer;
    color: var(--fg-dim);
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
  .badge {
    font-size: 10px;
    text-transform: uppercase;
    padding: 0 4px;
    border-radius: 2px;
    color: var(--bg);
    font-weight: 700;
  }
  .badge.ssh {
    background: var(--accent);
  }
  .badge.adb {
    background: var(--accent-2);
  }
  .badge.local {
    background: var(--warn);
  }
  .rename {
    background: var(--bg);
    border: 1px solid var(--accent);
    color: var(--fg-bright);
    font: inherit;
    width: 12ch;
    padding: 0 2px;
  }
  .x {
    border: none;
    background: transparent;
    padding: 0 4px;
    color: var(--fg-dim);
  }
  .x:hover {
    color: var(--error);
    background: transparent;
  }
  .spacer {
    flex: 1;
  }
  .add {
    align-self: center;
  }
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
    min-width: 130px;
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
  .ctx button.danger:hover {
    background: var(--error);
    color: var(--bg);
  }
</style>
