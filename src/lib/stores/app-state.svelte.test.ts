import { beforeEach, describe, expect, it } from 'vitest';
import { appState } from './app-state.svelte';

// The store is a module singleton, so every test starts from a clean slate.
describe('recording pause state', () => {
  beforeEach(() => {
    appState.resetToIdle();
  });

  it('does not leak a paused flag into the next recording', () => {
    // Regression (260725): stopping a meeting while paused left `_isPaused`
    // set — the next meeting's timer effect never started and the header
    // showed Resume while the backend recorded unpaused.
    appState.setRecording(true);
    appState.startRecordingClock(Date.now() - 60_000);
    appState.setPaused(true);
    appState.setRecording(false);

    appState.setRecording(true);
    appState.startRecordingClock(Date.now());

    expect(appState.isPaused).toBe(false);
  });

  it('still excludes paused spans from the elapsed clock', () => {
    const t0 = Date.now() - 10_000;
    appState.setRecording(true);
    appState.startRecordingClock(t0);
    // A pause opens a live span; elapsedSeconds must not count past it.
    appState.setPaused(true);
    const during = appState.elapsedSeconds(Date.now());
    appState.setPaused(false);
    expect(appState.elapsedSeconds(Date.now())).toBeGreaterThanOrEqual(during);
  });
});

describe('speaker labeling standing', () => {
  beforeEach(() => {
    appState.resetToIdle();
    appState.setLiveDiarization(true);
  });

  it('separates the guard standing labeling down from the user doing it', () => {
    // The header note names the reason, so the two must not look alike.
    appState.setLiveDiarization(false);
    expect(appState.liveDiarization).toBe(false);
    expect(appState.diarizationRunaway).toBe(false);

    appState.standDownDiarization();
    expect(appState.liveDiarization).toBe(false);
    expect(appState.diarizationRunaway).toBe(true);
  });

  it('drops the guard reason when labeling is set deliberately', () => {
    // Turning labeling back on, and the focus-time reconcile adopting the
    // backend's flag, both leave "too many speakers" untrue.
    appState.standDownDiarization();
    appState.setLiveDiarization(true);
    expect(appState.diarizationRunaway).toBe(false);
  });
});
