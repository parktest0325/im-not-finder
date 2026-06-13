<script lang="ts">
  import type { Session } from "../lib/api";

  let {
    sessions,
    activeId,
    onSelect,
    onAdd,
    onClose,
  }: {
    sessions: Session[];
    activeId: string | null;
    onSelect: (id: string) => void;
    onAdd: () => void;
    onClose: (id: string) => void;
  } = $props();
</script>

<div class="bar">
  <div class="brand">im-not-finder</div>
  <div class="tabs">
    {#each sessions as s (s.id)}
      <div
        class="tab"
        class:active={s.id === activeId}
        onclick={() => onSelect(s.id)}
        role="tab"
        tabindex="0"
        onkeydown={(e) => e.key === "Enter" && onSelect(s.id)}
      >
        <span class="badge {s.kind}">{s.kind}</span>
        <span class="label">{s.label}</span>
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
</style>
