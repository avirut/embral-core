<script lang="ts">
  import { onMount } from 'svelte';
  import { ArrowDown, Play, Scissors, Star, Trash2, UserPen, X } from 'lucide-svelte';
  import type { MeetingDetail, TranscriptionSegment } from '$lib/types';
  import { meetingsStore } from '$lib/stores/meetings.svelte';
  import { speakersStore } from '$lib/stores/speakers.svelte';
  import { chipClass } from '$lib/utils/speakerColors';
  import { formatTime } from '$lib/utils/meetingFormat';
  import { cn } from '$lib/utils';
  import SpeakerNameInput from './SpeakerNameInput.svelte';
  import Tip from './Tip.svelte';

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
  // Label being renamed via its header chip.
  let editingLabel = $state<string | null>(null);
  let labelDraft = $state('');
  // Row armed for click-to-split.
  let splittingRow = $state<number | null>(null);

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

  // --- Playback sync: highlight the segment under the playhead and follow
  // it while playing; scrolling by hand unfollows until the pill re-pins.
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
    scrollActiveIntoView('smooth');
  }

  /** Re-pin to the playhead (player star clicks) so the seek's landing
   * segment scrolls into view. */
  export function followPlayhead() {
    following = true;
  }

  function scrollActiveIntoView(behavior: ScrollBehavior) {
    const el = rowEls[activeIndex];
    if (!el) return;
    autoScrolling = true;
    el.scrollIntoView({ block: 'nearest', behavior });
    setTimeout(() => (autoScrolling = false), 350);
  }

  $effect(() => {
    activeIndex;
    if (playing && following && activeIndex >= 0) {
      scrollActiveIntoView('smooth');
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
      error = e instanceof Error ? e.message : String(e);
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
  <Tip text="Play from here">
    {#snippet children({ props })}
      <button
        {...props}
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
  </Tip>
{/snippet}

<div class="flex h-full min-h-0 flex-col">
  {#if detail.name_suggestions.length > 0}
    <div class="mb-2 shrink-0 space-y-1.5">
      {#each detail.name_suggestions as sug (sug.label)}
        <div
          class="flex items-center justify-between gap-3 rounded-lg border border-primary/30 bg-primary/5 px-3 py-2"
        >
          <p class="min-w-0 truncate text-xs">
            <span class="font-medium">{sug.label}</span>
            <span class="text-muted-foreground"> looks like </span>
            <span class="font-medium">{sug.name}</span>
            <span class="text-muted-foreground"> (from your notes)</span>
          </p>
          <div class="flex shrink-0 items-center gap-1">
            <button
              class="inline-flex items-center gap-1 rounded-md bg-primary px-2 py-1 text-[11px] font-medium text-primary-foreground transition-colors hover:bg-primary/90"
              disabled={busy}
              onclick={() => confirmName(sug.label, sug.name)}
            >
              Apply
            </button>
            <button
              class="rounded-md px-2 py-1 text-[11px] text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
              disabled={busy}
              onclick={() => dismissName(sug.label)}
            >
              Dismiss
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
            onCommit={commitLabelEdit}
            onCancel={() => (editingLabel = null)}
          />
        {:else}
          <Tip
            text="Rename this speaker (type another speaker's name to merge; right-click to remove from the transcript)"
          >
            {#snippet children({ props })}
              <button
                {...props}
                class={cn(
                  'rounded-full px-2.5 py-0.5 text-[11px] font-medium transition-opacity hover:opacity-75',
                  chipClass(label, labels)
                )}
                onclick={() => startLabelEdit(label)}
                oncontextmenu={(e) => {
                  e.preventDefault();
                  void clearLabel(label);
                }}
              >
                {label}
              </button>
            {/snippet}
          </Tip>
        {/if}
      {/each}
    </div>
  {/if}

  {#if error}
    <p class="mb-2 shrink-0 text-xs text-destructive">{error}</p>
  {/if}

  <div class="relative min-h-0 flex-1">
  <div onscroll={onListScroll} class="h-full space-y-1 overflow-y-auto pr-1">
    {#each segments as seg, i (i)}
      {#each starMarkers.get(i) ?? [] as star (star)}
        {@render starRow(star)}
      {/each}
      <div
        bind:this={rowEls[i]}
        class={cn(
          'group rounded-md border-l-2 border-transparent px-2 py-1.5 transition-colors duration-150 hover:bg-accent/40',
          activeIndex === i && 'border-l-foreground/60 bg-accent/50',
          splittingRow === i && 'bg-primary/5 ring-1 ring-primary/40'
        )}
      >
        <div class="flex items-center gap-2">
          {#if onSeek}
            <Tip text="Play from here">
              {#snippet children({ props })}
                <button
                  {...props}
                  class="relative w-9 shrink-0 text-left font-mono text-[10px] tabular-nums text-muted-foreground transition-colors hover:text-foreground"
                  aria-label="Play from {formatTime(seg.start)}"
                  onclick={() => onSeek?.(seg.start)}
                >
                  <span class="group-hover:opacity-0">{formatTime(seg.start)}</span>
                  <span
                    class="absolute inset-y-0 left-0 flex items-center opacity-0 group-hover:opacity-100"
                  >
                    <Play size={11} fill="currentColor" />
                  </span>
                </button>
              {/snippet}
            </Tip>
          {:else}
            <span class="w-9 shrink-0 font-mono text-[10px] tabular-nums text-muted-foreground">
              {formatTime(seg.start)}
            </span>
          {/if}
          {#if editingRow === i}
            <SpeakerNameInput
              bind:value={rowSpeakerDraft}
              suggestions={nameSuggestions(seg.speaker)}
              onCommit={() => commitRowEdit(i)}
              onCancel={() => (editingRow = null)}
            />
          {:else if seg.speaker}
            <!-- The narrowing from `{:else if seg.speaker}` does not reach inside
                 the snippet (it is its own closure), so the label is captured
                 here rather than re-derived in there. -->
            {@const speaker = seg.speaker}
            <Tip text="Change this segment's speaker">
              {#snippet children({ props })}
                <button
                  {...props}
                  class={cn(
                    'shrink-0 rounded-full px-2 py-0.5 text-[10px] font-medium transition-opacity hover:opacity-75',
                    chipClass(speaker, labels)
                  )}
                  disabled={busy}
                  onclick={() => startRowEdit(i)}
                >
                  {speaker}
                </button>
              {/snippet}
            </Tip>
          {/if}
          <span class="min-w-0 flex-1"></span>
          <div
            class="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100"
          >
            {#if !seg.speaker}
              <!-- Labeled rows edit via their pill; this is the affordance
                   for rows that have no pill to click. -->
              <Tip text="Assign a speaker">
                {#snippet children({ props })}
                  <button
                    {...props}
                    class="rounded p-1 text-muted-foreground hover:bg-accent hover:text-foreground"
                    aria-label="Assign a speaker"
                    disabled={busy}
                    onclick={() => startRowEdit(i)}
                  >
                    <UserPen size={12} />
                  </button>
                {/snippet}
              </Tip>
            {/if}
            <Tip text="Split segment">
              {#snippet children({ props })}
                <button
                  {...props}
                  class={cn(
                    'rounded p-1 hover:bg-accent',
                    splittingRow === i
                      ? 'text-primary'
                      : 'text-muted-foreground hover:text-foreground'
                  )}
                  aria-label="Split segment"
                  disabled={busy}
                  onclick={() => armSplit(i)}
                >
                  {#if splittingRow === i}<X size={12} />{:else}<Scissors size={12} />{/if}
                </button>
              {/snippet}
            </Tip>
            <Tip text="Delete segment">
              {#snippet children({ props })}
                <button
                  {...props}
                  class="rounded p-1 text-muted-foreground hover:bg-destructive/10 hover:text-destructive"
                  aria-label="Delete segment"
                  disabled={busy}
                  onclick={() => deleteRow(i)}
                >
                  <Trash2 size={12} />
                </button>
              {/snippet}
            </Tip>
          </div>
        </div>
        {#if splittingRow === i}
          <p class="mt-0.5 pl-11 text-[10px] text-primary">
            Click inside the text where the new segment should start.
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
    {#each starMarkers.get(segments.length) ?? [] as star (star)}
      {@render starRow(star)}
    {/each}
  </div>

  {#if playing && !following && activeIndex >= 0}
    <button
      onclick={jumpToCurrent}
      class="absolute bottom-3 left-1/2 inline-flex -translate-x-1/2 items-center gap-1 rounded-full border border-border bg-background/95 px-3 py-1 text-[11px] font-medium text-muted-foreground shadow-sm transition-colors hover:text-foreground"
    >
      Jump to current
      <ArrowDown size={11} />
    </button>
  {/if}
  </div>
</div>
