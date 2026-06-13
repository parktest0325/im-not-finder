<script lang="ts">
  import { readChunk, type Session } from "../lib/api";

  let { session, path }: { session: Session; path: string } = $props();

  const PAGE = 4096; // bytes per page (256 rows of 16)
  let offset = $state(0);
  let bytes = $state<Uint8Array>(new Uint8Array());
  let loading = $state(false);
  let error = $state("");
  let atEnd = $state(false);

  async function loadPage(off: number) {
    loading = true;
    error = "";
    try {
      const data = await readChunk(session.id, path, off, PAGE);
      bytes = data;
      offset = off;
      atEnd = data.length < PAGE;
    } catch (e) {
      error = String(e);
      bytes = new Uint8Array();
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    // (re)load whenever the bound file changes
    void session;
    void path;
    void loadPage(0);
  });

  function hex(n: number): string {
    return n.toString(16).padStart(2, "0");
  }
  function ascii(n: number): string {
    return n >= 0x20 && n < 0x7f ? String.fromCharCode(n) : ".";
  }

  let rows = $derived.by(() => {
    const out: { addr: string; cells: number[] }[] = [];
    for (let i = 0; i < bytes.length; i += 16) {
      out.push({
        addr: (offset + i).toString(16).padStart(8, "0"),
        cells: Array.from(bytes.slice(i, i + 16)),
      });
    }
    return out;
  });
</script>

<div class="hex">
  <div class="toolbar">
    <button disabled={offset === 0 || loading} onclick={() => loadPage(Math.max(0, offset - PAGE))}
      >◀ prev</button
    >
    <span class="dim mono-num">0x{offset.toString(16).padStart(8, "0")}</span>
    <button disabled={atEnd || loading} onclick={() => loadPage(offset + PAGE)}>next ▶</button>
    <span class="spacer"></span>
    <span class="dim">{bytes.length} bytes</span>
  </div>

  {#if error}
    <div class="err">{error}</div>
  {:else if loading && bytes.length === 0}
    <div class="dim pad">loading…</div>
  {:else if bytes.length === 0}
    <div class="dim pad">empty</div>
  {:else}
    <div class="grid">
      {#each rows as r (r.addr)}
        <div class="line">
          <span class="addr">{r.addr}</span>
          <span class="bytes">
            {#each Array(16) as _, c (c)}
              {#if c < r.cells.length}
                <span class="b" class:zero={r.cells[c] === 0}>{hex(r.cells[c])}</span>
              {:else}
                <span class="b empty">  </span>
              {/if}
              {#if c === 7}<span class="gap"></span>{/if}
            {/each}
          </span>
          <span class="ascii">
            {#each r.cells as cell (cell)}<span
                class="a"
                class:zero={cell === 0}>{ascii(cell)}</span
              >{/each}
          </span>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .hex {
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
  .grid {
    flex: 1 1 auto;
    overflow: auto;
    padding: 4px 8px;
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
  .line {
    display: flex;
    gap: 16px;
    white-space: pre;
    line-height: 1.5;
  }
  .addr {
    color: var(--fg-dim);
  }
  .bytes .b {
    margin-right: 5px;
  }
  .bytes .b.zero,
  .ascii .a.zero {
    color: var(--fg-dim);
    opacity: 0.5;
  }
  .gap {
    display: inline-block;
    width: 6px;
  }
  .ascii {
    color: var(--accent-2);
  }
</style>
