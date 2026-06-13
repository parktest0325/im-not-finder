<script lang="ts">
  import {
    macroState,
    recorder,
    setMacro,
    clearMacro,
    comboFromEvent,
    isValidShortcut,
    prettyCombo,
  } from "../lib/macros.svelte";

  let {
    index,
    tabs,
    onClose,
  }: {
    index: number;
    tabs: { id: string; name: string; kind: string; sess: string }[];
    onClose: () => void;
  } = $props();

  const m = macroState.list[index];
  let label = $state(m.label);
  let color = $state(m.color);
  let commands = $state(m.commands);
  let shortcut = $state<string | null>(m.shortcut);
  let pinnedTabId = $state<string | null>(m.pinnedTabId);
  let recording = $state(false);
  let hint = $state("");

  const COLORS = [
    "",
    "#7aa2f7",
    "#9ece6a",
    "#e0af68",
    "#f7768e",
    "#bb9af7",
    "#7dcfff",
    "#ff9e64",
  ];

  let conflict = $derived(
    shortcut
      ? macroState.list.find((x, i) => i !== index && x.shortcut === shortcut)
      : undefined,
  );

  function startRec() {
    recording = true;
    recorder.active = true;
    hint = "";
  }
  function stopRec() {
    recording = false;
    recorder.active = false;
  }
  function onRecKey(e: KeyboardEvent) {
    if (!recording) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape") {
      stopRec();
      return;
    }
    if (isValidShortcut(e)) {
      shortcut = comboFromEvent(e);
      stopRec();
    } else {
      hint = "use Ctrl / Alt / Meta, or an F-key";
    }
  }

  function save() {
    setMacro(index, {
      id: m.id,
      label: label.trim(),
      color,
      commands,
      shortcut,
      pinnedTabId: pinnedTabId || null,
    });
    onClose();
  }
  function clearAll() {
    clearMacro(index);
    onClose();
  }
</script>

<div class="backdrop" role="presentation"></div>
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="dialog panel"
  role="dialog"
  aria-modal="true"
  tabindex="-1"
  onkeydown={(e) => {
    if (e.key === "Escape" && !recording) onClose();
  }}
>
  <div class="panel-title">
    edit macro {index + 1}
    <span class="spacer"></span>
    <button class="closebtn" onclick={onClose}>×</button>
  </div>

  <div class="panel-body body">
    <label>label<input bind:value={label} placeholder="deploy" maxlength="18" /></label>

    <div class="field">
      <span class="lbl">color</span>
      <div class="swatches">
        {#each COLORS as c (c)}
          <button
            class="sw"
            class:on={color === c}
            class:none={c === ""}
            style={c ? `background:${c}` : ""}
            title={c || "default"}
            aria-label={c || "default"}
            onclick={() => (color = c)}
          ></button>
        {/each}
      </div>
    </div>

    <label
      >commands (one per line, run top-to-bottom)
      <textarea
        bind:value={commands}
        rows="5"
        placeholder={"cd /var/log\ntail -n 50 syslog"}
      ></textarea>
    </label>

    <div class="field">
      <span class="lbl">shortcut</span>
      <div class="shortcut">
        <button
          class="rec"
          class:on={recording}
          onkeydown={onRecKey}
          onclick={startRec}
          onblur={stopRec}
        >
          {#if recording}
            press keys…
          {:else if shortcut}
            {prettyCombo(shortcut)}
          {:else}
            click & press keys
          {/if}
        </button>
        {#if shortcut}
          <button class="mini" title="clear" onclick={() => (shortcut = null)}
            >×</button
          >
        {/if}
      </div>
    </div>
    {#if hint}<div class="hint warn">{hint}</div>{/if}
    {#if conflict}<div class="hint warn">also used by macro “{conflict.label || conflict.id}”</div>{/if}

    <label
      >run in
      <select bind:value={pinnedTabId}>
        <option value="">focused terminal (default)</option>
        {#each tabs as t (t.id)}
          <option value={t.id}>{t.name} — {t.sess}</option>
        {/each}
      </select>
    </label>

    <div class="actions">
      <button class="danger" onclick={clearAll}>clear</button>
      <span class="spacer"></span>
      <button onclick={onClose}>cancel</button>
      <button class="primary" onclick={save}>save</button>
    </div>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    z-index: 60;
  }
  .dialog {
    position: fixed;
    z-index: 61;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: 420px;
    max-height: 86vh;
    box-shadow: 0 8px 40px rgba(0, 0, 0, 0.6);
  }
  .closebtn {
    border: none;
    background: transparent;
    color: var(--fg-dim);
  }
  .body {
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: 11px;
    color: var(--fg-dim);
  }
  textarea {
    font-family: var(--font-mono);
    font-size: 13px;
    color: var(--fg-bright);
    background: var(--bg);
    border: 1px solid var(--border);
    padding: 4px 6px;
    resize: vertical;
  }
  .field {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .lbl {
    font-size: 11px;
    color: var(--fg-dim);
    width: 60px;
  }
  .swatches {
    display: flex;
    gap: 5px;
  }
  .sw {
    width: 20px;
    height: 20px;
    border-radius: 3px;
    border: 1px solid var(--border);
    padding: 0;
  }
  .sw.on {
    outline: 2px solid var(--fg-bright);
    outline-offset: 1px;
  }
  .sw.none {
    background: repeating-linear-gradient(
      45deg,
      var(--bg-soft),
      var(--bg-soft) 4px,
      var(--border) 4px,
      var(--border) 5px
    );
  }
  .shortcut {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .rec {
    min-width: 150px;
  }
  .rec.on {
    border-color: var(--accent);
    color: var(--accent);
  }
  .mini {
    border: none;
    background: transparent;
    color: var(--fg-dim);
    padding: 0 4px;
  }
  .hint {
    font-size: 11px;
  }
  .hint.warn {
    color: var(--warn);
  }
  .actions {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 2px;
  }
  .actions .spacer {
    flex: 1;
  }
  .primary {
    background: var(--accent);
    color: var(--bg);
    border-color: var(--accent);
    font-weight: 600;
  }
  .danger {
    color: var(--error);
    border-color: var(--error);
  }
  .panel-title .spacer {
    flex: 1;
  }
</style>
