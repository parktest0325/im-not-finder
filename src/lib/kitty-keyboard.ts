import type { Terminal } from "@xterm/xterm";

// Kitty keyboard protocol ("comprehensive keyboard handling").
// https://sw.kovidgoyal.net/kitty/keyboard-protocol/
//
// Why this exists: the legacy VT encoding has a single byte for Return (CR), so
// a bare terminal can't tell Enter from Shift+Enter / Ctrl+Enter — every app
// just sees \r and treats it as "submit". Instead of hard-coding a key→byte
// hack, we implement the protocol the way real terminals (kitty, ghostty, …)
// do: the application *negotiates* enhanced keys by querying / pushing flags,
// and only then do we report modified keys as the standard CSI-u sequences.
// When no app has enabled the protocol nothing changes, so plain shells keep
// their exact legacy behaviour (no stray characters, no surprises).

const FLAG_REPORT_ALL = 0b1000; // "report all keys as escape codes"

export function installKittyKeyboard(
  term: Terminal,
  write: (data: string) => void,
): () => void {
  // Stack of active flag sets; the top entry is the current mode (0 = off).
  const stack: number[] = [];
  const flags = () => (stack.length ? stack[stack.length - 1] : 0);

  const disposers: Array<{ dispose(): void }> = [];

  // CSI ? u  — query: report the currently active flags back to the app.
  disposers.push(
    term.parser.registerCsiHandler({ prefix: "?", final: "u" }, () => {
      write(`\x1b[?${flags()}u`);
      return true;
    }),
  );

  // CSI > <flags> u  — push a new flag set.
  disposers.push(
    term.parser.registerCsiHandler({ prefix: ">", final: "u" }, (params) => {
      stack.push(num(params, 0, 0));
      return true;
    }),
  );

  // CSI < <n> u  — pop n flag sets (default 1).
  disposers.push(
    term.parser.registerCsiHandler({ prefix: "<", final: "u" }, (params) => {
      let n = num(params, 0, 1) || 1;
      while (n-- > 0 && stack.length) stack.pop();
      return true;
    }),
  );

  // CSI = <flags> ; <mode> u  — set flags (mode 1 replace / 2 set bits / 3 clear).
  disposers.push(
    term.parser.registerCsiHandler({ prefix: "=", final: "u" }, (params) => {
      const f = num(params, 0, 0);
      const mode = num(params, 1, 1);
      const cur = flags();
      const next = mode === 2 ? cur | f : mode === 3 ? cur & ~f : f;
      if (stack.length) stack[stack.length - 1] = next;
      else stack.push(next);
      return true;
    }),
  );

  // While the protocol is active, report keys legacy encoding can't express as
  // their standard CSI-u form. We currently encode Return (the one that matters
  // for newline-vs-submit); everything else falls through to xterm's normal
  // handling, which is correct under the common "disambiguate" flag level.
  term.attachCustomKeyEventHandler((e) => {
    if (e.type !== "keydown" || flags() === 0) return true;
    const seq = encode(e, flags());
    if (seq === null) return true; // not ours → let xterm encode it
    e.preventDefault();
    write(seq);
    return false; // suppress xterm's legacy bytes for this key
  });

  return () => {
    for (const d of disposers) d.dispose();
    term.attachCustomKeyEventHandler(() => true);
  };
}

function num(params: (number | number[])[], i: number, dflt: number): number {
  const p = params[i];
  const v = Array.isArray(p) ? p[0] : p;
  return v == null || v === 0 ? dflt : v;
}

// kitty modifier param = 1 + bitmask(shift=1, alt=2, ctrl=4, super=8)
function modParam(e: KeyboardEvent): number {
  return (
    1 +
    ((e.shiftKey ? 1 : 0) |
      (e.altKey ? 2 : 0) |
      (e.ctrlKey ? 4 : 0) |
      (e.metaKey ? 8 : 0))
  );
}

function encode(e: KeyboardEvent, f: number): string | null {
  if (e.key === "Enter") {
    const m = modParam(e);
    if (m > 1) return `\x1b[13;${m}u`; // modified Return → CSI-u
    if (f & FLAG_REPORT_ALL) return "\x1b[13u"; // plain Return when app wants all keys
    return null; // otherwise plain Return stays legacy CR
  }
  return null;
}
