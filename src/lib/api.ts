// Thin typed wrappers over the Tauri command layer (src-tauri/src/commands.rs).
import { invoke } from "@tauri-apps/api/core";

export type SessionKind = "ssh" | "adb" | "local";

export interface Session {
  id: string;
  kind: SessionKind;
  label: string; // e.g. "user@host" or "Pixel 7 (adb)"
  home: string; // initial directory to open
  key: string; // connection identity (for reconnect): adb serial / ssh id / local
  wsKey?: string; // per-instance storage slot (key, or key#2… on collision)
}

export type EntryKind = "file" | "dir" | "symlink" | "other";

export interface DirEntry {
  name: string;
  path: string; // absolute path on the remote
  kind: EntryKind;
  size: number;
  mode: number; // unix mode bits (0 if unknown)
  mtime: number; // unix seconds (0 if unknown)
}

export interface AdbDevice {
  serial: string;
  state: string; // "device", "unauthorized", "offline", ...
  model: string;
}

export interface SshConnectOpts {
  host: string;
  port: number;
  username: string;
  // exactly one auth method
  password?: string;
  keyPath?: string;
  keyPassphrase?: string;
}

export interface ExecResult {
  stdout: string;
  stderr: string;
  code: number;
}

export interface SshHistoryEntry {
  id: string;
  host: string;
  port: number;
  username: string;
  authMode: "password" | "key";
  keyPath?: string;
  lastUsed: number;
}

// ---- session lifecycle ----
export const listAdbDevices = () => invoke<AdbDevice[]>("list_adb_devices");
export const connectAdb = (serial: string) =>
  invoke<Session>("connect_adb", { serial });
export const connectSsh = (opts: SshConnectOpts) =>
  invoke<Session>("connect_ssh", { opts });
export const connectLocal = () => invoke<Session>("connect_local");
export const disconnect = (sessionId: string) =>
  invoke<void>("disconnect_session", { sessionId });

// ---- privilege elevation (FILES/VIEWER) ----
export interface ElevateStatus {
  elevated: boolean;
  needsPassword: boolean;
  message: string;
}
/** Turn elevation on. Pass no password first to auto-detect (SSH NOPASSWD). */
export const elevate = (sessionId: string, password?: string) =>
  invoke<ElevateStatus>("elevate", { sessionId, password });
export const unelevate = (sessionId: string) =>
  invoke<void>("unelevate", { sessionId });

// ---- connection history ----
export const listSshHistory = () =>
  invoke<SshHistoryEntry[]>("list_ssh_history");
export const deleteSshHistory = (id: string) =>
  invoke<void>("delete_ssh_history", { id });
/** im-not-finder's dedicated SSH key path (generated on first use). */
export const ensureAppSshKey = () => invoke<string>("ensure_app_ssh_key");
/** Install <keyPath>.pub into the server's authorized_keys via password (ssh-copy-id). */
export const registerSshKey = (
  host: string,
  port: number,
  username: string,
  password: string,
  keyPath: string,
) =>
  invoke<void>("register_ssh_key", { host, port, username, password, keyPath });

// ---- workspace (open sessions + per-session layout, restored on relaunch) ----
export interface WsTerminal {
  name: string;
  cwd: string | null;
}
export interface WsEntry {
  kind: SessionKind;
  identity: string; // connection identity (reuse-match + reconnect): serial / ssh id / local
  terminals: WsTerminal[];
  filesCwd: string | null;
  elevated: boolean; // su / root state to re-apply on reconnect
  name?: string; // custom session-tab label
}
export interface Workspace {
  active: string | null; // last active wsKey
  openKeys: string[]; // wsKeys open at last persist → auto-reconnect on launch
  entries: Record<string, WsEntry>; // per-instance layout, keyed by unique wsKey
}

export const emptyWorkspace = (): Workspace => ({
  active: null,
  openKeys: [],
  entries: {},
});

function normalizeWs(raw: unknown): Workspace {
  const w = emptyWorkspace();
  if (raw && typeof raw === "object" && "openKeys" in raw) {
    const o = raw as Record<string, unknown>;
    w.active = (o.active as string) ?? null;
    w.entries = (o.entries as Workspace["entries"]) ?? {};
    // dedupe + keep only keys that actually have a saved entry
    const seen = new Set<string>();
    for (const k of Array.isArray(o.openKeys) ? o.openKeys : []) {
      if (typeof k === "string" && !seen.has(k) && w.entries[k]) {
        seen.add(k);
        w.openKeys.push(k);
      }
    }
  }
  // older/incompatible blobs are discarded (start fresh) rather than migrated
  return w;
}

export const loadWorkspace = async (): Promise<Workspace> => {
  try {
    return normalizeWs(JSON.parse(await invoke<string>("load_workspace")));
  } catch {
    return emptyWorkspace();
  }
};
export const saveWorkspace = (ws: Workspace) =>
  invoke<void>("save_workspace", { data: JSON.stringify(ws) });

// ---- filesystem ----
export const listDir = (sessionId: string, path: string) =>
  invoke<DirEntry[]>("list_dir", { sessionId, path });

function withTimeout<T>(p: Promise<T>, ms: number, label: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout>;
  const timeout = new Promise<T>((_, reject) => {
    timer = setTimeout(
      () => reject(new Error(`${label} timed out after ${ms / 1000}s`)),
      ms,
    );
  });
  return Promise.race([p, timeout]).finally(() => clearTimeout(timer));
}

/** Read up to `len` bytes from `offset`; returns raw bytes for the hex/text view. */
export const readChunk = async (
  sessionId: string,
  path: string,
  offset: number,
  len: number,
): Promise<Uint8Array> => {
  const arr = await withTimeout(
    invoke<number[]>("read_chunk", { sessionId, path, offset, len }),
    20000,
    "read",
  );
  return Uint8Array.from(arr);
};

/** Download a remote file/dir to a local temp path so the OS can accept a native drag. */
export const stageForDrag = (sessionId: string, path: string) =>
  invoke<string>("stage_for_drag", { sessionId, path });

/** Path to a small PNG used as the native drag-preview image. */
export const dragIcon = () => invoke<string>("drag_icon");

export interface DragItemInput {
  path: string;
  name: string;
  isDir: boolean;
  size: number;
}
/** Windows delayed-rendering drag: files are downloaded only at drop time. */
export const startPromisedDrag = (sessionId: string, items: DragItemInput[]) =>
  invoke<void>("start_promised_drag", { sessionId, items });

/** Upload a local file/dir into a remote directory (drag-in). */
export const upload = (
  sessionId: string,
  localPath: string,
  remoteDir: string,
) => invoke<void>("upload", { sessionId, localPath, remoteDir });

export const removePath = (sessionId: string, path: string) =>
  invoke<void>("remove_path", { sessionId, path });
export const renamePath = (sessionId: string, from: string, to: string) =>
  invoke<void>("rename_path", { sessionId, from, to });
export const copyPath = (sessionId: string, from: string, toDir: string) =>
  invoke<void>("copy_path", { sessionId, from, toDir });

export const exec = (sessionId: string, command: string) =>
  invoke<ExecResult>("exec_command", { sessionId, command });

// ---- interactive shell (terminal) ----
// The caller pre-generates shellId and subscribes to term://<shellId> BEFORE
// calling this, so no early shell output (the first prompt) is ever lost.
export const shellOpen = (
  sessionId: string,
  shellId: string,
  cols: number,
  rows: number,
) => invoke<string>("shell_open", { sessionId, shellId, cols, rows });
export const shellWrite = (shellId: string, data: string) =>
  invoke<void>("shell_write", { shellId, data });
export const shellResize = (shellId: string, cols: number, rows: number) =>
  invoke<void>("shell_resize", { shellId, cols, rows });
export const shellClose = (shellId: string) =>
  invoke<void>("shell_close", { shellId });

// ---- helpers ----
export function joinPath(dir: string, name: string): string {
  if (dir.endsWith("/")) return dir + name;
  return dir + "/" + name;
}

export function parentPath(path: string): string {
  if (path === "/" || path === "") return "/";
  const trimmed = path.replace(/\/+$/, "");
  const idx = trimmed.lastIndexOf("/");
  if (idx <= 0) return "/";
  return trimmed.slice(0, idx);
}

export function formatSize(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["K", "M", "G", "T"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 ? 1 : 0)}${units[i]}`;
}
