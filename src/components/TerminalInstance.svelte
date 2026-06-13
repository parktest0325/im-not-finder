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
  }: {
    session: Session;
    active: boolean;
  } = $props();

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
        fontSize: 13,
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

  function safeFit() {
    try {
      fit?.fit();
    } catch {
      /* element not measurable yet */
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
