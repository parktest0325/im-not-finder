<script lang="ts">
  import {
    listDir,
    stageForDrag,
    dragIcon,
    upload,
    elevate,
    unelevate,
    removePath,
    renamePath,
    copyPath,
    parentPath,
    joinPath,
    formatSize,
    type Session,
    type DirEntry,
  } from "../lib/api";
  import { startDrag } from "@crabnebula/tauri-plugin-drag";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";

  let {
    session,
    onOpen,
    onStatus,
    onSyncTerminal,
  }: {
    session: Session | null;
    onOpen: (session: Session, path: string, name: string) => void;
    onStatus: (s: string) => void;
    onSyncTerminal: (path: string) => void;
  } = $props();

  let cwd = $state("/");
  let entries = $state<DirEntry[]>([]);
  let loading = $state(false);
  let error = $state("");
  let dropActive = $state(false);
  let panelEl: HTMLDivElement;

  // selection (multi)
  let selected = $state<Set<string>>(new Set());
  let anchorIdx: number | null = null;

  // inline rename
  let renaming = $state<string | null>(null);
  let renameValue = $state("");

  // copy/cut clipboard (remote paths, same session)
  let clipboard = $state<{
    paths: string[];
    mode: "copy" | "cut";
    sessionId: string;
  } | null>(null);

  // right-click context menu
  let menu = $state<{ x: number; y: number; target: DirEntry | null } | null>(
    null,
  );
  // delete confirmation
  let confirmDel = $state<string[] | null>(null);

  // privilege elevation (per session)
  let elevatedBy = $state<Record<string, boolean>>({});
  let elevated = $derived(session ? !!elevatedBy[session.id] : false);
  let pwPrompt = $state(false);
  let pwValue = $state("");
  let pwError = $state("");

  let hasClip = $derived(
    !!clipboard && !!session && clipboard.sessionId === session.id,
  );

  // ---------- elevation ----------
  async function toggleSu() {
    if (!session) return;
    const sid = session.id;
    if (elevated) {
      await unelevate(sid);
      elevatedBy = { ...elevatedBy, [sid]: false };
      onStatus("elevation off");
      void load();
      return;
    }
    try {
      const st = await elevate(sid);
      if (st.elevated) {
        elevatedBy = { ...elevatedBy, [sid]: true };
        onStatus(`elevated — ${st.message}`);
        void load();
      } else if (st.needsPassword) {
        pwPrompt = true;
        pwValue = "";
        pwError = "";
      }
    } catch (e) {
      onStatus(`elevate failed: ${e}`);
    }
  }
  async function submitPw() {
    if (!session) return;
    const sid = session.id;
    try {
      const st = await elevate(sid, pwValue);
      if (st.elevated) {
        elevatedBy = { ...elevatedBy, [sid]: true };
        pwPrompt = false;
        pwValue = "";
        onStatus("elevated — sudo");
        void load();
      } else {
        pwError = st.message || "authentication failed";
      }
    } catch (e) {
      pwError = String(e);
    }
  }

  function autofocus(node: HTMLInputElement) {
    node.focus();
    node.select?.();
  }

  // ---------- navigation / loading ----------
  let lastSession: string | null = null;
  $effect(() => {
    if (session && session.id !== lastSession) {
      lastSession = session.id;
      cwd = session.home || "/";
      void load();
    } else if (!session) {
      lastSession = null;
      entries = [];
    }
  });

  async function load() {
    if (!session) return;
    loading = true;
    error = "";
    closeMenu();
    try {
      const list = await listDir(session.id, cwd);
      list.sort((a, b) => {
        const ad = a.kind === "dir" ? 0 : 1;
        const bd = b.kind === "dir" ? 0 : 1;
        if (ad !== bd) return ad - bd;
        return a.name.toLowerCase().localeCompare(b.name.toLowerCase());
      });
      entries = list;
      // drop selections that no longer exist
      const present = new Set(list.map((e) => e.path));
      selected = new Set([...selected].filter((p) => present.has(p)));
      onStatus(`${cwd} — ${list.length} items`);
    } catch (e) {
      error = String(e);
      entries = [];
      onStatus(`error: ${e}`);
    } finally {
      loading = false;
    }
  }

  function navigate(path: string) {
    cwd = path;
    selected = new Set();
    anchorIdx = null;
    void load();
  }
  const up = () => navigate(parentPath(cwd));
  const goHome = () => session && navigate(session.home || "/");

  // click the path to copy it (with a brief visual confirmation)
  let copied = $state(false);
  let copyTimer: ReturnType<typeof setTimeout> | undefined;
  async function copyCwd() {
    if (!session) return;
    try {
      await writeText(cwd);
      copied = true;
      onStatus(`copied path: ${cwd}`);
      clearTimeout(copyTimer);
      copyTimer = setTimeout(() => (copied = false), 3000);
    } catch (e) {
      onStatus(`copy failed: ${e}`);
    }
  }

  function onRowActivate(entry: DirEntry) {
    if (entry.kind === "dir") navigate(entry.path);
    else if (session) onOpen(session, entry.path, entry.name);
  }

  // ---------- selection ----------
  function onRowClick(e: MouseEvent, entry: DirEntry, idx: number) {
    if (e.shiftKey && anchorIdx !== null) {
      const [a, b] = [anchorIdx, idx].sort((x, y) => x - y);
      selected = new Set(entries.slice(a, b + 1).map((en) => en.path));
    } else if (e.ctrlKey || e.metaKey) {
      const s = new Set(selected);
      s.has(entry.path) ? s.delete(entry.path) : s.add(entry.path);
      selected = s;
      anchorIdx = idx;
    } else {
      selected = new Set([entry.path]);
      anchorIdx = idx;
    }
  }

  function selectAll() {
    selected = new Set(entries.map((e) => e.path));
    closeMenu();
  }

  // ---------- context menu ----------
  function openMenu(e: MouseEvent, entry: DirEntry | null, idx?: number) {
    e.preventDefault();
    e.stopPropagation();
    if (entry) {
      if (!selected.has(entry.path)) {
        selected = new Set([entry.path]);
        anchorIdx = idx ?? null;
      }
    }
    menu = { x: e.clientX, y: e.clientY, target: entry };
  }
  function closeMenu() {
    menu = null;
  }

  // ---------- operations ----------
  function selectionPaths(): string[] {
    return [...selected];
  }

  function startRename(entry: DirEntry) {
    renaming = entry.path;
    renameValue = entry.name;
    closeMenu();
  }
  async function commitRename(entry: DirEntry) {
    const name = renameValue.trim();
    renaming = null;
    if (!session || !name || name === entry.name) return;
    try {
      await renamePath(session.id, entry.path, joinPath(cwd, name));
      onStatus(`renamed → ${name}`);
      void load();
    } catch (e) {
      onStatus(`rename failed: ${e}`);
    }
  }

  function doCopy() {
    if (!session) return;
    clipboard = { paths: selectionPaths(), mode: "copy", sessionId: session.id };
    onStatus(`copied ${clipboard.paths.length} item(s)`);
    closeMenu();
  }
  function doCut() {
    if (!session) return;
    clipboard = { paths: selectionPaths(), mode: "cut", sessionId: session.id };
    onStatus(`cut ${clipboard.paths.length} item(s)`);
    closeMenu();
  }
  async function doPaste() {
    closeMenu();
    if (!session || !clipboard) return;
    if (clipboard.sessionId !== session.id) {
      onStatus("paste: same session only");
      return;
    }
    const { paths, mode } = clipboard;
    for (const p of paths) {
      const base = p.split("/").pop() || p;
      try {
        if (mode === "copy") await copyPath(session.id, p, cwd);
        else await renamePath(session.id, p, joinPath(cwd, base));
      } catch (e) {
        onStatus(`paste failed: ${e}`);
        return;
      }
    }
    if (mode === "cut") clipboard = null;
    onStatus(`pasted ${paths.length} item(s) → ${cwd}`);
    void load();
  }

  function askDelete() {
    confirmDel = selectionPaths();
    closeMenu();
  }
  async function confirmDeleteYes() {
    if (!session || !confirmDel) return;
    const paths = confirmDel;
    confirmDel = null;
    for (const p of paths) {
      try {
        await removePath(session.id, p);
      } catch (e) {
        onStatus(`delete failed: ${e}`);
        break;
      }
    }
    selected = new Set();
    onStatus(`deleted ${paths.length} item(s)`);
    void load();
  }

  // ---------- drag-in ----------
  $effect(() => {
    let un: (() => void) | undefined;
    let disposed = false;
    getCurrentWebview()
      .onDragDropEvent((ev) => {
        const p = ev.payload as any;
        if (p.type === "enter" || p.type === "over") {
          dropActive = overPanel(p.position);
        } else if (p.type === "leave") {
          dropActive = false;
        } else if (p.type === "drop") {
          const over = overPanel(p.position);
          dropActive = false;
          if (over) void handleDrop(p.paths ?? []);
        }
      })
      .then((u) => (disposed ? u() : (un = u)));
    return () => {
      disposed = true;
      un?.();
    };
  });

  function overPanel(pos?: { x: number; y: number }): boolean {
    if (!panelEl || !pos) return false;
    const dpr = window.devicePixelRatio || 1;
    const z = parseFloat(document.documentElement.style.zoom || "1") || 1;
    const el = document.elementFromPoint(pos.x / dpr / z, pos.y / dpr / z);
    return !!el && panelEl.contains(el);
  }

  async function handleDrop(paths: string[]) {
    if (!session || !paths.length) return;
    for (const pth of paths) {
      const base = pth.split(/[\\/]/).pop() || pth;
      onStatus(`uploading ${base} → ${cwd}…`);
      try {
        await upload(session.id, pth, cwd);
      } catch (e) {
        onStatus(`upload failed: ${e}`);
        return;
      }
    }
    onStatus(`uploaded ${paths.length} item(s)`);
    void load(); // refresh once after an external drop
  }

  // ---------- drag-out ----------
  let iconPath: string | null = null;
  async function ensureIcon(): Promise<string> {
    if (iconPath === null) {
      try {
        iconPath = await dragIcon();
      } catch {
        iconPath = "";
      }
    }
    return iconPath;
  }

  async function onDragStart(e: DragEvent, entry: DirEntry) {
    e.preventDefault();
    if (!session) return;
    // drag the whole selection if the grabbed row is part of it
    const targets =
      selected.has(entry.path) && selected.size > 1
        ? entries.filter((en) => selected.has(en.path))
        : [entry];
    onStatus(`staging ${targets.length} item(s)…`);
    try {
      const icon = await ensureIcon();
      const locals: string[] = [];
      for (const t of targets) locals.push(await stageForDrag(session.id, t.path));
      onStatus(`drag ${targets.length} item(s) — drop onto a folder`);
      await startDrag({ item: locals, icon, mode: "copy" });
    } catch (err) {
      onStatus(`drag failed: ${err}`);
    }
  }
</script>

<svelte:window onclick={() => menu && closeMenu()} />

<div class="panel" class:drop={dropActive} bind:this={panelEl}>
  <div class="panel-title">
    <span>files</span>
    {#if elevated}<span class="rootbadge">root</span>{/if}
    <span class="spacer"></span>
    <button
      class="su"
      class:on={elevated}
      title="elevate (su / sudo) for file operations"
      onclick={toggleSu}
      disabled={!session}>su</button
    >
    <button
      class="mini"
      title="cd terminal to this path"
      onclick={() => session && onSyncTerminal(cwd)}
      disabled={!session}>⇄</button
    >
    <button class="mini" title="home" onclick={goHome} disabled={!session}>⌂</button>
    <button class="mini" title="up" onclick={up} disabled={!session}>↑</button>
    <button
      class="mini"
      title="refresh"
      onclick={() => load()}
      disabled={!session}>↻</button
    >
  </div>

  {#if pwPrompt}
    <div class="pwbar">
      <input
        type="password"
        placeholder="sudo password"
        bind:value={pwValue}
        use:autofocus
        onkeydown={(e) => {
          if (e.key === "Enter") submitPw();
          else if (e.key === "Escape") pwPrompt = false;
        }}
      />
      <button onclick={submitPw}>ok</button>
      <button class="mini" onclick={() => (pwPrompt = false)}>×</button>
      {#if pwError}<span class="pwerr" title={pwError}>{pwError}</span>{/if}
    </div>
  {/if}

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="crumbs"
    class:copied
    dir="ltr"
    title="click to copy path"
    onclick={copyCwd}
  >
    {session ? cwd : "no session"}
  </div>

  {#if confirmDel}
    <div class="confirm">
      <span>delete {confirmDel.length} item(s)?</span>
      <span class="spacer"></span>
      <button class="danger" onclick={confirmDeleteYes}>delete</button>
      <button class="mini" onclick={() => (confirmDel = null)}>cancel</button>
    </div>
  {/if}

  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="panel-body" oncontextmenu={(e) => openMenu(e, null)}>
    {#if !session}
      <div class="hint dim">connect a session to browse files</div>
    {:else if loading}
      <div class="hint dim">loading…</div>
    {:else if error}
      <div class="hint err">{error}</div>
    {:else}
      {#if cwd !== "/"}
        <div
          class="row"
          ondblclick={up}
          role="button"
          tabindex="0"
          onkeydown={(e) => e.key === "Enter" && up()}
        >
          <span class="glyph dim">←</span><span class="name dim">..</span>
        </div>
      {/if}
      {#each entries as entry, i (entry.path)}
        <div
          class="row"
          class:selected={selected.has(entry.path)}
          class:cut={clipboard?.mode === "cut" &&
            clipboard.paths.includes(entry.path)}
          draggable={renaming !== entry.path}
          onclick={(e) => onRowClick(e, entry, i)}
          ondblclick={() => onRowActivate(entry)}
          oncontextmenu={(e) => openMenu(e, entry, i)}
          ondragstart={(e) => onDragStart(e, entry)}
          role="button"
          tabindex="0"
          onkeydown={(e) => e.key === "Enter" && onRowActivate(entry)}
          title={entry.name}
        >
          <span class="glyph" class:dir={entry.kind === "dir"}>
            {entry.kind === "dir" ? "▸" : entry.kind === "symlink" ? "↪" : "·"}
          </span>
          {#if renaming === entry.path}
            <input
              class="rename"
              bind:value={renameValue}
              use:autofocus
              onclick={(e) => e.stopPropagation()}
              onblur={() => commitRename(entry)}
              onkeydown={(e) => {
                if (e.key === "Enter") commitRename(entry);
                else if (e.key === "Escape") renaming = null;
              }}
            />
          {:else}
            <span class="name" class:dirname={entry.kind === "dir"}
              >{entry.name}</span
            >
          {/if}
          <span class="spacer"></span>
          {#if entry.kind !== "dir"}
            <span class="size dim mono-num">{formatSize(entry.size)}</span>
          {/if}
        </div>
      {/each}
      {#if entries.length === 0}
        <div class="hint dim">empty</div>
      {/if}
    {/if}
  </div>

  {#if dropActive}
    <div class="drop-overlay">drop to upload → {cwd}</div>
  {/if}
</div>

{#if menu}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div
    class="ctxmenu"
    style="left: {menu.x}px; top: {menu.y}px;"
    onclick={(e) => e.stopPropagation()}
    role="menu"
    tabindex="-1"
  >
    {#if menu.target}
      {#if selected.size === 1}
        <button
          onclick={() => {
            const t = menu?.target;
            closeMenu();
            if (t) onRowActivate(t);
          }}>open</button
        >
        <button onclick={() => menu && startRename(menu.target!)}>rename</button>
        <div class="sep"></div>
      {/if}
      <button onclick={doCut}>cut</button>
      <button onclick={doCopy}>copy</button>
      {#if hasClip}<button onclick={doPaste}>paste</button>{/if}
      <div class="sep"></div>
      <button class="danger" onclick={askDelete}>delete ({selected.size})</button>
    {:else}
      {#if hasClip}<button onclick={doPaste}>paste</button>{/if}
      <button onclick={selectAll}>select all</button>
      <button onclick={() => (closeMenu(), load())}>refresh</button>
    {/if}
  </div>
{/if}

<style>
  .panel {
    position: relative;
  }
  .panel.drop {
    outline: 2px dashed var(--accent);
    outline-offset: -2px;
  }
  .drop-overlay {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    background: rgba(122, 162, 247, 0.1);
    color: var(--fg-bright);
    pointer-events: none;
    z-index: 10;
  }
  .mini {
    border: none;
    background: transparent;
    color: var(--fg-dim);
    padding: 0 4px;
  }
  .mini:hover:not(:disabled) {
    color: var(--fg-bright);
    background: transparent;
  }
  .su {
    border: 1px solid var(--border);
    background: var(--bg-panel);
    color: var(--fg-dim);
    padding: 0 6px;
    font-size: 11px;
    line-height: 16px;
  }
  .su.on {
    background: var(--error);
    border-color: var(--error);
    color: var(--bg);
    font-weight: 700;
  }
  .rootbadge {
    font-size: 10px;
    text-transform: uppercase;
    background: var(--error);
    color: var(--bg);
    border-radius: 2px;
    padding: 0 4px;
    font-weight: 700;
  }
  .pwbar,
  .confirm {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    background: var(--bg-soft);
    border-bottom: 1px solid var(--border);
  }
  .pwbar input {
    flex: 1 1 auto;
    min-width: 0;
  }
  .pwerr {
    color: var(--error);
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 140px;
  }
  .confirm .spacer {
    flex: 1;
  }
  .danger {
    color: var(--error);
    border-color: var(--error);
  }
  .crumbs {
    flex: 0 0 auto;
    padding: 2px 8px;
    color: var(--accent);
    background: var(--bg);
    border-bottom: 1px solid var(--border);
    white-space: nowrap;
    overflow-x: auto;
    overflow-y: hidden;
    text-align: left;
    direction: ltr;
    cursor: pointer;
    border: 1px solid transparent;
    transition: border-color 0.2s, color 0.2s;
  }
  .crumbs:hover {
    color: var(--fg-bright);
  }
  /* brief confirmation after copying the path */
  .crumbs.copied {
    border-color: var(--accent-2);
    color: var(--accent-2);
  }
  /* thin scrollbar so the full absolute path is reachable */
  .crumbs::-webkit-scrollbar {
    height: 5px;
  }
  .glyph {
    width: 12px;
    text-align: center;
    color: var(--fg-dim);
  }
  .glyph.dir {
    color: var(--accent);
  }
  .name {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .dirname {
    color: var(--fg-bright);
  }
  .rename {
    flex: 1 1 auto;
    min-width: 0;
    padding: 0 2px;
    font-size: 13px;
  }
  .row.cut {
    opacity: 0.5;
  }
  .size {
    font-size: 11px;
    padding-left: 8px;
  }
  .hint {
    padding: 12px;
    text-align: center;
  }
  .hint.err {
    color: var(--error);
    white-space: pre-wrap;
    word-break: break-all;
    text-align: left;
  }
  .ctxmenu {
    position: fixed;
    z-index: 50;
    min-width: 140px;
    background: var(--bg-panel);
    border: 1px solid var(--border-bright);
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.5);
    padding: 3px;
    display: flex;
    flex-direction: column;
  }
  .ctxmenu button {
    border: none;
    background: transparent;
    text-align: left;
    padding: 4px 10px;
    color: var(--fg);
  }
  .ctxmenu button:hover {
    background: var(--bg-hover);
    color: var(--fg-bright);
  }
  .ctxmenu button.danger {
    color: var(--error);
  }
  .ctxmenu .sep {
    height: 1px;
    background: var(--border);
    margin: 3px 0;
  }
</style>
