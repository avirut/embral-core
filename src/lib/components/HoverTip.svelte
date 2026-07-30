<script lang="ts">
  // The single floating label behind `use:tip` (see $lib/tip.svelte.ts).
  // Styled to be indistinguishable from the vendored Tooltip.Content, so
  // the app reads as having one tooltip. Scrolling moves the anchor out
  // from under the label, so any scroll hides it.
  import { hoverTip, hideTip } from '$lib/tip.svelte';
</script>

<svelte:window onscrollcapture={hideTip} onresize={hideTip} />

{#if hoverTip.current}
  <div
    class="pointer-events-none fixed z-50 flex -translate-x-1/2 -translate-y-full flex-col items-center pb-1"
    style="left: {hoverTip.current.x}px; top: {hoverTip.current.y}px;"
  >
    <div class="w-max max-w-xs rounded-md bg-foreground px-3 py-1.5 text-xs text-background">
      {hoverTip.current.text}
    </div>
    <div class="-mt-[5px] size-2.5 rotate-45 rounded-[2px] bg-foreground"></div>
  </div>
{/if}
