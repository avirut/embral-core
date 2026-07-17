<script lang="ts">
  import { onDestroy } from 'svelte';
  import { ChevronLeft, Trash2 } from 'lucide-svelte';
  import { meetingsStore, PENDING_MEETING_ID } from '$lib/stores/meetings.svelte';
  import { appState } from '$lib/stores/app-state.svelte';
  import { configStore } from '$lib/stores/config.svelte';
  import type { MeetingDetail } from '$lib/types';
  import AudioPlayer from './AudioPlayer.svelte';
  import ConfirmDialog from './ConfirmDialog.svelte';
  import Tip from './Tip.svelte';
  import MarkdownEditor from './MarkdownEditor.svelte';
  import PendingMeetingDetail from './PendingMeetingDetail.svelte';
  import TranscriptSegments from './TranscriptSegments.svelte';
  import UserNotesView from './UserNotesView.svelte';
  import type { MeetingStar } from '$lib/types';
  import { formatDuration, formatMeetingDate } from '$lib/utils/meetingFormat';

  type DetailTab = 'summary' | 'notes' | 'transcript';
  type SaveState = 'idle' | 'saving' | 'saved' | 'error';

  let {
    showBack = false,
    onBack
  }: { showBack?: boolean; onBack?: () => void } = $props();

  let activeTab = $state<DetailTab>('summary');
  let titleDraft = $state('');
  let notesDraft = $state('');
  let transcriptDraft = $state('');
  let notesFrontmatter = $state('');
  let transcriptFrontmatter = $state('');
  let loadedDraftId = $state<string | null>(null);
  let saveState = $state<SaveState>('idle');
  let saveError = $state<string | null>(null);
  let confirmDelete = $state(false);
  let isDeleting = $state(false);
  let player = $state<AudioPlayer | null>(null);
  // Playback state mirrored out of the player — drives the transcript
  // tab's current-segment highlight and auto-follow.
  let playbackTime = $state(0);
  let playbackActive = $state(false);
  let userNotesRef = $state<UserNotesView | null>(null);
  let transcriptRef = $state<TranscriptSegments | null>(null);

  /** A star tick/chip on the player was clicked: the player already
   * seeked; scroll the active tab to that star's place. */
  function onStarActivate(star: MeetingStar, index: number) {
    if (activeTab === 'notes' && star.note_block !== null) {
      userNotesRef?.scrollToStar(index);
    } else if (activeTab === 'transcript') {
      transcriptRef?.followPlayhead();
    }
  }

  let titleTimer: ReturnType<typeof setTimeout> | null = null;
  let notesTimer: ReturnType<typeof setTimeout> | null = null;
  let transcriptTimer: ReturnType<typeof setTimeout> | null = null;
  let saveRevision = 0;

  const detail = $derived(meetingsStore.selectedDetail);
  const isLoading = $derived(
    meetingsStore.selectedId !== null &&
      meetingsStore.detailLoadingId === meetingsStore.selectedId
  );

  /** A meeting recorded with summaries off has no summary document — it is its
   * notes and its transcript, so it shows no Summary tab rather than an empty
   * one. Meetings summarized before the setting changed keep theirs.
   *
   * Keyed off the *saved* meeting, not the draft: from the draft, the tab would
   * vanish out from under a user who selected all and hit delete. */
  const hasSummary = $derived((detail?.notes_markdown ?? '').trim().length > 0);
  const tabs = $derived(
    (hasSummary
      ? [
          ['summary', 'Summary'],
          ['notes', 'Notes'],
          ['transcript', 'Transcript']
        ]
      : [
          ['notes', 'Notes'],
          ['transcript', 'Transcript']
        ]) as [DetailTab, string][]
  );
  const statusText = $derived.by(() => {
    if (saveState === 'saving') return 'Saving...';
    if (saveState === 'saved') return 'Saved';
    if (saveState === 'error') return saveError ?? "Couldn't save";
    return '';
  });
  // Read-only: speakers are edited through the transcript pills, and this
  // line just reflects them (frontmatter attendees for legacy meetings).
  const attendeeLine = $derived.by(() => {
    if (detail && detail.segments.length > 0) {
      const seen: string[] = [];
      for (const s of detail.segments) {
        if (s.speaker && !seen.includes(s.speaker)) seen.push(s.speaker);
      }
      if (seen.length > 0) return seen.join(', ');
    }
    return detail?.attendees.join(', ') ?? '';
  });

  function stripFirstHeading(markdown: string): string {
    const lines = markdown.split(/\r?\n/);
    const headingIndex = lines.findIndex((line) => line.startsWith('# '));
    if (headingIndex < 0) return markdown;

    lines.splice(headingIndex, 1);
    while (headingIndex < lines.length && lines[headingIndex]?.trim() === '') {
      lines.splice(headingIndex, 1);
    }
    return lines.join('\n').trimStart();
  }

  function splitEditableMarkdown(markdown: string): { frontmatter: string; body: string } {
    if (!markdown.startsWith('---')) {
      return { frontmatter: '', body: stripFirstHeading(markdown) };
    }
    const end = markdown.indexOf('\n---', 3);
    if (end === -1) {
      return { frontmatter: '', body: stripFirstHeading(markdown) };
    }
    const closingEnd = markdown.indexOf('\n', end + 4);
    if (closingEnd === -1) {
      return { frontmatter: markdown, body: '' };
    }
    return {
      frontmatter: markdown.slice(0, closingEnd).trimEnd(),
      body: stripFirstHeading(markdown.slice(closingEnd + 1).trimStart())
    };
  }

  function joinEditableMarkdown(frontmatter: string, heading: string, body: string): string {
    const trimmedFrontmatter = frontmatter.trim();
    const trimmedBody = body.trimStart();
    const markdown = `# ${heading.trim()}\n\n${trimmedBody}`.trimEnd();
    if (!trimmedFrontmatter) return markdown;
    return `${trimmedFrontmatter}\n\n${markdown}`;
  }

  $effect(() => {
    const selectedId = meetingsStore.selectedId;
    const selectedDetail = detail;
    if (!selectedId) {
      clearSaveTimers();
      loadedDraftId = null;
      titleDraft = '';
      notesDraft = '';
      transcriptDraft = '';
      notesFrontmatter = '';
      transcriptFrontmatter = '';
      return;
    }
    if (selectedDetail && loadedDraftId !== selectedId) {
      clearSaveTimers();
      const notesParts = splitEditableMarkdown(selectedDetail.notes_markdown);
      const transcriptParts = splitEditableMarkdown(selectedDetail.transcript_markdown);
      loadedDraftId = selectedId;
      titleDraft = selectedDetail.record.title;
      notesDraft = notesParts.body;
      transcriptDraft = transcriptParts.body;
      notesFrontmatter = notesParts.frontmatter;
      transcriptFrontmatter = transcriptParts.frontmatter;
      saveState = 'idle';
      saveError = null;
      confirmDelete = false;
      // The config names one of the detail's three tabs directly; a meeting
      // with no summary has no Summary tab to open on, so that one degrades
      // to Notes.
      const preferred: DetailTab = configStore.config?.open_meeting_tab ?? 'summary';
      activeTab =
        preferred === 'summary' && !notesParts.body.trim() ? 'notes' : preferred;
    }
  });

  /** Space toggles playback, ←/→ skip ±10s — unless focus is in an editor
   * or other input, which owns its keys. */
  function onDetailKeydown(e: KeyboardEvent) {
    if (!detail?.audio_exists || !player) return;
    const target = e.target as HTMLElement | null;
    if (
      target &&
      (target.tagName === 'INPUT' ||
        target.tagName === 'TEXTAREA' ||
        target.isContentEditable)
    ) {
      return;
    }
    if (e.key === ' ') {
      e.preventDefault();
      void player.toggle();
    } else if (e.key === 'ArrowLeft') {
      e.preventDefault();
      player.skip(-10);
    } else if (e.key === 'ArrowRight') {
      e.preventDefault();
      player.skip(10);
    }
  }

  function clearTimer(timer: ReturnType<typeof setTimeout> | null) {
    if (timer) clearTimeout(timer);
  }

  function clearSaveTimers() {
    clearTimer(titleTimer);
    clearTimer(notesTimer);
    clearTimer(transcriptTimer);
    titleTimer = null;
    notesTimer = null;
    transcriptTimer = null;
    saveRevision++;
  }

  function markSaving() {
    saveState = 'saving';
    saveError = null;
  }

  function markSaved(revision: number) {
    if (revision === saveRevision) {
      saveState = 'saved';
      saveError = null;
    }
  }

  function markError(error: unknown, revision: number) {
    if (revision === saveRevision) {
      saveState = 'error';
      saveError = error instanceof Error ? error.message : String(error);
    }
  }

  function scheduleTitleSave() {
    clearTimer(titleTimer);
    const id = meetingsStore.selectedId;
    const title = titleDraft.trim();
    if (!id) return;
    if (!title) {
      saveState = 'error';
      saveError = 'Title required';
      return;
    }
    titleTimer = setTimeout(async () => {
      const revision = ++saveRevision;
      markSaving();
      try {
        await meetingsStore.updateTitle(id, title);
        markSaved(revision);
      } catch (e) {
        markError(e, revision);
      }
    }, 500);
  }

  function scheduleNotesSave(value: string) {
    notesDraft = value;
    clearTimer(notesTimer);
    const id = meetingsStore.selectedId;
    if (!id) return;
    notesTimer = setTimeout(async () => {
      const revision = ++saveRevision;
      markSaving();
      try {
        await meetingsStore.updateNotes(
          id,
          joinEditableMarkdown(notesFrontmatter, titleDraft, value)
        );
        markSaved(revision);
      } catch (e) {
        markError(e, revision);
      }
    }, 900);
  }

  function scheduleTranscriptSave(value: string) {
    transcriptDraft = value;
    clearTimer(transcriptTimer);
    const id = meetingsStore.selectedId;
    if (!id) return;
    transcriptTimer = setTimeout(async () => {
      const revision = ++saveRevision;
      markSaving();
      try {
        await meetingsStore.updateTranscript(
          id,
          joinEditableMarkdown(transcriptFrontmatter, `${titleDraft} Transcript`, value)
        );
        markSaved(revision);
      } catch (e) {
        markError(e, revision);
      }
    }, 900);
  }

  /// Segment edits regenerate the transcript document (and can rename
  /// attendees) backend-side — pull those fields back into the drafts.
  function syncFromDetail(updated: MeetingDetail) {
    const transcriptParts = splitEditableMarkdown(updated.transcript_markdown);
    transcriptDraft = transcriptParts.body;
    transcriptFrontmatter = transcriptParts.frontmatter;
  }

  async function deleteSelectedMeeting() {
    const id = meetingsStore.selectedId;
    if (!id) return;
    isDeleting = true;
    try {
      await meetingsStore.deleteMeeting(id);
      confirmDelete = false;
      onBack?.();
    } finally {
      isDeleting = false;
    }
  }

  onDestroy(() => {
    clearSaveTimers();
  });
</script>

<div class="flex flex-col min-w-0 min-h-0 bg-background flex-1">
  {#if meetingsStore.selectedId === PENDING_MEETING_ID && appState.pendingMeeting}
    <PendingMeetingDetail pending={appState.pendingMeeting} />
  {:else if !meetingsStore.selectedRecord}
    <div class="flex flex-1 items-center justify-center p-6 text-center">
      <p class="text-sm text-muted-foreground">Select a meeting to view its notes.</p>
    </div>
  {:else}
    <div class="px-3 py-3 border-b border-border shrink-0">
      {#if showBack}
        <button
          onclick={onBack}
          aria-label="Back to meetings"
          class="mb-2 inline-flex items-center gap-1.5 text-xs font-medium text-muted-foreground hover:text-foreground transition-colors min-[960px]:hidden"
        >
          <ChevronLeft size={14} />
          Meetings
        </button>
      {/if}

      <div class="flex items-start gap-2">
        <!-- Borderless like the writing surface: editing the title should
             feel like editing text, not a form field. -->
        <input
          bind:value={titleDraft}
          oninput={scheduleTitleSave}
          class="font-display min-w-0 flex-1 bg-transparent px-0 py-0.5 text-lg leading-snug outline-none
            placeholder:text-muted-foreground/70"
          aria-label="Meeting title"
        />
        <Tip text="Delete meeting">
          {#snippet children({ props })}
            <button
              {...props}
              onclick={() => (confirmDelete = true)}
              class="p-1.5 rounded-md text-muted-foreground hover:text-destructive hover:bg-destructive/10 transition-colors shrink-0"
              aria-label="Delete meeting"
            >
              <Trash2 size={15} />
            </button>
          {/snippet}
        </Tip>
      </div>

      <!-- One muted metadata line: date · duration · speakers (read-only —
           the transcript pills are where speakers get edited), with the
           save status trailing quietly. -->
      <div class="mt-1 flex items-baseline gap-x-2 text-xs text-muted-foreground">
        <span class="shrink-0">{formatMeetingDate(meetingsStore.selectedRecord.date)}</span>
        <span class="shrink-0 tabular-nums"
          >{formatDuration(meetingsStore.selectedRecord.duration_seconds)}</span
        >
        <span class="min-w-0 flex-1 truncate py-0.5">{attendeeLine}</span>
        {#if statusText}
          <span
            class="shrink-0 {saveState === 'error' ? 'text-destructive' : 'text-muted-foreground/70'}"
          >
            {statusText}
          </span>
        {/if}
      </div>
    </div>

    <!-- Quiet underline tabs — text, a hairline, and an accent edge on the
         active one; no boxed segmented control. -->
    <div class="flex shrink-0 items-center gap-5 border-b border-border px-4">
      {#each tabs as [key, label] (key)}
        <button
          onclick={() => (activeTab = key as DetailTab)}
          class="-mb-px border-b-2 px-0.5 py-2 text-sm font-medium transition-colors
            {activeTab === key
            ? 'border-foreground text-foreground'
            : 'border-transparent text-muted-foreground hover:text-foreground'}"
        >
          {label}
        </button>
      {/each}
    </div>

    <div class="flex min-h-0 flex-1 flex-col">
      {#if isLoading}
        <p class="p-3 text-sm text-muted-foreground">Loading meeting...</p>
      {:else if meetingsStore.error}
        <p class="p-3 text-sm text-destructive">{meetingsStore.error}</p>
      {:else if detail}
        {#if activeTab === 'summary'}
          {#key `${detail.record.id}:summary`}
            <MarkdownEditor
              bind:value={notesDraft}
              placeholder="No summary saved yet."
              onChange={scheduleNotesSave}
            />
          {/key}
        {:else if activeTab === 'notes'}
          {#key `${detail.record.id}:usernotes`}
            <UserNotesView
              bind:this={userNotesRef}
              notes={detail.user_notes}
              stars={detail.stars}
              onStarClick={detail.audio_exists
                ? (star) => player?.seekTo(star.seconds)
                : undefined}
            />
          {/key}
        {:else if detail.segments.length > 0}
          <div class="min-h-0 flex-1 px-3 py-3">
            {#key `${detail.record.id}:segments`}
              <TranscriptSegments
                bind:this={transcriptRef}
                {detail}
                onDetailChange={syncFromDetail}
                currentTime={playbackTime}
                playing={playbackActive}
                onSeek={detail.audio_exists ? (secs) => player?.seekTo(secs) : undefined}
              />
            {/key}
          </div>
        {:else}
          {#key `${detail.record.id}:transcript`}
            <MarkdownEditor
              bind:value={transcriptDraft}
              placeholder="No transcript saved yet."
              onChange={scheduleTranscriptSave}
            />
          {/key}
        {/if}
      {/if}
    </div>

    {#if detail || !isLoading}
      <AudioPlayer
        bind:this={player}
        bind:currentTime={playbackTime}
        bind:playing={playbackActive}
        audioPath={detail?.audio_path ?? null}
        stars={detail?.stars ?? []}
        onStarActivate={onStarActivate}
      />
    {/if}
  {/if}
</div>

<svelte:window onkeydown={onDetailKeydown} />

<ConfirmDialog
  bind:open={confirmDelete}
  title="Delete meeting?"
  body="Deleting this meeting will permanently delete its notes, transcript, and audio."
  busy={isDeleting}
  onConfirm={deleteSelectedMeeting}
/>
