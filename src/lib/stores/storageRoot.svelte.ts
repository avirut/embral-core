import { invoke } from '@tauri-apps/api/core';

/** The library's absolute path, which is what turns a stored
 * `assets/{id}/img-01.png` into something the webview can load. Read once
 * and kept: it only changes when the user moves the library, which the
 * settings save re-reads. */
let _root = $state('');

function isTauri() {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export const storageRoot = {
  get value() {
    return _root;
  },

  async load() {
    if (!isTauri()) return;
    try {
      _root = await invoke<string>('storage_root');
    } catch (e) {
      console.warn('[embral] could not resolve the library path; images may not render', e);
    }
  }
};
