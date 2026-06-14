<script lang="ts">
  // A draggable bar that emits incremental pixel deltas while dragging.
  // Tracks absolute pointer position (more reliable than movementX/Y under
  // HiDPI / WebView2) and captures the pointer for smooth drags.
  let {
    axis = "x",
    style = "",
    ondelta,
    onstart,
  }: {
    axis?: "x" | "y";
    style?: string;
    ondelta: (delta: number) => void;
    onstart?: () => void;
  } = $props();

  let dragging = $state(false);
  let last = 0;

  function onpointerdown(e: PointerEvent) {
    dragging = true;
    last = axis === "x" ? e.clientX : e.clientY;
    onstart?.();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    e.preventDefault();
  }
  function onpointermove(e: PointerEvent) {
    if (!dragging) return;
    const cur = axis === "x" ? e.clientX : e.clientY;
    const delta = cur - last;
    if (delta !== 0) {
      last = cur;
      ondelta(delta);
    }
  }
  function onpointerup(e: PointerEvent) {
    dragging = false;
    try {
      (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    } catch {
      /* ignore */
    }
  }
</script>

<div
  class="splitter {axis}"
  class:dragging
  {style}
  {onpointerdown}
  {onpointermove}
  {onpointerup}
  role="separator"
  aria-orientation={axis === "x" ? "vertical" : "horizontal"}
  tabindex="-1"
>
  <span class="grip"></span>
</div>

<style>
  .splitter {
    position: relative;
    z-index: 5;
    display: flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    touch-action: none;
    user-select: none;
    transition: background 0.12s ease;
  }
  .splitter.x {
    cursor: col-resize;
  }
  .splitter.y {
    cursor: row-resize;
  }
  .splitter:hover,
  .splitter.dragging {
    background: var(--accent);
  }
  /* the centred grip only appears on hover/drag */
  .grip {
    background: transparent;
    border-radius: 2px;
    transition: background 0.12s ease;
  }
  .splitter.x .grip {
    width: 2px;
    height: 22px;
  }
  .splitter.y .grip {
    width: 22px;
    height: 2px;
  }
  .splitter:hover .grip,
  .splitter.dragging .grip {
    background: var(--bg);
  }
</style>
