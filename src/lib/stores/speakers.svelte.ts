import { invoke } from '@tauri-apps/api/core';
import type { SpeakerProfile } from '$lib/types';

function isTauri() {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

// Module-level so the registry survives navigating between views.
let _speakers = $state<SpeakerProfile[]>([]);
let _loaded = $state(false);
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
  }): Promise<SpeakerProfile | null> {
    if (!isTauri()) return null;
    _error = null;
    try {
      const saved = await invoke<SpeakerProfile>('upsert_speaker', {
        id: profile.id ?? null,
        name: profile.name,
        notes: profile.notes
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

  clearError() {
    _error = null;
  }
};
