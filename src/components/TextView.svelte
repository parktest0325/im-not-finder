<script lang="ts">
  import { readChunk, type Session } from "../lib/api";

  let { session, path }: { session: Session; path: string } = $props();

  let limit = $state(131072); // 128 KB initial window
  let text = $state("");
  let loadedLen = $state(0);
  let loading = $state(false);
  let error = $state("");
  let maybeMore = $state(false);

  const decoder = new TextDecoder("utf-8", { fatal: false });

  async function load(n: number) {
    loading = true;
    error = "";
    try {
      const data = await readChunk(session.id, path, 0, n);
      loadedLen = data.length;
      maybeMore = data.length >= n; // hit the window; more may follow
      text = decoder.decode(data);
    } catch (e) {
      error = String(e);
      text = "";
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void session;
    void path;
    limit = 131072;
    void load(limit);
  });

  let lineCount = $derived(text.length ? text.split("\n").length : 0);
</script>

<div class="text">
  <div class="toolbar">
    <span class="dim mono-num">{lineCount} lines · {loadedLen} bytes</span>
    <span class="spacer"></span>
    {#if maybeMore}
      <button
        disabled={loading}
        onclick={() => {
          limit *= 2;
          void load(limit);
        }}>load more</button
      >
    {/if}
  </div>

  {#if error}
    <div class="err">{error}</div>
  {:else if loading && text === ""}
    <div class="dim pad">loading…</div>
  {:else if text === ""}
    <div class="dim pad">empty</div>
  {:else}
    <div class="scroll">
      <pre class="gutter">{Array.from({ length: lineCount }, (_, i) => i + 1).join("\n")}</pre>
      <pre class="content">{text}</pre>
    </div>
  {/if}
</div>

<style>
  .text {
    height: 100%;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .toolbar {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 8px;
    border-bottom: 1px solid var(--border);
  }
  .spacer {
    flex: 1;
  }
  .scroll {
    flex: 1 1 auto;
    overflow: auto;
    display: flex;
    align-items: flex-start;
  }
  .pad {
    padding: 12px;
  }
  .err {
    padding: 12px;
    color: var(--error);
    white-space: pre-wrap;
    word-break: break-all;
  }
  pre {
    margin: 0;
    line-height: 1.5;
  }
  .gutter {
    flex: 0 0 auto;
    padding: 6px 8px;
    text-align: right;
    color: var(--fg-dim);
    background: var(--bg);
    border-right: 1px solid var(--border);
    user-select: none;
    position: sticky;
    left: 0;
  }
  .content {
    flex: 1 1 auto;
    padding: 6px 10px;
    color: var(--fg);
    white-space: pre;
    tab-size: 4;
  }
</style>
