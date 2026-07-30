<script lang="ts">
  import MarkdownEditor from './MarkdownEditor.svelte';
  import { copy } from '$lib/copy';

  const t = $derived(copy.meetings.notes);

  let {
    value = $bindable(''),
    onStarClick,
    onPasteError
  }: {
    value?: string;
    onStarClick?: (id: number) => void;
    onPasteError?: (message: string) => void;
  } = $props();

  let editorRef = $state<MarkdownEditor | null>(null);

  /** Present, so paste is enabled; `undefined`, because the recording in
   * flight owns the meeting id and the backend reads it from the recovery
   * scratch. There is no id here to give. */
  const liveRecording = () => undefined;

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
  {onPasteError}
  placeholder={t.placeholder}
  pasteMeetingId={liveRecording}
/>
