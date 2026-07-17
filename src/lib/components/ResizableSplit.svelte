<script lang="ts">
  import { onMount, type Snippet } from 'svelte';

  /** Two panes around a draggable divider. One side keeps a fixed pixel
   * width (persisted per machine in localStorage); the other flexes. Pane
   * classes are overridable so callers can add responsive behavior (the
   * meetings page collapses to a single panel on narrow windows). */
  let {
    left,
    right,
    fixedSide,
    storageKey,
    defaultSize = 340,
    minFixed = 260,
    minFlex = 320,
    fixedClass = 'flex w-(--split-size) shrink-0',
    flexClass = 'flex min-w-0 flex-1',
    dividerClass = ''
  }: {
    left: Snippet;
    right: Snippet;
    fixedSide: 'left' | 'right';
    storageKey: string;
    defaultSize?: number;
    minFixed?: number;
    minFlex?: number;
    fixedClass?: string;
    flexClass?: string;
    dividerClass?: string;
  } = $props();

  // Snapshot on mount is intentional: defaultSize is a static per-usage
  // constant, and the saved width overrides it right away anyway.
  // svelte-ignore state_referenced_locally
  let size = $state(defaultSize);
  let containerEl = $state<HTMLElement | null>(null);
  let dragging = $state(false);

  onMount(() => {
    const saved = Number(localStorage.getItem(storageKey));
    if (Number.isFinite(saved) && saved >= minFixed) {
      size = saved;
    }
  });

  function onPointerDown(e: PointerEvent) {
    dragging = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onPointerMove(e: PointerEvent) {
    if (!dragging || !containerEl) return;
    const rect = containerEl.getBoundingClientRect();
    const max = Math.max(minFixed, rect.width - minFlex);
    const raw =
      fixedSide === 'right' ? rect.right - e.clientX : e.clientX - rect.left;
    size = Math.min(max, Math.max(minFixed, raw));
  }

  function onPointerUp(e: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    localStorage.setItem(storageKey, String(size));
  }
</script>

<div
  bind:this={containerEl}
  class="flex min-h-0 min-w-0 flex-1"
  style="--split-size: {size}px"
>
  <div class="min-h-0 flex-col {fixedSide === 'left' ? fixedClass : flexClass}">
    {@render left()}
  </div>
  <div
    role="separator"
    aria-orientation="vertical"
    aria-label="Resize panels"
    class="w-[3px] shrink-0 cursor-col-resize transition-colors {dragging
      ? 'bg-muted-foreground/50'
      : 'bg-border hover:bg-muted-foreground/40'} {dividerClass}"
    onpointerdown={onPointerDown}
    onpointermove={onPointerMove}
    onpointerup={onPointerUp}
  ></div>
  <div class="min-h-0 flex-col {fixedSide === 'right' ? fixedClass : flexClass}">
    {@render right()}
  </div>
</div>
