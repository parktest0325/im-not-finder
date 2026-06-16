<script lang="ts">
  import { untrack, onMount } from "svelte";
  import {
    loadWorkspace,
    saveWorkspace,
    emptyWorkspace,
    listSshHistory,
    connectLocal,
    connectAdb,
    connectSsh,
    type Session,
    type Workspace,
    type WsEntry,
  } from "./lib/api";
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
  let fileTree = $state<FileTree>();

  function syncTerminalTo(path: string) {
    const esc = path.replace(/'/g, "'\\''");
    terminalPanel?.sendToActive(`cd '${esc}'\r`);
  }

  // terminal reported its cwd (via the "⇄ files" button) → move FILES there
  function syncFilesTo(path: string) {
    fileTree?.navigateTo(path.trim());
  }

  // ---- workspace persistence (open sessions + per-session layout) ----
  let ws = $state<Workspace>(emptyWorkspace());
  let loaded = false; // guard: never write before the first read (would wipe it)
  // live per-session signals fed back from the panels, keyed by storage slot
  const filesCwdByKey: Record<string, string> = {};
  const elevatedByKey: Record<string, boolean> = {};
  const nameByKey: Record<string, string> = {};

  // a session's storage slot (per-instance; same device opened twice → key, key#2)
  const slotOf = (s: Session) => s.wsKey ?? s.key;

  function rememberFilesCwd(path: string) {
    if (activeSession) filesCwdByKey[slotOf(activeSession)] = path;
  }
  function rememberElevated(on: boolean) {
    if (activeSession) elevatedByKey[slotOf(activeSession)] = on;
  }

  // custom session-tab names, keyed by storage slot (for ConnectionBar display)
  let sessionNames = $derived(
    Object.fromEntries(
      Object.entries(ws.entries)
        .filter(([, e]) => e.name)
        .map(([k, e]) => [k, e.name as string]),
    ),
  );
  function renameSession(id: string, name: string) {
    const s = sessions.find((x) => x.id === id);
    if (!s) return;
    const slot = slotOf(s);
    const clean = name || undefined;
    if (clean) nameByKey[slot] = clean;
    else delete nameByKey[slot];
    const prev = ws.entries[slot] ?? blankEntry(s);
    ws = { ...ws, entries: { ...ws.entries, [slot]: { ...prev, name: clean } } };
    persist();
  }

  const blankEntry = (s: Session): WsEntry => ({
    kind: s.kind,
    identity: s.key,
    terminals: [],
    filesCwd: null,
    elevated: false,
  });

  // Pick the storage slot for a new connection to `identity`: reuse the first
  // saved slot for that identity that ISN'T currently open (so its layout is
  // restored), else mint a fresh unique key.
  function pickSlot(identity: string): string {
    const live = new Set(sessions.map(slotOf));
    for (const [wsKey, e] of Object.entries(ws.entries))
      if (e.identity === identity && !live.has(wsKey)) return wsKey;
    return crypto.randomUUID();
  }

  // Rebuild each live session's entry from the panels (others kept as saved),
  // refresh active. openKeys is maintained by connect/disconnect, not here.
  function persist() {
    if (!loaded) return;
    const snap = terminalPanel?.snapshot() ?? {};
    const entries = { ...ws.entries };
    for (const s of sessions) {
      const slot = slotOf(s);
      const prev = entries[slot] ?? blankEntry(s);
      entries[slot] = {
        kind: prev.kind ?? s.kind,
        identity: prev.identity ?? s.key,
        terminals: snap[slot] ?? prev.terminals ?? [],
        filesCwd: filesCwdByKey[slot] ?? prev.filesCwd ?? null,
        elevated: elevatedByKey[slot] ?? prev.elevated ?? false,
        name: nameByKey[slot] ?? prev.name,
      };
    }
    ws = {
      ...ws,
      active: activeSession ? slotOf(activeSession) : ws.active,
      entries,
    };
    void saveWorkspace(ws);
  }

  // Reconnect by identity without any stored secret: local always, adb if the
  // device is present, key-auth SSH via history. Used for restore + duplicate.
  async function connectByIdentity(
    kind: Session["kind"],
    identity: string,
    hist: Awaited<ReturnType<typeof listSshHistory>>,
  ): Promise<Session | null> {
    try {
      if (kind === "local") return await connectLocal();
      if (kind === "adb") return await connectAdb(identity.slice(4));
      if (kind === "ssh") {
        const h = hist.find((x) => x.id === identity.slice(4));
        if (h && h.authMode === "key")
          return await connectSsh({
            host: h.host,
            port: h.port,
            username: h.username,
            keyPath: h.keyPath,
          });
      }
    } catch (err) {
      status = `couldn't connect ${identity}: ${err}`;
    }
    return null; // password SSH (or failure) → user connects manually
  }

  // Duplicate a session: open another live instance of the same device in a
  // brand-new slot, cloning the whole layout (every terminal tab + its cwd,
  // the FILES dir and su state) so it comes up identical.
  async function duplicateSession(id: string) {
    const s = sessions.find((x) => x.id === id);
    if (!s) return;
    persist(); // flush the source's live tabs/cwd into ws.entries first
    const src = ws.entries[slotOf(s)];
    const hist = await listSshHistory().catch(() => []);
    const ns = await connectByIdentity(s.kind, s.key, hist);
    if (!ns) {
      status = `can't duplicate ${s.label} (needs manual connect)`;
      return;
    }
    const slot = crypto.randomUUID();
    const sess = { ...ns, wsKey: slot };
    const cloned: WsEntry = src
      ? {
          kind: sess.kind,
          identity: sess.key,
          terminals: src.terminals.map((t) => ({ ...t })),
          filesCwd: src.filesCwd,
          elevated: src.elevated,
          name: src.name ? `${src.name} copy` : undefined,
        }
      : blankEntry(sess);
    // seed the live maps so persist keeps the cloned state
    if (cloned.filesCwd) filesCwdByKey[slot] = cloned.filesCwd;
    if (cloned.elevated) elevatedByKey[slot] = true;
    if (cloned.name) nameByKey[slot] = cloned.name;
    ws = { ...ws, entries: { ...ws.entries, [slot]: cloned } };
    sessions = [...sessions, sess];
    activeId = sess.id;
    markOpen(slot);
    ws = { ...ws, active: slot };
    persist();
  }

  // Close a session AND forget it: drop its saved layout so it won't be restored.
  function forgetSession(id: string) {
    const gone = sessions.find((x) => x.id === id);
    onClose(id);
    if (!gone) return;
    const slot = slotOf(gone);
    const rest = { ...ws.entries };
    delete rest[slot];
    delete filesCwdByKey[slot];
    delete elevatedByKey[slot];
    delete nameByKey[slot];
    ws = { ...ws, entries: rest, active: ws.active === slot ? null : ws.active };
    void saveWorkspace(ws);
  }

  onMount(() => {
    void (async () => {
      const w = await loadWorkspace();
      ws = w;
      for (const [key, e] of Object.entries(w.entries)) {
        if (e.filesCwd) filesCwdByKey[key] = e.filesCwd;
        if (e.elevated) elevatedByKey[key] = true;
        if (e.name) nameByKey[key] = e.name;
      }
      loaded = true;
      // reconnect the sessions that were open at last close, keeping each slot,
      // then focus the last active one
      const hist = await listSshHistory().catch(() => []);
      for (const wsKey of w.openKeys) {
        const e = w.entries[wsKey];
        if (!e) continue;
        const s = await connectByIdentity(e.kind, e.identity, hist);
        if (s) sessions = [...sessions, { ...s, wsKey }];
      }
      activeId =
        sessions.find((s) => slotOf(s) === w.active)?.id ??
        sessions.at(-1)?.id ??
        null;
    })();

    const timer = setInterval(persist, 5000);
    const onHide = () => persist();
    window.addEventListener("pagehide", onHide);
    document.addEventListener("visibilitychange", onHide);
    return () => {
      clearInterval(timer);
      window.removeEventListener("pagehide", onHide);
      document.removeEventListener("visibilitychange", onHide);
      persist();
    };
  });

  // add/remove a session from the auto-reconnect set (by unique wsKey)
  function markOpen(slot: string) {
    if (!ws.openKeys.includes(slot))
      ws = { ...ws, openKeys: [...ws.openKeys, slot] };
  }
  function unmarkOpen(slot: string) {
    ws = { ...ws, openKeys: ws.openKeys.filter((k) => k !== slot) };
  }
  function selectSession(id: string) {
    activeId = id;
    const s = sessions.find((x) => x.id === id);
    if (s) ws = { ...ws, active: slotOf(s) };
  }

  // The entry currently shown in the main (hex/preview) panel.
  let preview = $state<{ session: Session; path: string; name: string } | null>(
    null,
  );
  let status = $state("ready");

  // resizable layout
  let leftWidth = $state(320);
  let termHeight = $state(220);

  // workspace size (layout px, unaffected by zoom) → dynamic resize limits
  let workspaceEl: HTMLDivElement;
  let workspaceW = $state(1200);
  let workspaceH = $state(600);
  const MIN_LEFT = 180;
  const MIN_TERM = 80;
  const MIN_MAIN = 200; // keep the viewer usable
  const MIN_TOP = 22; // keep only the FILES/VIEWER header bars visible
  const SPLIT = 6;
  let maxLeft = $derived(Math.max(MIN_LEFT, workspaceW - SPLIT - MIN_MAIN));
  let maxTerm = $derived(Math.max(MIN_TERM, workspaceH - SPLIT - MIN_TOP));
  const clampLeft = (v: number) => Math.max(MIN_LEFT, Math.min(maxLeft, v));
  const clampTerm = (v: number) => Math.max(MIN_TERM, Math.min(maxTerm, v));

  // unclamped accumulators so an over-drag must be "paid back" before the size
  // moves again (the divider tracks the mouse, not a clamped residue)
  let rawLeftWidth = leftWidth;
  let rawTermHeight = termHeight;

  $effect(() => {
    if (!workspaceEl) return;
    const ro = new ResizeObserver((entries) => {
      const r = entries[0].contentRect;
      workspaceW = r.width;
      workspaceH = r.height;
    });
    ro.observe(workspaceEl);
    return () => ro.disconnect();
  });

  // re-clamp when the window (and thus the limits) shrink
  $effect(() => {
    const ml = maxLeft;
    const mt = maxTerm;
    untrack(() => {
      leftWidth = Math.max(MIN_LEFT, Math.min(ml, leftWidth));
      termHeight = Math.max(MIN_TERM, Math.min(mt, termHeight));
    });
  });

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
    // reuse a saved-but-closed slot for this device (restores it), else mint a
    // fresh unique key for a brand-new instance
    const slot = pickSlot(s.key);
    s = { ...s, wsKey: slot };
    if (!ws.entries[slot])
      ws = { ...ws, entries: { ...ws.entries, [slot]: blankEntry(s) } };
    sessions = [...sessions, s];
    activeId = s.id;
    dialogOpen = false;
    status = `connected: ${s.label}`;
    markOpen(slot);
    ws = { ...ws, active: slot };
    persist();
  }

  function onClose(id: string) {
    const gone = sessions.find((s) => s.id === id);
    sessions = sessions.filter((s) => s.id !== id);
    if (activeId === id) activeId = sessions.at(-1)?.id ?? null;
    if (preview && !sessions.some((s) => s.id === preview!.session.id))
      preview = null;
    if (gone) unmarkOpen(slotOf(gone)); // keep its entry for reuse, just not auto-reconnected
    persist();
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
    names={sessionNames}
    onSelect={selectSession}
    onAdd={() => (dialogOpen = true)}
    {onClose}
    onRename={renameSession}
    onDuplicate={duplicateSession}
    onForget={forgetSession}
  />

  <div
    class="workspace"
    bind:this={workspaceEl}
    style="grid-template-columns: {leftWidth}px 6px 1fr; grid-template-rows: 1fr 6px {termHeight}px;"
  >
    <div class="tree-cell">
      <FileTree
        bind:this={fileTree}
        session={activeSession}
        onOpen={openPreview}
        onStatus={(s) => (status = s)}
        onSyncTerminal={syncTerminalTo}
        restoreCwd={activeSession
          ? (ws.entries[slotOf(activeSession)]?.filesCwd ?? null)
          : null}
        savedElevated={activeSession
          ? !!ws.entries[slotOf(activeSession)]?.elevated
          : false}
        onCwd={rememberFilesCwd}
        onElevated={rememberElevated}
      />
    </div>

    <Splitter
      axis="x"
      style="grid-column: 2; grid-row: 1;"
      onstart={() => (rawLeftWidth = leftWidth)}
      ondelta={(dx) => {
        rawLeftWidth += dx / zoom;
        leftWidth = clampLeft(rawLeftWidth);
      }}
    />

    <div class="main-cell">
      <MainPanel {preview} />
    </div>

    <Splitter
      axis="y"
      style="grid-column: 1 / span 3; grid-row: 2;"
      onstart={() => (rawTermHeight = termHeight)}
      ondelta={(dy) => {
        rawTermHeight -= dy / zoom;
        termHeight = clampTerm(rawTermHeight);
      }}
    />

    <div class="term-cell">
      <TerminalPanel
        bind:this={terminalPanel}
        session={activeSession}
        {sessions}
        {zoom}
        workspace={ws}
        onCwd={syncFilesTo}
      />
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
