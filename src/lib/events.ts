import { invoke } from '@tauri-apps/api/core';
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { sendNotification } from '@tauri-apps/plugin-notification';
import type {
  InterimSegment,
  MeetingRecord,
  ProviderCapabilities,
  TranscriptionSegment
} from '$lib/types';
import { appState } from '$lib/stores/app-state.svelte';
import { modelsStore } from '$lib/stores/models.svelte';
import { meetingsStore, PENDING_MEETING_ID } from '$lib/stores/meetings.svelte';
import { configStore } from '$lib/stores/config.svelte';
import { dictationStore } from '$lib/stores/dictation.svelte';
import { updaterStore } from '$lib/stores/updater.svelte';
import type { ModelProgress } from '$lib/types';

/// Whether a notification event may fire, per the user's notification config.
/// There is no master switch — each event owns its own toggle.
function notificationsAllowed(
  event: 'summary_ready' | 'recording_started' | 'call_detected'
): boolean {
  const cfg = configStore.config;
  if (!cfg) return false;
  switch (event) {
    case 'summary_ready':
      return cfg.notify_summary_ready;
    case 'recording_started':
      return cfg.notify_recording_started;
    case 'call_detected':
      return cfg.notify_call_detected;
  }
}

// Module-level registry of the listeners installed by the most recent
// `setupEventListeners` call. Vite HMR can remount the page component
// (which calls `setupEventListeners` again from `onMount`) without
// destroying the prior Tauri listeners, so without this guard each HMR
// cycle stacks another full set of listeners — observed in practice as
// transcript segments being persisted N× (one duplicate per HMR cycle).
let _activeUnlisteners: UnlistenFn[] | null = null;

export async function setupEventListeners(): Promise<UnlistenFn[]> {
  if (_activeUnlisteners) {
    for (const fn of _activeUnlisteners) {
      try {
        fn();
      } catch {
        // Listener already torn down; ignore.
      }
    }
    _activeUnlisteners = null;
  }

  const unlisteners: UnlistenFn[] = [];

  unlisteners.push(
    await listen<{ capabilities: ProviderCapabilities; started_at: number }>(
      'recording-started',
      async (e) => {
        appState.setView('recording');
        appState.setRecording(true);
        appState.startRecordingClock(e.payload.started_at);
        appState.clearSegments();
        appState.setError(null);
        appState.setFallbackNotice(null);
        appState.setDetectedApp(null);
        appState.setProviderCapabilities(e.payload.capabilities);

        // Heads-up when a recording begins while the window is hidden (tray
        // start today; auto-detection in R3 makes this the main path).
        const visible = await getCurrentWindow().isVisible();
        if (!visible && notificationsAllowed('recording_started')) {
          await sendNotification({
            title: 'Recording started',
            body: 'embral is recording this meeting.'
          });
        }
      }
    )
  );

  unlisteners.push(
    await listen<{ mic: number[]; system: number[] }>('audio-level', (e) => {
      appState.setAudioLevel(e.payload.mic, e.payload.system);
    })
  );

  // The encoded MP3 lands well before the notes finish — the pending
  // meeting mounts its player as soon as it exists.
  unlisteners.push(
    await listen<string>('pending-audio-ready', (e) => {
      appState.setPendingAudioPath(e.payload);
    })
  );

  unlisteners.push(
    await listen('recording-paused', () => {
      appState.setPaused(true);
    })
  );

  unlisteners.push(
    await listen('recording-stopped', () => {
      // Capture where each star sits in the notes before the recording
      // view unmounts, so the saved notes can re-anchor them.
      const anchors = appState.collectStarAnchors();
      if (anchors.length > 0) {
        invoke('set_star_anchors', { anchors }).catch((e) =>
          console.error('set_star_anchors failed:', e)
        );
      }
      // Straight back to the Meetings page: the just-stopped meeting shows
      // there immediately as a pending entry (transcript in hand, notes and
      // audio in progress) instead of a separate processing screen.
      appState.setRecording(false);
      appState.beginPendingMeeting();
      appState.setView('idle');
      void meetingsStore.select(PENDING_MEETING_ID);
    })
  );

  unlisteners.push(
    await listen<TranscriptionSegment>('transcription-segment', (e) => {
      appState.addSegment(e.payload);
    })
  );

  // Cloud transcription died mid-recording and a local session took over
  // (cloud builds only ever emit this). The recording continues; the
  // banner explains the switch.
  unlisteners.push(
    await listen<{ message: string }>('transcription-fallback', async (e) => {
      appState.setFallbackNotice(
        `Switched to local transcription (${e.payload.message}).`
      );
      const visible = await getCurrentWindow().isVisible();
      // A mid-recording provider switch is news about the recording, so it
      // rides the recording toggle now that there is no master switch.
      if (!visible && notificationsAllowed('recording_started')) {
        await sendNotification({
          title: 'Switched to local transcription',
          body: 'The recording continues on this device.'
        });
      }
    })
  );

  // The session died with nothing to fall back to; live transcription is
  // over for this recording (audio capture itself continues).
  unlisteners.push(
    await listen<{ message: string }>('transcription-failed', (e) => {
      appState.setError(`Live transcription stopped: ${e.payload.message}`);
    })
  );

  // Cloud hours ran out (or the cloud refused) with "disable transcription"
  // chosen: the recording and notes continue, deliberately without a
  // transcript. A notice, not an error — this is the configured behavior.
  unlisteners.push(
    await listen<{ message: string }>('transcription-disabled', (e) => {
      appState.setFallbackNotice(
        `Transcription is off for this recording (${e.payload.message}). Audio and notes continue.`
      );
    })
  );

  unlisteners.push(
    await listen<InterimSegment>('transcription-interim', (e) => {
      appState.setInterim(e.payload);
    })
  );

  unlisteners.push(
    await listen<string>('transcription-final-complete', () => {
      appState.setProcessingStep('finalizing-transcript');
    })
  );

  unlisteners.push(
    await listen('notes-generation-started', () => {
      appState.setProcessingStep('generating-notes');
      appState.setPendingStage('notes');
    })
  );

  unlisteners.push(
    await listen<MeetingRecord>('notes-generation-complete', async (e) => {
      appState.setImporting(false);
      // Imports still use the processing view; live meetings finish on the
      // Meetings page with the pending entry selected.
      const wasProcessing = appState.view === 'processing';
      const pendingSelected = meetingsStore.selectedId === PENDING_MEETING_ID;
      if (wasProcessing) {
        appState.resetToIdle();
      }
      if (wasProcessing || pendingSelected || meetingsStore.selectedId === null) {
        await meetingsStore.refreshAndSelect(e.payload.id);
      } else {
        // The user moved on to another meeting — don't steal the selection.
        await meetingsStore.load();
      }
      // Clear the pending entry only after the real record took over the
      // selection, so the detail pane never blinks through an empty state.
      appState.clearPendingMeeting();

      const visible = await getCurrentWindow().isVisible();
      if (!visible && notificationsAllowed('summary_ready')) {
        await sendNotification({
          title: 'Meeting notes ready',
          body: e.payload.title
        });
      }
    })
  );

  unlisteners.push(
    await listen<string>('processing-error', (e) => {
      appState.setError(e.payload);
      appState.setPendingError(e.payload);
    })
  );

  // Meeting detection (prompt policy): show the in-app banner and, when the
  // window is hidden, a notification. The banner clears when the call ends
  // or a recording starts.
  unlisteners.push(
    await listen<{ app: string }>('meeting-detected', async (e) => {
      appState.setDetectedApp(e.payload.app);
      const visible = await getCurrentWindow().isVisible();
      if (!visible && notificationsAllowed('call_detected')) {
        await sendNotification({
          title: 'Call in progress',
          body: `${e.payload.app} is using your microphone. Open embral to record it.`
        });
      }
    })
  );

  unlisteners.push(
    await listen('meeting-ended', () => {
      appState.setDetectedApp(null);
    })
  );

  // Import flow: file transcription progress renders in the processing view;
  // completion arrives through the shared notes-generation-complete path.
  unlisteners.push(
    await listen('import-started', () => {
      appState.setImporting(true);
      appState.setError(null);
      appState.setView('processing');
      appState.setProcessingStep('transcribing-import');
    })
  );

  unlisteners.push(
    await listen<{ fraction: number }>('import-progress', (e) => {
      appState.setImportFraction(e.payload.fraction);
    })
  );

  // Local model downloads — handled globally so progress and the
  // configured-state refresh survive leaving the Settings view mid-download.
  unlisteners.push(
    await listen<ModelProgress>('model-download-progress', (e) => {
      modelsStore._onProgress(e.payload);
    })
  );

  unlisteners.push(
    await listen('model-download-complete', () => {
      // Refresh statuses so `configStore.isConfigured` (and the record
      // button) update immediately.
      modelsStore._onComplete();
    })
  );

  unlisteners.push(
    await listen<boolean>('dictation-active', (e) => {
      dictationStore._setActive(e.payload);
    })
  );

  _activeUnlisteners = unlisteners;
  scheduleStartupUpdateCheck();
  return unlisteners;
}

// One quiet update check per app run, well after boot so it never competes
// with the startup path — and skipped outright if a recording is already
// live (an auto-detected meeting can start before the timer fires). The
// flag lives at module level so HMR remounts don't stack timers.
let _updateCheckScheduled = false;
function scheduleStartupUpdateCheck() {
  if (_updateCheckScheduled) return;
  _updateCheckScheduled = true;
  setTimeout(() => {
    if (appState.isRecording) return;
    void updaterStore.checkNow({ silent: true, notify: true });
  }, 30_000);
}
