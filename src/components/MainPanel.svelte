<script lang="ts">
  import type { Session } from "../lib/api";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import HexView from "./HexView.svelte";
  import TextView from "./TextView.svelte";

  let {
    preview,
  }: {
    preview: { session: Session; path: string; name: string } | null;
  } = $props();

  // right-click copies the current text selection (hex/ascii or text body)
  async function onContextMenu(e: MouseEvent) {
    const sel = window.getSelection()?.toString();
    if (sel) {
      e.preventDefault();
      try {
        await writeText(sel);
      } catch {
        /* ignore */
      }
    }
  }

  type Viewer = "text" | "hex";
  let viewer = $state<Viewer>("text");
  const viewers: { id: Viewer; label: string }[] = [
    { id: "text", label: "TEXT" },
    { id: "hex", label: "HEX" },
  ];
</script>

<div class="panel">
  <div class="panel-title">
    <span>viewer</span>
    {#if preview}<span class="dim">— {preview.name}</span>{/if}
    <span class="spacer"></span>
    <div class="vtabs">
      {#each viewers as v (v.id)}
        <button
          class="vtab"
          class:on={viewer === v.id}
          disabled={!preview}
          onclick={() => (viewer = v.id)}>{v.label}</button
        >
      {/each}
    </div>
  </div>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="panel-body selectable" oncontextmenu={onContextMenu}>
    {#if preview}
      {#key preview.session.id + preview.path + viewer}
        {#if viewer === "hex"}
          <HexView session={preview.session} path={preview.path} />
        {:else}
          <TextView session={preview.session} path={preview.path} />
        {/if}
      {/key}
    {:else}
      <div class="placeholder dim">
        <pre>{`  ╭─────────────────────────────╮
  │  open a file to inspect     │
  │  TEXT · HEX viewers         │
  ╰─────────────────────────────╯`}</pre>
      </div>
    {/if}
  </div>
</div>

<style>
  .spacer {
    flex: 1;
  }
  /* let users select & copy bytes/text in the viewer (body is user-select:none) */
  .selectable,
  .selectable :global(*) {
    user-select: text;
  }
  .vtabs {
    display: flex;
    gap: 2px;
  }
  .vtab {
    border: 1px solid var(--border);
    background: var(--bg-panel);
    color: var(--fg-dim);
    padding: 0 8px;
    font-size: 11px;
    line-height: 16px;
  }
  .vtab.on {
    color: var(--bg);
    background: var(--accent);
    border-color: var(--accent);
    font-weight: 700;
  }
  .placeholder {
    height: 100%;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  pre {
    margin: 0;
    line-height: 1.3;
  }
</style>
