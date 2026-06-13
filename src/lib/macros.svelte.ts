// Stream-Deck-style terminal macros: reactive state + shortcut helpers.
// Persisted to localStorage; shared across components via Svelte 5 module $state.

export interface Macro {
  id: string;
  label: string;
  color: string; // hex, or "" for default
  commands: string; // newline-separated shell lines
  shortcut: string | null; // canonical combo, e.g. "Ctrl+Shift+Digit1"
  pinnedTabId: string | null; // run in this terminal tab, else the focused one
}

export const MACRO_COUNT = 10;
const KEY = "imnf.macros.v1";

function blank(i: number): Macro {
  return {
    id: `m${i}`,
    label: "",
    color: "",
    commands: "",
    shortcut: null,
    pinnedTabId: null,
  };
}

function loadInit(): Macro[] {
  try {
    const raw = localStorage.getItem(KEY);
    if (raw) {
      const arr = JSON.parse(raw) as Macro[];
      return Array.from({ length: MACRO_COUNT }, (_, i) => arr[i] ?? blank(i));
    }
  } catch {
    /* ignore */
  }
  return Array.from({ length: MACRO_COUNT }, (_, i) => blank(i));
}

export const macroState = $state<{ list: Macro[] }>({ list: loadInit() });

// true while the editor is capturing a new shortcut, so the global hotkey
// handler doesn't fire an existing macro mid-recording.
export const recorder = $state<{ active: boolean }>({ active: false });

export function saveMacros() {
  try {
    localStorage.setItem(KEY, JSON.stringify(macroState.list));
  } catch {
    /* ignore */
  }
}

export function setMacro(index: number, m: Macro) {
  macroState.list[index] = m;
  saveMacros();
}

export function clearMacro(index: number) {
  macroState.list[index] = blank(index);
  saveMacros();
}

/** Forget a pinned terminal across all macros (when that tab is closed). */
export function unpinTab(tabId: string) {
  let changed = false;
  for (const m of macroState.list) {
    if (m.pinnedTabId === tabId) {
      m.pinnedTabId = null;
      changed = true;
    }
  }
  if (changed) saveMacros();
}

export function isConfigured(m: Macro): boolean {
  return m.commands.trim().length > 0 || m.label.trim().length > 0;
}

/** Lines to type into the shell (each terminated by Enter). */
export function macroRunText(m: Macro): string {
  return m.commands
    .split(/\r?\n/)
    .filter((l) => l.trim().length > 0)
    .map((l) => l + "\r")
    .join("");
}

// ---------------- shortcut helpers ----------------

const MODS = ["Control", "Shift", "Alt", "Meta"];

export function isFunctionKey(code: string): boolean {
  return /^F([1-9]|1[0-9]|2[0-4])$/.test(code);
}

/** Canonical combo string for an event, or null for reserved keys. */
export function comboFromEvent(e: KeyboardEvent): string | null {
  if (e.key === "Escape" || MODS.includes(e.key)) return null;
  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.metaKey) parts.push("Meta");
  if (e.shiftKey) parts.push("Shift");
  parts.push(e.code || e.key);
  return parts.join("+");
}

/** A shortcut must use Ctrl/Alt/Meta, or be a function key (so plain typing
 *  in the terminal is never hijacked). ESC and lone modifiers are rejected. */
export function isValidShortcut(e: KeyboardEvent): boolean {
  if (e.key === "Escape" || MODS.includes(e.key)) return false;
  return e.ctrlKey || e.altKey || e.metaKey || isFunctionKey(e.code);
}

const PRETTY: Record<string, string> = {
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  Space: "Space",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Backslash: "\\",
  Minus: "-",
  Equal: "=",
  BracketLeft: "[",
  BracketRight: "]",
  Semicolon: ";",
  Quote: "'",
  Backquote: "`",
  Enter: "↵",
  Tab: "Tab",
  Meta: "Win",
};

export function prettyCombo(combo: string | null): string {
  if (!combo) return "";
  return combo
    .split("+")
    .map((p) => {
      if (p.startsWith("Key")) return p.slice(3);
      if (p.startsWith("Digit")) return p.slice(5);
      if (p.startsWith("Numpad")) return "Num" + p.slice(6);
      return PRETTY[p] ?? p;
    })
    .join("+");
}
