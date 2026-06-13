<script lang="ts">
  import type { Session } from "./lib/api";
  import { macroState, recorder, comboFromEvent } from "./lib/macros.svelte";
  import ConnectionBar from "./components/ConnectionBar.svelte";
  import ConnectDialog from "./components/ConnectDialog.svelte";
  import FileTree from "./components/FileTree.svelte";
  import MainPanel from "./components/MainPanel.svelte";
  import TerminalPanel from "./components/TerminalPanel.svelte";
  import StatusBar from "./components/StatusBar.svelte";
  import Splitter from "./components/Splitter.svelte";

  let sessions = $state<Session[]>([]);
  let activeId = $state<string | null>(null);
  let dialogOpen = $state(false);
  let terminalPanel = $state<TerminalPanel>();

  function syncTerminalTo(path: string) {
    const esc = path.replace(/'/g, "'\\''");
    terminalPanel?.sendToActive(`cd '${esc}'\r`);
  }

  // The entry currently shown in the main (hex/preview) panel.
  let preview = $state<{ session: Session; path: string; name: string } | null>(
    null,
  );
  let status = $state("ready");

  // resizable layout
  let leftWidth = $state(320);
  let termHeight = $state(220);

  // app-wide zoom (Ctrl + / Ctrl - / Ctrl 0)
  let zoom = $state(1);
  function onKeydown(e: KeyboardEvent) {
    if (!e.ctrlKey) return;
    if (e.key === "=" || e.key === "+") {
      e.preventDefault();
      zoom = Math.min(2.5, +(zoom + 0.1).toFixed(2));
    } else if (e.key === "-" || e.key === "_") {
      e.preventDefault();
      zoom = Math.max(0.5, +(zoom - 0.1).toFixed(2));
    } else if (e.key === "0") {
      e.preventDefault();
      zoom = 1;
    }
  }
  // Apply zoom to the document root so it scales EVERYTHING, including
  // fixed-position modals (connect dialog) that live outside .app-shell.
  $effect(() => {
    document.documentElement.style.zoom = String(zoom);
  });

  // Global macro hotkeys — capture phase so they win over xterm and every
  // other handler. A match is fully consumed; non-matches pass through.
  $effect(() => {
    function onCapture(e: KeyboardEvent) {
      if (recorder.active) return; // don't fire while recording a shortcut
      const combo = comboFromEvent(e);
      if (!combo) return;
      const m = macroState.list.find((x) => x.shortcut === combo);
      if (m) {
        e.preventDefault();
        e.stopImmediatePropagation();
        terminalPanel?.runMacro(m);
      }
    }
    window.addEventListener("keydown", onCapture, true);
    return () => window.removeEventListener("keydown", onCapture, true);
  });

  let activeSession = $derived(
    sessions.find((s) => s.id === activeId) ?? null,
  );

  function onConnected(s: Session) {
    sessions = [...sessions, s];
    activeId = s.id;
    dialogOpen = false;
    status = `connected: ${s.label}`;
  }

  function onClose(id: string) {
    sessions = sessions.filter((s) => s.id !== id);
    if (activeId === id) activeId = sessions.at(-1)?.id ?? null;
    if (preview && !sessions.some((s) => s.id === preview!.session.id))
      preview = null;
  }

  function openPreview(session: Session, path: string, name: string) {
    preview = { session, path, name };
  }
</script>

<svelte:window onkeydown={onKeydown} />

<div class="app-shell">
  <ConnectionBar
    {sessions}
    {activeId}
    onSelect={(id) => (activeId = id)}
    onAdd={() => (dialogOpen = true)}
    {onClose}
  />

  <div
    class="workspace"
    style="grid-template-columns: {leftWidth}px 6px 1fr; grid-template-rows: 1fr 6px {termHeight}px;"
  >
    <div class="tree-cell">
      <FileTree
        session={activeSession}
        onOpen={openPreview}
        onStatus={(s) => (status = s)}
        onSyncTerminal={syncTerminalTo}
      />
    </div>

    <Splitter
      axis="x"
      style="grid-column: 2; grid-row: 1;"
      ondelta={(dx) => (leftWidth = Math.max(180, Math.min(640, leftWidth + dx)))}
    />

    <div class="main-cell">
      <MainPanel {preview} />
    </div>

    <Splitter
      axis="y"
      style="grid-column: 1 / span 3; grid-row: 2;"
      ondelta={(dy) =>
        (termHeight = Math.max(80, Math.min(560, termHeight - dy)))}
    />

    <div class="term-cell">
      <TerminalPanel bind:this={terminalPanel} session={activeSession} {sessions} />
    </div>
  </div>

  <StatusBar {status} session={activeSession} />
</div>

{#if dialogOpen}
  <ConnectDialog onConnected={onConnected} onCancel={() => (dialogOpen = false)} />
{/if}

<style>
  .app-shell {
    height: 100%;
    display: grid;
    grid-template-rows: auto 1fr auto;
    background: var(--bg);
  }

  .workspace {
    display: grid;
    min-height: 0;
    gap: 0;
    padding: 4px;
  }

  /* row 1: tree | splitter | main ; row 3 (term) spans all columns */
  .tree-cell,
  .main-cell,
  .term-cell {
    display: flex;
    min-width: 0;
    min-height: 0;
  }
  /* make each panel fill its grid cell so its body can own the scroll */
  .tree-cell > :global(.panel),
  .main-cell > :global(.panel),
  .term-cell > :global(.panel) {
    flex: 1 1 auto;
    min-width: 0;
    min-height: 0;
  }
  .tree-cell {
    grid-column: 1;
    grid-row: 1;
  }
  .main-cell {
    grid-column: 3;
    grid-row: 1;
  }
  .term-cell {
    grid-column: 1 / span 3;
    grid-row: 3;
  }
</style>
