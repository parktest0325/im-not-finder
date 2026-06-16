<script lang="ts">
  import {
    listAdbDevices,
    connectAdb,
    connectSsh,
    connectLocal,
    listSshHistory,
    deleteSshHistory,
    ensureAppSshKey,
    registerSshKey,
    type Session,
    type AdbDevice,
    type SshConnectOpts,
    type SshHistoryEntry,
  } from "../lib/api";

  let {
    onConnected,
    onCancel,
  }: {
    onConnected: (s: Session) => void;
    onCancel: () => void;
  } = $props();

  let tab = $state<"adb" | "ssh" | "local">("adb");
  let busy = $state(false);
  let error = $state("");

  const localShell = navigator.userAgent.includes("Win")
    ? "PowerShell"
    : "your default shell";

  async function connectLocalNow() {
    error = "";
    busy = true;
    try {
      onConnected(await connectLocal());
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }

  // saved SSH connections (no secrets)
  let history = $state<SshHistoryEntry[]>([]);
  let histOpen = $state(false);
  async function loadHistory() {
    try {
      history = await listSshHistory();
    } catch {
      history = [];
    }
  }
  loadHistory();

  function loadFromHistory(h: SshHistoryEntry) {
    tab = "ssh";
    host = h.host;
    port = h.port;
    username = h.username;
    authMode = h.authMode;
    keyPath = h.keyPath ?? "";
    password = "";
    keyPassphrase = "";
    histOpen = false;
  }

  async function removeHistory(id: string) {
    try {
      await deleteSshHistory(id);
    } catch {
      /* ignore */
    }
    await loadHistory();
  }

  function onDialogKeydown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      onCancel();
    } else if (e.key === "Enter" && tab === "ssh" && !busy && host && username) {
      e.preventDefault();
      void connectSshNow();
    } else if (e.key === "Enter" && tab === "local" && !busy) {
      e.preventDefault();
      void connectLocalNow();
    }
  }

  // adb
  let devices = $state<AdbDevice[]>([]);
  async function refreshDevices() {
    error = "";
    busy = true;
    try {
      devices = await listAdbDevices();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
  refreshDevices();

  async function pickDevice(d: AdbDevice) {
    if (d.state !== "device") return;
    error = "";
    busy = true;
    try {
      onConnected(await connectAdb(d.serial));
    } catch (e) {
      error = String(e);
      busy = false;
    }
  }

  // ssh
  let host = $state("");
  let port = $state(22);
  let username = $state("");
  let authMode = $state<"password" | "key">("password");
  let password = $state("");
  let keyPath = $state("");
  let keyPassphrase = $state("");

  // im-not-finder's dedicated key (created on first use); the default key path
  let defaultKey = $state("");
  (async () => {
    try {
      const k = await ensureAppSshKey();
      if (k) {
        defaultKey = k;
        if (!keyPath) {
          keyPath = k;
          authMode = "key";
        }
      }
    } catch {
      /* ignore */
    }
  })();

  // fill the default key whenever key-auth is chosen and no path is set yet
  // (a saved/history connection keeps its own path; only empty → default)
  function useDefaultKeyIfEmpty() {
    if (!keyPath && defaultKey) keyPath = defaultKey;
  }

  // ssh-copy-id fallback: when key auth fails, install the key with a password
  let registerOpen = $state(false);
  let regPassword = $state("");

  async function connectSshNow() {
    error = "";
    busy = true;
    try {
      const opts: SshConnectOpts = { host, port, username };
      if (authMode === "password") opts.password = password;
      else {
        opts.keyPath = keyPath;
        if (keyPassphrase) opts.keyPassphrase = keyPassphrase;
      }
      onConnected(await connectSsh(opts));
    } catch (e) {
      error = String(e);
      busy = false;
      // offer to register the key on the server (one-time, via password)
      if (authMode === "key" && keyPath) registerOpen = true;
    }
  }

  async function registerNow() {
    error = "";
    busy = true;
    try {
      await registerSshKey(host, port, username, regPassword, keyPath);
      registerOpen = false;
      regPassword = "";
      const opts: SshConnectOpts = { host, port, username, keyPath };
      if (keyPassphrase) opts.keyPassphrase = keyPassphrase;
      onConnected(await connectSsh(opts));
    } catch (e) {
      error = `register failed: ${e}`;
      busy = false;
    }
  }
</script>

<!-- dim layer; intentionally does NOT close on click -->
<div class="backdrop" role="presentation"></div>
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="dialog panel"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onkeydown={onDialogKeydown}
>
  <div class="panel-title">
    connect
    <span class="spacer"></span>
    <button class="closebtn" onclick={onCancel}>×</button>
  </div>

  <div class="tabs">
    <button class:on={tab === "adb"} onclick={() => (tab = "adb")}>ADB</button>
    <button class:on={tab === "ssh"} onclick={() => (tab = "ssh")}>SSH</button>
    <button class:on={tab === "local"} onclick={() => (tab = "local")}>LOCAL</button>
  </div>

  <div class="panel-body body">
    {#if tab === "adb"}
      <div class="toolbar">
        <button onclick={refreshDevices} disabled={busy}>↻ refresh</button>
        <span class="dim">{devices.length} device(s)</span>
      </div>
      {#if devices.length === 0}
        <div class="empty dim">
          no devices. plug in a device with USB debugging, or run<br />
          <code>adb devices</code>
        </div>
      {/if}
      {#each devices as d (d.serial)}
        <div
          class="row device"
          class:disabled={d.state !== "device"}
          onclick={() => pickDevice(d)}
          role="button"
          tabindex="0"
          onkeydown={(e) => e.key === "Enter" && pickDevice(d)}
        >
          <span class="badge adb">adb</span>
          <span class="model">{d.model || d.serial}</span>
          <span class="dim serial">{d.serial}</span>
          <span class="spacer"></span>
          <span class="state {d.state}">{d.state}</span>
        </div>
      {/each}
    {:else if tab === "ssh"}
      {#if history.length > 0}
        <div class="histdd">
          <button
            class="histtoggle"
            onclick={() => (histOpen = !histOpen)}
            title="load a saved connection"
          >
            <span>saved connections ({history.length})</span>
            <span class="caret">{histOpen ? "▴" : "▾"}</span>
          </button>
          {#if histOpen}
            <div class="histlist">
              {#each history as h (h.id)}
                <div
                  class="row hist"
                  onclick={() => loadFromHistory(h)}
                  role="button"
                  tabindex="0"
                  onkeydown={(e) => e.key === "Enter" && loadFromHistory(h)}
                  title="click to load"
                >
                  <span class="badge ssh">{h.authMode === "key" ? "key" : "pwd"}</span>
                  <span class="hid">{h.username}@{h.host}:{h.port}</span>
                  <span class="spacer"></span>
                  <button
                    class="x"
                    title="remove from history"
                    onclick={(e) => {
                      e.stopPropagation();
                      void removeHistory(h.id);
                    }}>×</button
                  >
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
      <div class="form">
        <!-- svelte-ignore a11y_autofocus -->
        <label>host<input bind:value={host} placeholder="192.168.0.10" /></label>
        <label class="narrow">port<input type="number" bind:value={port} /></label>
        <label>user<input bind:value={username} placeholder="root" /></label>

        <div class="authmode">
          <label class="radio"
            ><input type="radio" bind:group={authMode} value="password" />
            password</label
          >
          <label class="radio"
            ><input
              type="radio"
              bind:group={authMode}
              value="key"
              onchange={useDefaultKeyIfEmpty}
            /> key file</label
          >
        </div>

        {#if authMode === "password"}
          <label class="full"
            >password<input type="password" bind:value={password} /></label
          >
        {:else}
          <label class="full"
            >private key path<input
              bind:value={keyPath}
              placeholder="C:\Users\me\.ssh\id_ed25519"
            /></label
          >
          <label class="full"
            >passphrase (optional)<input
              type="password"
              bind:value={keyPassphrase}
            /></label
          >
        {/if}

        <div class="actions">
          <button class="primary" onclick={connectSshNow} disabled={busy || !host || !username}
            >connect</button
          >
        </div>

        {#if registerOpen}
          <div class="register full">
            <p class="dim">
              key auth failed — install this key on the server (one-time, needs
              the account password):
            </p>
            <input
              type="password"
              bind:value={regPassword}
              placeholder="server password"
              onkeydown={(e) => e.key === "Enter" && regPassword && registerNow()}
            />
            <div class="actions">
              <button
                onclick={() => {
                  registerOpen = false;
                  regPassword = "";
                }}>cancel</button
              >
              <button
                class="primary"
                onclick={registerNow}
                disabled={busy || !regPassword}>register &amp; connect</button
              >
            </div>
          </div>
        {/if}
      </div>
    {:else}
      <div class="local">
        <p class="dim">
          Open a terminal on this machine ({localShell}) and browse the local
          filesystem.
        </p>
        <div class="actions">
          <button class="primary" onclick={connectLocalNow} disabled={busy}
            >open local session</button
          >
        </div>
      </div>
    {/if}

    {#if error}
      <div class="error">{error}</div>
    {/if}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    z-index: 40;
  }
  .dialog {
    position: fixed;
    z-index: 41;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 440px;
    max-height: 80vh;
    box-shadow: 0 8px 40px rgba(0, 0, 0, 0.6);
  }
  .closebtn {
    border: none;
    background: transparent;
    color: var(--fg-dim);
  }
  .tabs {
    display: flex;
    border-bottom: 1px solid var(--border);
  }
  .tabs button {
    flex: 1;
    border: none;
    border-bottom: 2px solid transparent;
    background: var(--bg-panel);
    border-radius: 0;
    color: var(--fg-dim);
  }
  .tabs button.on {
    color: var(--fg-bright);
    border-bottom-color: var(--accent);
  }
  .histdd {
    position: relative;
    margin-bottom: 10px;
  }
  .histtoggle {
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    background: var(--bg);
    border: 1px solid var(--border);
    color: var(--fg-dim);
    padding: 4px 8px;
  }
  .histtoggle:hover {
    border-color: var(--border-bright);
    color: var(--fg);
  }
  .caret {
    color: var(--fg-dim);
  }
  .histlist {
    position: absolute;
    top: 100%;
    left: 0;
    right: 0;
    z-index: 20;
    margin-top: 2px;
    border: 1px solid var(--border-bright);
    background: var(--bg-panel);
    max-height: 220px;
    overflow: auto;
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.5);
  }
  .hist {
    height: 26px;
  }
  .hist .hid {
    color: var(--fg-bright);
  }
  .hist .badge.ssh {
    font-size: 10px;
    text-transform: uppercase;
    padding: 0 4px;
    border-radius: 2px;
    background: var(--accent);
    color: var(--bg);
    font-weight: 700;
  }
  .hist .x {
    border: none;
    background: transparent;
    color: var(--fg-dim);
    padding: 0 4px;
  }
  .hist .x:hover {
    color: var(--error);
    background: transparent;
  }
  .body {
    padding: 10px;
  }
  .toolbar {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 8px;
  }
  .empty {
    padding: 16px;
    text-align: center;
    line-height: 1.8;
  }
  .device {
    height: 28px;
    border: 1px solid transparent;
  }
  .device:hover {
    border-color: var(--border);
  }
  .device.disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .badge.adb {
    font-size: 10px;
    text-transform: uppercase;
    padding: 0 4px;
    border-radius: 2px;
    background: var(--accent-2);
    color: var(--bg);
    font-weight: 700;
  }
  .model {
    color: var(--fg-bright);
  }
  .serial {
    font-size: 11px;
  }
  .state {
    font-size: 11px;
  }
  .state.device {
    color: var(--accent-2);
  }
  .state.unauthorized,
  .state.offline {
    color: var(--warn);
  }
  .spacer {
    flex: 1;
  }
  .form {
    display: grid;
    grid-template-columns: 1fr 90px;
    gap: 8px;
  }
  .form label {
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: 11px;
    color: var(--fg-dim);
  }
  .form label.full {
    grid-column: 1 / span 2;
  }
  .authmode {
    grid-column: 1 / span 2;
    display: flex;
    gap: 16px;
  }
  .radio {
    flex-direction: row !important;
    align-items: center;
    gap: 6px !important;
    color: var(--fg) !important;
  }
  .actions {
    grid-column: 1 / span 2;
    display: flex;
    justify-content: flex-end;
    margin-top: 4px;
  }
  .primary {
    background: var(--accent);
    color: var(--bg);
    border-color: var(--accent);
    font-weight: 600;
  }
  .register {
    border: 1px solid var(--accent);
    background: rgba(122, 162, 247, 0.06);
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .register p {
    margin: 0;
    line-height: 1.5;
  }
  .register .actions {
    gap: 8px;
  }
  .local p {
    margin: 0 0 14px;
    line-height: 1.5;
  }
  .local .actions {
    display: flex;
    justify-content: flex-end;
  }
  .error {
    margin-top: 10px;
    padding: 6px 8px;
    color: var(--error);
    border: 1px solid var(--error);
    background: rgba(247, 118, 142, 0.08);
    white-space: pre-wrap;
    word-break: break-all;
  }
</style>
