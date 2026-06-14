<script lang="ts">
  import { Terminal as XTerm } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { readText, writeText } from "@tauri-apps/plugin-clipboard-manager";
  import {
    shellOpen,
    shellWrite,
    shellResize,
    shellClose,
    type Session,
  } from "../lib/api";

  let {
    session,
    active,
    zoom = 1,
  }: {
    session: Session;
    active: boolean;
    zoom?: number;
  } = $props();

  const BASE_FONT = 13;
  let host: HTMLDivElement;
  let term: XTerm | undefined;
  let fit: FitAddon | undefined;
  let shellId: string | null = null;
  let unlisten: UnlistenFn | undefined;
  let started = false;

  const theme = {
    background: "#0b0e14",
    foreground: "#c4cad6",
    cursor: "#7aa2f7",
    selectionBackground: "#2d3a52",
  };

  $effect(() => {
    if (!term && host) {
      term = new XTerm({
        fontFamily:
          '"Cascadia Mono","JetBrains Mono","Consolas",ui-monospace,monospace',
        fontSize: BASE_FONT,
        theme,
        cursorBlink: true,
        scrollback: 5000,
      });
      fit = new FitAddon();
      term.loadAddon(fit);
      term.open(host);
      safeFit();
      term.onData((d) => {
        if (shellId) void shellWrite(shellId, d);
      });
      const ro = new ResizeObserver(() => {
        safeFit();
        if (shellId && term) void shellResize(shellId, term.cols, term.rows);
      });
      ro.observe(host);
      void start();
    }
  });

  // re-fit and focus whenever this tab becomes the active one
  $effect(() => {
    if (active && term) {
      queueMicrotask(() => {
        safeFit();
        term?.focus();
        if (shellId && term) void shellResize(shellId, term.cols, term.rows);
      });
    }
  });

  // The terminal scales with the app via the document's CSS zoom. Browser zoom
  // doesn't fire ResizeObserver (layout size is unchanged), so refit explicitly
  // whenever the zoom changes.
  $effect(() => {
    void zoom;
    if (term) {
      requestAnimationFrame(() => {
        safeFit();
        if (shellId && term) void shellResize(shellId, term.cols, term.rows);
      });
    }
  });

  // Derive the *actual rendered* cell size from xterm's own screen element
  // (its size ÷ its current cols/rows), then fit the host to it. Everything is
  // read via getBoundingClientRect in the same instant, so the CSS-zoom factor
  // cancels and the grid matches the visible viewport exactly (no overflow).
  function safeFit() {
    if (!term || !host) return;
    try {
      const rect = host.getBoundingClientRect();
      const screen = host.querySelector(
        ".xterm-screen",
      ) as HTMLElement | null;
      if (
        !screen ||
        term.cols < 1 ||
        term.rows < 1 ||
        rect.width < 8 ||
        rect.height < 8
      ) {
        fit?.fit();
        return;
      }
      const sRect = screen.getBoundingClientRect();
      const cellW = sRect.width / term.cols;
      const cellH = sRect.height / term.rows;
      if (cellW <= 0 || cellH <= 0) {
        fit?.fit();
        return;
      }
      // host padding (4px x / 2px y) is in layout px, but rect/cell are zoomed,
      // so scale it; a touch extra guarantees the grid never overflows the host.
      const z = zoom || 1;
      const cols = Math.max(2, Math.floor((rect.width - 10 * z) / cellW));
      const rows = Math.max(1, Math.floor((rect.height - 6 * z) / cellH));
      if (cols !== term.cols || rows !== term.rows) term.resize(cols, rows);
    } catch {
      try {
        fit?.fit();
      } catch {
        /* ignore */
      }
    }
  }

  async function start() {
    if (started || !term) return;
    started = true;
    safeFit();
    term.write(`\x1b[2m── ${session.label} ──\x1b[0m\r\n`);
    try {
      shellId = await shellOpen(session.id, term.cols || 80, term.rows || 24);
      unlisten = await listen<number[]>(`term://${shellId}`, (ev) => {
        term?.write(Uint8Array.from(ev.payload));
      });
    } catch (e) {
      term.write(`\r\n\x1b[31mshell error: ${e}\x1b[0m\r\n`);
    }
  }

  // right-click: copy when there's a selection, otherwise paste
  async function onContextMenu(e: MouseEvent) {
    e.preventDefault();
    if (!term) return;
    if (term.hasSelection()) {
      const sel = term.getSelection();
      if (sel) {
        try {
          await writeText(sel);
        } catch {
          /* ignore */
        }
        term.clearSelection();
      }
    } else if (shellId) {
      try {
        const txt = await readText();
        if (txt) await shellWrite(shellId, txt);
      } catch {
        /* ignore */
      }
    }
  }

  // programmatically type into this shell (e.g. sync `cd` from the file tree)
  export function send(text: string) {
    if (shellId) void shellWrite(shellId, text);
  }

  export async function dispose() {
    unlisten?.();
    if (shellId) {
      try {
        await shellClose(shellId);
      } catch {
        /* ignore */
      }
      shellId = null;
    }
    term?.dispose();
    term = undefined;
  }
</script>

<div
  class="term-host"
  class:active
  bind:this={host}
  oncontextmenu={onContextMenu}
  role="application"
></div>

<style>
  .term-host {
    position: absolute;
    inset: 0;
    padding: 2px 4px;
    background: var(--bg);
    overflow: hidden;
  }
  /* inactive tabs are hidden but kept alive */
  .term-host:not(.active) {
    visibility: hidden;
    z-index: -1;
  }
</style>
