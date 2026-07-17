<script lang="ts">
  import MarkdownEditor from './MarkdownEditor.svelte';

  let {
    value = $bindable(''),
    onStarClick
  }: { value?: string; onStarClick?: (id: number) => void } = $props();

  let editorRef = $state<MarkdownEditor | null>(null);

  /** Anchor a gutter star at the caret's line. */
  export function addStar(id: number) {
    editorRef?.addStar(id);
  }

  /** Drop a gutter star's widget. */
  export function removeStar(id: number) {
    editorRef?.removeStar(id);
  }

  /** The block index each star sits on (persisted at stop). */
  export function getStarBlocks(): Map<number, number> {
    return editorRef?.getStarBlocks() ?? new Map();
  }

  /** The star already on the caret's line, if any (Ctrl+S toggles it). */
  export function starAtCursor(): number | null {
    return editorRef?.starAtCursor() ?? null;
  }
</script>

<MarkdownEditor
  bind:this={editorRef}
  bind:value
  autofocus
  {onStarClick}
  placeholder="Take notes — they'll be woven into the summary."
/>
