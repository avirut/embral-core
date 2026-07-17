import { invoke } from '@tauri-apps/api/core';
import type { DictationRow } from '$lib/types';

function isTauri() {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

let _history = $state<DictationRow[]>([]);
let _active = $state(false);
let _error = $state<string | null>(null);

export const dictationStore = {
  get history() {
    return _history;
  },
  get active() {
    return _active;
  },
  get error() {
    return _error;
  },

  async refresh() {
    if (!isTauri()) return;
    try {
      _history = await invoke<DictationRow[]>('list_dictations', { limit: 100 });
    } catch (e) {
      _error = String(e);
    }
  },

  async start() {
    if (!isTauri()) return;
    _error = null;
    try {
      await invoke('start_dictation');
    } catch (e) {
      _error = String(e);
    }
  },

  async stop() {
    if (!isTauri()) return;
    _error = null;
    try {
      await invoke('stop_dictation');
      await this.refresh();
    } catch (e) {
      _error = String(e);
    }
  },

  async remove(id: number) {
    if (!isTauri()) return;
    try {
      await invoke('delete_dictation', { id });
      _history = _history.filter((d) => d.id !== id);
    } catch (e) {
      _error = String(e);
    }
  },

  // Wired from the global event listeners in events.ts.
  _setActive(active: boolean) {
    _active = active;
    if (!active) void this.refresh();
  }
};
