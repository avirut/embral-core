<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { Pause, Play, Square, Star } from 'lucide-svelte';
  import { appState } from '$lib/stores/app-state.svelte';
  import { formatTime } from '$lib/utils/meetingFormat';
  import LevelRibbon from './LevelRibbon.svelte';
  import Tip from './Tip.svelte';

  let {
    userNotes = $bindable(''),
    meetingTitle = $bindable(''),
    onStar
  }: { userNotes: string; meetingTitle: string; onStar?: () => void } = $props();

  // Elapsed time derives from the store's recording clock (backend start
  // instant, paused spans excluded); the interval only refreshes `now`, so
  // the timer survives leaving and re-entering this view.
  let now = $state(Date.now());
  let elapsed = $derived(appState.elapsedSeconds(now));

  $effect(() => {
    if (appState.isRecording && !appState.isPaused) {
      now = Date.now();
      const interval = setInterval(() => {
        now = Date.now();
      }, 1000);
      return () => clearInterval(interval);
    }
  });


  async function togglePause() {
    if (appState.isPaused) {
      await invoke('resume_recording');
      appState.setPaused(false);
    } else {
      await invoke('pause_recording');
      appState.setPaused(true);
    }
  }

  async function stop() {
    // The pending meeting on the Meetings page carries this title until the
    // persisted record replaces it. The notes/title drafts are NOT cleared
    // here — the pending view still reads them; a new recording clears them.
    appState.setPendingTitleHint(meetingTitle);
    await invoke('stop_recording', {
      userNotes: userNotes || null,
      meetingTitle: meetingTitle.trim() || null
    });
  }
</script>

<div class="flex h-12 shrink-0 items-center gap-3 border-b border-border px-4">
  <!-- The sidebar's record button already carries the recording status;
       the header leads with the star action and the timer. -->
  <div class="flex shrink-0 items-center gap-1.5">
    <Tip text="Star this moment (Ctrl+S)">
      {#snippet children({ props })}
        <button
          {...props}
          onclick={() => onStar?.()}
          class="rounded-md p-2 text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          aria-label="Star this moment"
        >
          <Star size={16} />
        </button>
      {/snippet}
    </Tip>
    <span class="font-mono text-sm tabular-nums">{formatTime(elapsed)}</span>
    {#if appState.isPaused}
      <span class="text-[11px] font-medium tracking-wide text-muted-foreground uppercase">
        Paused
      </span>
    {/if}
  </div>

  <!-- Borderless: editing the title feels like editing text, not a field. -->
  <input
    bind:value={meetingTitle}
    class="font-display h-8 min-w-0 flex-1 bg-transparent px-2 text-base outline-none
      placeholder:text-muted-foreground/70"
    placeholder="Meeting title"
    aria-label="Meeting title"
  />

  <LevelRibbon />

  <div class="flex shrink-0 items-center gap-1.5">
    <Tip text={appState.isPaused ? 'Resume' : 'Pause'}>
      {#snippet children({ props })}
        <button
          {...props}
          onclick={togglePause}
          class="rounded-md p-2 text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
          aria-label={appState.isPaused ? 'Resume recording' : 'Pause recording'}
        >
          {#if appState.isPaused}
            <Play size={16} />
          {:else}
            <Pause size={16} />
          {/if}
        </button>
      {/snippet}
    </Tip>
    <Tip text="Stop recording">
      {#snippet children({ props })}
        <button
          {...props}
          onclick={stop}
          class="rounded-md p-2 text-destructive transition-colors hover:bg-destructive/10"
          aria-label="Stop recording"
        >
          <Square size={16} fill="currentColor" />
        </button>
      {/snippet}
    </Tip>
  </div>
</div>
