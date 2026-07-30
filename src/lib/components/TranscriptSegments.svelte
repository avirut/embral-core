<script lang="ts">
	import { errorMessage } from '$lib/copy/errors';
  import { onMount, tick } from 'svelte';
  import { Play, Scissors, Star, Trash2, UserPen, X } from 'lucide-svelte';
  import type { MeetingDetail, TranscriptionSegment } from '$lib/types';
  import { meetingsStore } from '$lib/stores/meetings.svelte';
  import { speakersStore } from '$lib/stores/speakers.svelte';
  import { nameClass } from '$lib/utils/speakerColors';
  import { formatTime } from '$lib/utils/meetingFormat';
  import { cn } from '$lib/utils';
  import SpeakerNameInput from './SpeakerNameInput.svelte';
  import { tip } from '$lib/tip.svelte';
  import CopyParts from './CopyParts.svelte';
  import { copy } from '$lib/copy';

  const t = $derived(copy.meetings.transcript);

  let {
    detail,
    onDetailChange,
    currentTime = 0,
    playing = false,
    onSeek
  }: {
    detail: MeetingDetail;
    onDetailChange?: (updated: MeetingDetail) => void;
    /** Playback position (seconds) — highlights the current segment. */
    currentTime?: number;
    playing?: boolean;
    /** Seek-and-play from a segment's timestamp; absent = no audio. */
    onSeek?: (seconds: number) => void;
  } = $props();

  let busy = $state(false);
  let error = $state<string | null>(null);
  // Row index whose speaker is being edited, and its draft value.
  let editingRow = $state<number | null>(null);
  let rowSpeakerDraft = $state('');
  // Label being renamed via its header name.
  let editingLabel = $state<string | null>(null);
  let labelDraft = $state('');
  // Row armed for click-to-split.
  let splittingRow = $state<number | null>(null);
  // The row under the pointer (or holding focus). Rows mount their action
  // cluster and play affordance only while hovered — a thousand rows of
  // always-mounted icons and tooltips is what made this tab slow.
  let hoverRow = $state<number | null>(null);

  onMount(() => {
    if (!speakersStore.loaded) void speakersStore.refresh();
  });

  const meetingId = $derived(detail.record.id);
  const segments = $derived(detail.segments);
  // Starred moments, mapped to the first segment starting at or after each
  // star (index == segments.length collects trailing stars).
  const starMarkers = $derived.by(() => {
    const map = new Map<number, number[]>();
    const stars = (detail.stars ?? []).map((s) => s.seconds).sort((a, b) => a - b);
    let si = 0;
    for (let i = 0; i < segments.length; i++) {
      const before: number[] = [];
      while (si < stars.length && stars[si] <= segments[i].start) {
        before.push(stars[si++]);
      }
      if (before.length) map.set(i, before);
    }
    if (si < stars.length) map.set(segments.length, stars.slice(si));
    return map;
  });
  const labels = $derived.by(() => {
    const seen: string[] = [];
    for (const s of segments) {
      if (s.speaker && !seen.includes(s.speaker)) seen.push(s.speaker);
    }
    return seen;
  });

  // --- Progressive render: an instant first screen, the rest mounted in
  // idle time. Long transcripts made the tab switch stall for seconds when
  // every row mounted synchronously.
  const INITIAL_ROWS = 200;
  const RENDER_BATCH = 400;
  let renderCount = $state(INITIAL_ROWS);
  const fullyRendered = $derived(renderCount >= segments.length);

  $effect(() => {
    if (fullyRendered) return;
    const grow = () =>
      (renderCount = Math.min(segments.length, renderCount + RENDER_BATCH));
    if (typeof requestIdleCallback === 'function') {
      const id = requestIdleCallback(grow);
      return () => cancelIdleCallback(id);
    }
    const timeout = setTimeout(grow, 0);
    return () => clearTimeout(timeout);
  });

  /** Mount up to a target row now — scroll targets can sit past the
   * rendered window while the idle growth is still catching up. */
  async function ensureRendered(index: number) {
    if (index < renderCount) return;
    renderCount = Math.min(segments.length, index + 50);
    await tick();
  }

  // --- Playback sync: highlight the segment under the playhead and follow
  // it while playing; scrolling by hand unfollows until the "Jump to
  // current" pill re-pins.
  const activeIndex = $derived.by(() => {
    if (!playing && currentTime <= 0) return -1;
    for (let i = segments.length - 1; i >= 0; i--) {
      if (currentTime >= segments[i].start) {
        return currentTime < segments[i].end + 0.75 ? i : -1;
      }
    }
    return -1;
  });

  let rowEls: (HTMLElement | null)[] = [];
  let following = $state(true);
  let autoScrolling = false;

  function onListScroll() {
    if (!autoScrolling) following = false;
  }

  function jumpToCurrent() {
    following = true;
    void scrollActiveIntoView('smooth');
  }

  /** Re-pin to the playhead (player star clicks) so the seek's landing
   * segment scrolls into view. */
  export function followPlayhead() {
    following = true;
  }

  /** Bring one line to the middle — how a search result arrives.
   *
   * Takes the index rather than reading the playhead: the row list is
   * virtualized and the playhead's own segment is derived from a time the
   * media element is still settling on, so "scroll to whatever is active"
   * raced both. The caller knows exactly which line it matched.
   *
   * `center` where playback following uses `nearest`: following moves as
   * little as possible while audio runs, but arriving from somewhere else
   * wants the line in the middle, with its lead-up visible above it. */
  export async function revealIndex(index: number) {
    if (index < 0 || index >= segments.length) return;
    following = true;
    await ensureRendered(index);
    autoScrolling = true;
    // Centre by arithmetic rather than `scrollIntoView`: the list is
    // virtualized, so rows keep rendering after the scroll starts and the
    // target drifts under it — a smooth `scrollIntoView` reliably finished
    // a screen short. Set the position, then correct once more after the
    // layout has settled.
    centreRow(index);
    requestAnimationFrame(() => centreRow(index));
    setTimeout(() => {
      centreRow(index);
      autoScrolling = false;
    }, 200);
  }

  function centreRow(index: number) {
    const el = rowEls[index];
    const scroller = el?.closest<HTMLElement>('.overflow-y-auto');
    if (!el || !scroller) return;
    const row = el.getBoundingClientRect();
    const box = scroller.getBoundingClientRect();
    scroller.scrollTop += row.top - box.top - (box.height - row.height) / 2;
  }

  async function scrollActiveIntoView(
    behavior: ScrollBehavior,
    block: ScrollLogicalPosition = 'nearest'
  ) {
    if (activeIndex < 0) return;
    await ensureRendered(activeIndex);
    const el = rowEls[activeIndex];
    if (!el) return;
    autoScrolling = true;
    el.scrollIntoView({ block, behavior });
    setTimeout(() => (autoScrolling = false), 350);
  }

  $effect(() => {
    activeIndex;
    if (playing && following && activeIndex >= 0) {
      void scrollActiveIntoView('smooth');
    }
  });

  /// Suggestion pool for a name editor: registry profiles plus the
  /// meeting's other labels (typing another label merges into it).
  function nameSuggestions(current: string | null): string[] {
    return [
      ...speakersStore.speakers.map((p) => p.name),
      ...labels.filter((l) => l !== current)
    ];
  }

  /// The registry id for a person whose name matches, so manual renames to a
  /// known person link the segments too.
  function registryIdFor(name: string): string | null {
    const person = speakersStore.speakers.find(
      (p) => p.name.toLowerCase() === name.trim().toLowerCase()
    );
    return person?.id ?? null;
  }

  async function apply(fn: () => Promise<MeetingDetail | undefined>) {
    if (busy) return;
    busy = true;
    error = null;
    try {
      const updated = await fn();
      if (updated) onDetailChange?.(updated);
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }

  function startRowEdit(index: number) {
    editingRow = index;
    rowSpeakerDraft = segments[index]?.speaker ?? '';
    splittingRow = null;
  }

  async function commitRowEdit(index: number) {
    const speaker = rowSpeakerDraft.trim();
    editingRow = null;
    if (!speaker || speaker === segments[index]?.speaker) return;
    await apply(() =>
      meetingsStore.editSegments(meetingId, {
        kind: 'reassign',
        index,
        speaker,
        speaker_id: registryIdFor(speaker)
      })
    );
  }

  async function deleteRow(index: number) {
    await apply(() => meetingsStore.editSegments(meetingId, { kind: 'delete', index }));
  }

  function armSplit(index: number) {
    splittingRow = splittingRow === index ? null : index;
    editingRow = null;
  }

  async function splitAtSelection(index: number) {
    if (splittingRow !== index) return;
    const selection = window.getSelection();
    if (!selection || selection.rangeCount === 0) return;
    const offset = selection.getRangeAt(0).startOffset;
    splittingRow = null;
    if (offset <= 0 || offset >= (segments[index]?.text.length ?? 0)) return;
    await apply(() =>
      meetingsStore.editSegments(meetingId, {
        kind: 'split',
        index,
        char_offset: offset
      })
    );
  }

  function startLabelEdit(label: string) {
    editingLabel = label;
    labelDraft = label;
  }

  /** "Speaker N" — a machine label, never worth a profile. */
  function isGenericLabel(label: string): boolean {
    return /^Speaker \d+$/.test(label);
  }

  async function commitLabelEdit() {
    const from = editingLabel;
    const to = labelDraft.trim();
    editingLabel = null;
    if (!from || !to || to === from) return;
    // A real name that isn't in the registry yet becomes a profile on the
    // spot, so the rename also links the segments.
    let speakerId = registryIdFor(to);
    if (!speakerId && !isGenericLabel(to)) {
      const created = await speakersStore.save({
        name: to,
        notes: ''
      });
      speakerId = created?.id ?? null;
    }
    await apply(() =>
      meetingsStore.editSegments(meetingId, {
        kind: 'relabel_all',
        from,
        to,
        speaker_id: speakerId
      })
    );
  }

  /// Remove a label from the transcript: its segments become unattributed.
  async function clearLabel(label: string) {
    await apply(() => meetingsStore.editSegments(meetingId, { kind: 'clear_label', label }));
  }

  async function confirmName(label: string, name: string) {
    await apply(() => meetingsStore.confirmNameSuggestion(meetingId, label, name));
  }

  async function dismissName(label: string) {
    await apply(() => meetingsStore.dismissNameSuggestion(meetingId, label));
  }
</script>

{#snippet starRow(star: number)}
  <!-- A starred moment at its place in the transcript; clicking plays it. The
       tooltip rides the enabled state for free: a disabled button emits no
       pointer events, so it simply never opens without a player to seek. -->
  <button
    use:tip={t.playFromHere}
    class="flex w-full items-center gap-2 px-2 py-1 text-[11px] text-muted-foreground transition-colors {onSeek
      ? 'hover:text-foreground'
      : 'cursor-default'}"
    disabled={!onSeek}
    onclick={() => onSeek?.(star)}
  >
    <Star size={11} fill="currentColor" class="shrink-0" />
    <span class="tabular-nums">{formatTime(star)}</span>
    <span class="h-px min-w-0 flex-1 bg-border"></span>
  </button>
{/snippet}

<div class="flex h-full min-h-0 flex-col">
  {#if detail.name_suggestions.length > 0}
    <div class="mb-2 shrink-0 space-y-1.5">
      {#each detail.name_suggestions as sug (sug.label)}
        <div
          class="flex items-center justify-between gap-3 rounded-lg border border-primary/30 bg-primary/5 px-3 py-2"
        >
          <p class="min-w-0 truncate text-xs">
            <CopyParts parts={t.suggestion(sug.label, sug.name)}>
              {#snippet part(slot, text)}
                {#if slot === 'strong'}<span class="font-medium">{text}</span
                  >{:else if slot === 'muted'}<span class="text-muted-foreground"
                    >{text}</span
                  >{/if}
              {/snippet}
            </CopyParts>
          </p>
          <div class="flex shrink-0 items-center gap-1">
            <button
              class="inline-flex items-center gap-1 rounded-md bg-primary px-2 py-1 text-[11px] font-medium text-primary-foreground transition-colors hover:bg-primary/90"
              disabled={busy}
              onclick={() => confirmName(sug.label, sug.name)}
            >
              {t.suggestionApply}
            </button>
            <button
              class="rounded-md px-2 py-1 text-[11px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
              disabled={busy}
              onclick={() => dismissName(sug.label)}
            >
              {t.suggestionDismiss}
            </button>
          </div>
        </div>
      {/each}
    </div>
  {/if}

  {#if labels.length > 0}
    <div class="mb-2 flex shrink-0 flex-wrap items-center gap-1.5">
      {#each labels as label (label)}
        {#if editingLabel === label}
          <SpeakerNameInput
            bind:value={labelDraft}
            suggestions={nameSuggestions(label)}
            class={cn('text-[11px] font-medium', nameClass(label, labels))}
            onCommit={commitLabelEdit}
            onCancel={() => (editingLabel = null)}
          />
        {:else}
          <button
            use:tip={t.renameSpeaker}
            class={cn(
              'text-[11px] font-medium underline-offset-4 transition-opacity hover:underline hover:opacity-75',
              nameClass(label, labels)
            )}
            onclick={() => startLabelEdit(label)}
            oncontextmenu={(e) => {
              e.preventDefault();
              void clearLabel(label);
            }}
          >
            {label}
          </button>
        {/if}
      {/each}
    </div>
  {/if}

  {#if error}
    <p class="mb-2 shrink-0 text-xs text-destructive">{error}</p>
  {/if}

  <div class="relative min-h-0 flex-1">
  <div onscroll={onListScroll} class="h-full space-y-1 overflow-y-auto pr-1">
    {#each segments.slice(0, renderCount) as seg, i (i)}
      {#each starMarkers.get(i) ?? [] as star (star)}
        {@render starRow(star)}
      {/each}
      <!-- svelte-ignore a11y_no_static_element_interactions -- hover
           tracking only; the row's controls carry their own semantics. -->
      <div
        bind:this={rowEls[i]}
        onpointerenter={() => (hoverRow = i)}
        onpointerleave={() => (hoverRow = null)}
        onfocusin={() => (hoverRow = i)}
        class={cn(
          'rounded-md border-l-2 border-transparent px-2 py-1.5 transition-colors duration-150 hover:bg-accent/40',
          activeIndex === i && 'border-l-foreground/60 bg-accent/50',
          splittingRow === i && 'bg-primary/5 ring-1 ring-primary/40'
        )}
      >
        <div class="flex items-center gap-2">
          {#if onSeek}
            <button
              use:tip={t.playFromHere}
              class="relative w-9 shrink-0 text-left font-mono text-[10px] tabular-nums text-muted-foreground transition-colors hover:text-foreground"
              aria-label={t.playFrom(formatTime(seg.start))}
              onclick={() => onSeek?.(seg.start)}
            >
              <span class={hoverRow === i ? 'opacity-0' : ''}>{formatTime(seg.start)}</span>
              {#if hoverRow === i}
                <span class="absolute inset-y-0 left-0 flex items-center">
                  <Play size={11} fill="currentColor" />
                </span>
              {/if}
            </button>
          {:else}
            <span class="w-9 shrink-0 font-mono text-[10px] tabular-nums text-muted-foreground">
              {formatTime(seg.start)}
            </span>
          {/if}
          {#if editingRow === i}
            <SpeakerNameInput
              bind:value={rowSpeakerDraft}
              suggestions={nameSuggestions(seg.speaker)}
              class={cn('text-[10px] font-medium', nameClass(seg.speaker ?? '', labels))}
              onCommit={() => commitRowEdit(i)}
              onCancel={() => (editingRow = null)}
            />
          {:else if seg.speaker}
            <button
              use:tip={t.changeSpeaker}
              class={cn(
                'shrink-0 text-[10px] font-medium underline-offset-4 transition-opacity hover:underline hover:opacity-75',
                nameClass(seg.speaker, labels)
              )}
              disabled={busy}
              onclick={() => startRowEdit(i)}
            >
              {seg.speaker}
            </button>
          {/if}
          <span class="min-w-0 flex-1"></span>
          {#if hoverRow === i || splittingRow === i}
            <!-- Mounted, not revealed: these existed opacity-0 on every row
                 and their tooltip trees dominated the tab's render cost. -->
            <div class="flex h-5 shrink-0 items-center gap-0.5">
              {#if !seg.speaker}
                <!-- Labeled rows edit by clicking the name itself; this is
                     the affordance for rows that have no name to click. -->
                <button
                  use:tip={t.assignSpeaker}
                  class="rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
                  aria-label={t.assignSpeaker}
                  disabled={busy}
                  onclick={() => startRowEdit(i)}
                >
                  <UserPen size={12} />
                </button>
              {/if}
              <button
                use:tip={t.splitSegment}
                class={cn(
                  'rounded p-1 hover:bg-accent',
                  splittingRow === i
                    ? 'text-primary'
                    : 'text-muted-foreground hover:text-foreground'
                )}
                aria-label={t.splitSegment}
                disabled={busy}
                onclick={() => armSplit(i)}
              >
                {#if splittingRow === i}<X size={12} />{:else}<Scissors size={12} />{/if}
              </button>
              <button
                use:tip={t.deleteSegment}
                class="rounded p-1 text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
                aria-label={t.deleteSegment}
                disabled={busy}
                onclick={() => deleteRow(i)}
              >
                <Trash2 size={12} />
              </button>
            </div>
          {:else}
            <!-- Height placeholder so hovering a row never reflows it. -->
            <div class="h-5 shrink-0"></div>
          {/if}
        </div>
        {#if splittingRow === i}
          <p class="mt-0.5 pl-11 text-[10px] text-primary">
            {t.splitHint}
          </p>
        {/if}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
        <p
          class={cn(
            'mt-0.5 pl-11 text-[15px] leading-relaxed',
            splittingRow === i && 'cursor-text select-text'
          )}
          onclick={() => splitAtSelection(i)}
        >
          {seg.text}
        </p>
      </div>
    {/each}
    {#if fullyRendered}
      {#each starMarkers.get(segments.length) ?? [] as star (star)}
        {@render starRow(star)}
      {/each}
    {/if}
  </div>

  {#if playing && !following && activeIndex >= 0}
    <button
      onclick={jumpToCurrent}
      class="absolute bottom-3 left-1/2 inline-flex -translate-x-1/2 items-center rounded-full border border-border bg-background/95 px-3 py-1 text-[11px] font-medium text-muted-foreground shadow-sm transition-colors hover:text-foreground"
    >
      {t.jumpToCurrent}
    </button>
  {/if}
  </div>
</div>
