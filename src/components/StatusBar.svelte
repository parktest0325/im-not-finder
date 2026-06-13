<script lang="ts">
  import type { Session } from "../lib/api";

  let {
    status,
    session,
  }: {
    status: string;
    session: Session | null;
  } = $props();
</script>

<div class="status">
  <span class="mode {session?.kind ?? 'none'}">
    {session ? session.kind.toUpperCase() : "OFFLINE"}
  </span>
  <span class="msg" title={status}>{status}</span>
  <span class="spacer"></span>
  {#if session}<span class="dim">{session.label}</span>{/if}
</div>

<style>
  .status {
    display: flex;
    align-items: center;
    gap: 10px;
    height: 22px;
    padding: 0 8px;
    background: var(--bg-soft);
    border-top: 1px solid var(--border);
    font-size: 12px;
  }
  .mode {
    font-weight: 700;
    color: var(--bg);
    padding: 0 6px;
    border-radius: 2px;
    background: var(--fg-dim);
  }
  .mode.ssh {
    background: var(--accent);
  }
  .mode.adb {
    background: var(--accent-2);
  }
  .msg {
    color: var(--fg-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .spacer {
    flex: 1;
  }
</style>
