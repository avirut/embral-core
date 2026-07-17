<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { Editor } from '@tiptap/core';
  import StarterKit from '@tiptap/starter-kit';
  import { Markdown } from 'tiptap-markdown';
  import {
    starAnchors,
    starAtCursor as starAtCursorIn,
    anchorStarAtCursor,
    clearStarAnchor,
    starBlockIndexes,
    anchorStarAtBlock
  } from '$lib/editor/starGutter';

  let {
    value = $bindable(''),
    placeholder = '',
    autofocus = false,
    readonly = false,
    onChange,
    onStarClick
  }: {
    value?: string;
    placeholder?: string;
    autofocus?: boolean;
    /** A display surface (the saved user-notes view): no editing. */
    readonly?: boolean;
    onChange?: (value: string) => void;
    /** Enables the star gutter: stars anchor to lines and clicking one
     * calls this with the star's id (remove while recording, seek in the
     * saved-notes view). */
    onStarClick?: (id: number) => void;
  } = $props();

  let editorEl: HTMLDivElement;
  let editor: Editor | undefined;
  let applyingExternalValue = false;

  const isEmpty = $derived(value.trim().length === 0);

  // One writing surface everywhere (live notes, saved notes, the raw
  // transcript fallback): borderless — the pane *is* the editor — with
  // flat, small margins (owner call: text hugs the left; no centering
  // math, which also guarantees the placeholder can't drift — `ch`-based
  // padding resolved differently on the two elements). Long lines stay
  // readable via a measure cap on the blocks themselves.
  const surfacePadding = 'padding: 1.25rem 1.5rem 3rem;';

  onMount(() => {
    const extensions = [
      StarterKit,
      Markdown.configure({ transformPastedText: true }),
      ...(onStarClick ? [starAnchors(onStarClick)] : [])
    ];
    editor = new Editor({
      element: editorEl,
      extensions,
      content: value,
      editable: !readonly,
      autofocus: autofocus ? 'end' : false,
      editorProps: {
        attributes: {
          class: `note-prose h-full overflow-y-auto focus:outline-none${
            readonly ? ' note-prose-readonly' : ''
          }`,
          style: surfacePadding
        }
      },
      onUpdate: ({ editor }) => {
        if (!applyingExternalValue) {
          const next = editor.storage.markdown.getMarkdown();
          value = next;
          onChange?.(next);
        }
      }
    });
  });

  $effect(() => {
    if (!editor) return;
    const current = editor.storage.markdown.getMarkdown();
    if (current === value) return;
    applyingExternalValue = true;
    editor.commands.setContent(value, false);
    applyingExternalValue = false;
  });

  /** The star already on the caret's line, if any (Ctrl+S toggles it). */
  export function starAtCursor(): number | null {
    return editor ? starAtCursorIn(editor) : null;
  }

  /** Anchor a gutter star on the caret's line (the last line when the
   * editor isn't focused — the user may be mid-call elsewhere). */
  export function addStar(id: number) {
    if (editor) anchorStarAtCursor(editor, id);
  }

  /** Drop a gutter star's anchor (store/backend removal happens upstream). */
  export function removeStar(id: number) {
    if (editor) clearStarAnchor(editor, id);
  }

  /** The textblock index each star currently sits on — persisted at stop
   * so the saved notes can re-anchor them. */
  export function getStarBlocks(): Map<number, number> {
    return editor ? starBlockIndexes(editor) : new Map();
  }

  /** Re-anchor a star at a saved textblock index (the saved-notes view). */
  export function addStarAtBlock(id: number, blockIndex: number) {
    if (editor) anchorStarAtBlock(editor, id, blockIndex);
  }

  /** Scroll a star's line into view (player star clicks). */
  export function scrollToStar(id: number) {
    if (!editor) return;
    const el = editor.view.dom.querySelector(
      `[data-star-id="${id}"]`
    ) as HTMLElement | null;
    el?.scrollIntoView({ block: 'center', behavior: 'smooth' });
  }

  onDestroy(() => editor?.destroy());
</script>

<div class="relative h-full min-h-0 flex-1 overflow-hidden">
  {#if placeholder && isEmpty}
    <div
      class="pointer-events-none absolute inset-x-0 top-0 select-none"
      style={surfacePadding}
      aria-hidden="true"
    >
      <span class="note-prose-placeholder">{placeholder}</span>
    </div>
  {/if}
  <div bind:this={editorEl} class="h-full"></div>
</div>

<style>
  /* The note type system — matches the transcript's reading style (15px/1.65)
     with the display face carrying headings, and everything else kept quiet. */
  :global(.note-prose) {
    font-size: 15px;
    line-height: 1.65;
    color: var(--foreground);
    caret-color: var(--foreground);
    position: relative; /* positions the gutter stars */
  }
  /* Never a focus outline: the pane is the editor, not a field. */
  :global(.note-prose:focus),
  :global(.note-prose:focus-visible) {
    outline: none;
  }

  /* A gutter star: a pseudo-element on the starred textblock (each bullet's
     own paragraph included), absolutely positioned into the surface's left
     padding at its line's top. Pseudo-elements are outside the DOM
     selection, so Ctrl+A copies only the notes text; clicks land via the
     extension's gutter hit-test. */
  :global(.note-prose [data-star-id]::before) {
    content: '★';
    position: absolute;
    left: 0.45rem;
    font-size: 15px;
    line-height: 24.75px; /* the body's 15px × 1.65 line box */
    color: var(--muted-foreground);
    cursor: pointer;
  }
  /* Readable line length on wide panes; the element itself stays
     full-width so the whole pane is clickable. */
  :global(.note-prose > *) {
    max-width: 72ch;
  }
  .note-prose-placeholder {
    font-size: 15px;
    line-height: 1.65;
    color: color-mix(in oklch, var(--muted-foreground) 70%, transparent);
  }

  /* Stack rhythm: spacing between top-level blocks only, so the first line
     always starts exactly at the surface padding (placeholder alignment
     depends on this). */
  :global(.note-prose > * + *) {
    margin-top: 0.55em;
  }
  :global(.note-prose p) {
    margin: 0;
  }

  :global(.note-prose h1),
  :global(.note-prose h2),
  :global(.note-prose h3) {
    font-family: 'Libre Baskerville', Georgia, 'Times New Roman', serif;
    font-weight: 400;
    line-height: 1.35;
  }
  :global(.note-prose h1) {
    font-size: 1.35rem;
  }
  :global(.note-prose h2) {
    font-size: 1.15rem;
  }
  :global(.note-prose h3) {
    font-size: 1rem;
    font-weight: 700;
  }
  :global(.note-prose > * + h1),
  :global(.note-prose > * + h2),
  :global(.note-prose > * + h3) {
    margin-top: 1.2em;
  }

  :global(.note-prose ul),
  :global(.note-prose ol) {
    padding-left: 1.4rem;
  }
  :global(.note-prose ul) {
    list-style: disc;
  }
  :global(.note-prose ol) {
    list-style: decimal;
  }
  :global(.note-prose li) {
    margin-top: 0.15em;
  }
  :global(.note-prose li p) {
    margin: 0;
  }
  :global(.note-prose li > ul),
  :global(.note-prose li > ol) {
    margin-top: 0.15em;
  }
  :global(.note-prose ::marker) {
    color: var(--muted-foreground);
  }

  :global(.note-prose blockquote) {
    border-left: 2px solid var(--border);
    padding-left: 0.85em;
    color: var(--muted-foreground);
  }
  :global(.note-prose code) {
    font-family:
      ui-monospace, SFMono-Regular, Menlo, Consolas, 'Liberation Mono', monospace;
    font-size: 0.85em;
    background: var(--muted);
    border-radius: 0.25rem;
    padding: 0.1em 0.35em;
  }
  :global(.note-prose pre) {
    background: var(--muted);
    border-radius: 0.375rem;
    padding: 0.6em 0.8em;
    overflow-x: auto;
  }
  :global(.note-prose pre code) {
    background: transparent;
    padding: 0;
    font-size: 0.85em;
  }
  :global(.note-prose hr) {
    border: 0;
    border-top: 1px solid var(--border);
    margin: 1.1em 0;
  }
  :global(.note-prose a) {
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  :global(.note-prose strong) {
    font-weight: 600;
  }
</style>
