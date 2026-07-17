import { invoke } from '@tauri-apps/api/core';
import type { SpeakerProfile } from '$lib/types';

function isTauri() {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

// Module-level so the registry survives navigating between views, and so an
// in-flight enrollment countdown keeps running if the page re-renders.
let _speakers = $state<SpeakerProfile[]>([]);
let _loaded = $state(false);
// "speakerId:slot" while that slot is recording, else null.
let _recordingSlot = $state<string | null>(null);
let _error = $state<string | null>(null);

export const speakersStore = {
  get speakers() {
    return _speakers;
  },
  get loaded() {
    return _loaded;
  },
  get error() {
    return _error;
  },
  get recordingSlot() {
    return _recordingSlot;
  },
  byId(id: string): SpeakerProfile | undefined {
    return _speakers.find((s) => s.id === id);
  },

  async refresh() {
    if (!isTauri()) return;
    try {
      _speakers = await invoke<SpeakerProfile[]>('list_speakers');
      _loaded = true;
    } catch (e) {
      _error = String(e);
    }
  },

  async save(profile: {
    id?: string;
    name: string;
    notes: string;
    is_you: boolean;
  }): Promise<SpeakerProfile | null> {
    if (!isTauri()) return null;
    _error = null;
    try {
      const saved = await invoke<SpeakerProfile>('upsert_speaker', {
        id: profile.id ?? null,
        name: profile.name,
        notes: profile.notes,
        isYou: profile.is_you
      });
      await this.refresh();
      return saved;
    } catch (e) {
      _error = String(e);
      return null;
    }
  },

  async remove(id: string) {
    if (!isTauri()) return;
    _error = null;
    try {
      await invoke('delete_speaker', { id });
      await this.refresh();
    } catch (e) {
      _error = String(e);
    }
  },

  /** Delete several people at once (the list's multi-select). */
  async removeMany(ids: string[]) {
    if (!isTauri() || ids.length === 0) return;
    _error = null;
    try {
      await invoke('delete_speakers', { ids });
      await this.refresh();
    } catch (e) {
      _error = String(e);
    }
  },

  /// Kick off a 10 s enrollment capture; resolves when the clip is stored.
  async recordSlot(speakerId: string, slot: number) {
    if (!isTauri() || _recordingSlot) return;
    _error = null;
    _recordingSlot = `${speakerId}:${slot}`;
    try {
      await invoke('record_voice_reference', { speakerId, slot });
      await this.refresh();
    } catch (e) {
      _error = String(e);
    } finally {
      _recordingSlot = null;
    }
  },

  async cancelRecording() {
    if (!isTauri()) return;
    try {
      await invoke('cancel_voice_reference');
    } catch {
      // The capture ends on its own timer anyway.
    }
  },

  async deleteSlot(refId: number) {
    if (!isTauri()) return;
    _error = null;
    try {
      await invoke('delete_voice_reference', { refId });
      await this.refresh();
    } catch (e) {
      _error = String(e);
    }
  },

  clearError() {
    _error = null;
  }
};
