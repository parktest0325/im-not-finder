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
  import { installKittyKeyboard } from "../lib/kitty-keyboard";

  let {
    session,
    active,
    zoom = 1,
    initialCwd = null,
  }: {
    session: Session;
    active: boolean;
    zoom?: number;
    initialCwd?: string | null; // restore: cd here once the shell is up
  } = $props();

  const BASE_FONT = 13;
  let host: HTMLDivElement;
  let term: XTerm | undefined;
  let fit: FitAddon | undefined;
  let shellId: string | null = null;
  let unlisten: UnlistenFn | undefined;
  let started = false;
  // last directory the shell reported (via the per-prompt OSC hook); used by the
  // "⇄ files" sync button and persisted so a reconnect can restore it.
  let cwd: string | null = null;

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
        fontSize: Math.max(6, Math.round(BASE_FONT * zoom)),
        theme,
        cursorBlink: true,
        scrollback: 5000,
      });
      fit = new FitAddon();
      term.loadAddon(fit);
      term.open(host);
      safeFit();
      // Negotiate the kitty keyboard protocol so apps that ask for it (Claude
      // Code, etc.) get proper modifier-encoded keys — e.g. Shift+Enter as a
      // real CSI-u sequence instead of a plain CR. No-op until an app enables it.
      installKittyKeyboard(term, (data) => {
        if (shellId) void shellWrite(shellId, data);
      });
      // The shell emits this private OSC on every prompt (see installCwdHook),
      // carrying $PWD. The parser consumes it invisibly, so we always know the
      // live directory without any on-screen noise.
      term.parser.registerOscHandler(7771, (data) => {
        if (data) cwd = data;
        return true;
      });
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

  // App zoom: CSS `zoom` on an xterm ancestor corrupts mouse/selection coords,
  // so we counter-zoom the host to a NET scale of 1 (and grow the font instead).
  // The host is sized 100%*Z then zoomed 1/Z, so it still fills the panel.
  $effect(() => {
    const z = zoom || 1;
    if (host) {
      host.style.zoom = String(1 / z);
      host.style.width = `calc(100% * ${z})`;
      host.style.height = `calc(100% * ${z})`;
    }
    if (term) {
      term.options.fontSize = Math.max(6, Math.round(BASE_FONT * z));
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
      // host is counter-zoomed to net scale 1, so px are screen px: subtract the
      // fixed host padding (4px x / 2px y) plus a touch to avoid overflow.
      const cols = Math.max(2, Math.floor((rect.width - 10) / cellW));
      const rows = Math.max(1, Math.floor((rect.height - 6) / cellH));
      if (cols !== term.cols || rows !== term.rows) term.resize(cols, rows);
    } catch {
      try {
        fit?.fit();
      } catch {
        /* ignore */
      }
    }
  }

  const isPowerShell = () =>
    session.kind === "local" && /pwsh|powershell/i.test(session.label);

  // One-time setup so the shell reports $PWD via OSC 7771 on every prompt.
  // PowerShell: redefine `prompt`; bash/zsh: PROMPT_COMMAND + precmd; ADB's mksh
  // (no PROMPT_COMMAND) embeds a command substitution in PS1, which mksh
  // re-evaluates each prompt.
  function cwdHookCmd(): string | null {
    if (isPowerShell())
      return `function prompt { [Console]::Out.Write([char]27 + ']7771;' + ($PWD.Path -replace '\\\\','/') + [char]7); 'PS ' + $PWD.Path + '> ' }`;
    if (session.kind === "adb")
      return `PS1='$(printf "\\033]7771;%s\\007" "$PWD")'"$PS1"`;
    return `PROMPT_COMMAND='printf "\\033]7771;%s\\007" "$PWD"'; precmd(){ printf '\\033]7771;%s\\007' "$PWD"; }`;
  }

  async function start() {
    if (started || !term) return;
    started = true;
    safeFit();
    term.write(`\x1b[2m── ${session.label} ──\x1b[0m\r\n`);
    try {
      // subscribe BEFORE spawning so the first prompt isn't lost (matters when
      // several terminals open at once, e.g. restoring an ADB session)
      const id = `${session.id}::${crypto.randomUUID()}`;
      unlisten = await listen<number[]>(`term://${id}`, (ev) => {
        term?.write(Uint8Array.from(ev.payload));
      });
      shellId = await shellOpen(session.id, id, term.cols || 80, term.rows || 24);
      // teach the shell to report its cwd, then restore the saved directory
      const hook = cwdHookCmd();
      if (hook) void shellWrite(shellId, hook + "\r");
      if (initialCwd) {
        const q = isPowerShell()
          ? initialCwd.replace(/'/g, "''")
          : initialCwd.replace(/'/g, "'\\''");
        cwd = initialCwd;
        void shellWrite(shellId, `cd '${q}'\r`);
      }
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

  // The directory the shell last reported (live, from the OSC 7771 hook).
  export function getCwd(): string | null {
    return cwd;
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
    /* width/height/zoom are set in JS to counter-zoom to a net scale of 1 */
    position: absolute;
    top: 0;
    left: 0;
    width: 100%;
    height: 100%;
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
