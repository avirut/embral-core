// The auto-updater's state, and the two actions the UI exposes: check and
// install. Checking is cheap and quiet; installing restarts the app, so it
// first asks the backend's `update_guard` whether anything (recording,
// dictation, import, enrollment) would die with it — the answer is a
// human-readable reason to wait, surfaced verbatim.
//
// The startup check runs once, delayed, from events.ts. A 404 on the
// endpoint is normal until the first release exists, so background checks
// are silent; only a manual "Check for updates" reports errors.
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { sendNotification } from '@tauri-apps/plugin-notification';
import { relaunch } from '@tauri-apps/plugin-process';
import { check, type Update } from '@tauri-apps/plugin-updater';
import { configStore } from './config.svelte';

class UpdaterStore {
  private _checking = $state(false);
  private _installing = $state(false);
  private _available = $state<{ version: string; notes: string } | null>(null);
  private _error = $state<string | null>(null);
  private _blocked = $state<string | null>(null);
  private update: Update | null = null;

  get checking() {
    return this._checking;
  }
  get installing() {
    return this._installing;
  }
  /** The staged update, when one exists. */
  get available() {
    return this._available;
  }
  /** A manual check's failure, verbatim. */
  get error() {
    return this._error;
  }
  /** Why the last install attempt refused (a recording, etc.). */
  get blocked() {
    return this._blocked;
  }

  /**
   * Ask the endpoint for a newer version. `silent` swallows errors (the
   * startup check — the endpoint 404s until a first release exists);
   * `notify` sends the update-ready notification when the window is hidden.
   */
  async checkNow(opts: { silent?: boolean; notify?: boolean } = {}): Promise<void> {
    if (this._checking || this._installing) return;
    this._checking = true;
    this._error = null;
    try {
      const found = await check();
      if (found) {
        this.update = found;
        this._available = { version: found.version, notes: found.body ?? '' };
        if (opts.notify && configStore.config?.notify_update_available) {
          const visible = await getCurrentWindow().isVisible();
          if (!visible) {
            await sendNotification({
              title: 'Update ready',
              body: `embral ${found.version} is available. Install it from Settings → About.`
            });
          }
        }
      } else {
        this.update = null;
        this._available = null;
      }
    } catch (e) {
      if (!opts.silent) this._error = e instanceof Error ? e.message : String(e);
    } finally {
      this._checking = false;
    }
  }

  /**
   * Download, install, and relaunch. Refuses while the backend reports the
   * app busy — the reason lands in `blocked` for the UI to show. The
   * relaunch exits through RunEvent::Exit, so the LLM and embedding
   * children are shut down like any other quit.
   */
  async install(): Promise<void> {
    if (!this.update || this._installing) return;
    this._blocked = null;
    this._error = null;
    const reason = await invoke<string | null>('update_guard');
    if (reason) {
      this._blocked = reason;
      return;
    }
    this._installing = true;
    try {
      await this.update.downloadAndInstall();
      await relaunch();
    } catch (e) {
      this._error = e instanceof Error ? e.message : String(e);
      this._installing = false;
    }
  }
}

export const updaterStore = new UpdaterStore();
