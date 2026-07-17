<script lang="ts">
  import { onMount } from 'svelte';
  import type { MeetingStar } from '$lib/types';
  import MarkdownEditor from './MarkdownEditor.svelte';

  /** The Notes tab: the user's raw live notes, verbatim and read-only,
   * with their stars re-anchored in the gutter. Clicking a star plays the
   * meeting from that moment. */
  let {
    notes,
    stars = [],
    onStarClick
  }: {
    notes: string;
    stars?: MeetingStar[];
    onStarClick?: (star: MeetingStar) => void;
  } = $props();

  let editorRef = $state<MarkdownEditor | null>(null);

  onMount(() => {
    // Star ids in this view are the star's index in the meeting's list.
    stars.forEach((star, i) => {
      if (star.note_block !== null) {
        editorRef?.addStarAtBlock(i, star.note_block);
      }
    });
  });

  /** Scroll a star's line into view (player star clicks). */
  export function scrollToStar(index: number) {
    editorRef?.scrollToStar(index);
  }
</script>

<MarkdownEditor
  bind:this={editorRef}
  value={notes}
  readonly
  onStarClick={(id) => {
    const star = stars[id];
    if (star) onStarClick?.(star);
  }}
  placeholder="No notes were taken during this meeting."
/>
