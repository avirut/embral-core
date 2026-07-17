import { configStore } from "$lib/stores/config.svelte";
import type { AppConfig } from "$lib/types";

// One shared draft of the config, edited by every settings section and
// autosaved with a debounce. Module-level so the draft (and a pending save)
// survives switching between sections. SettingsLayout owns the $effect that
// snapshots the draft and calls scheduleSave.

let _draft = $state<AppConfig | null>(null);
let _saveTimer: ReturnType<typeof setTimeout> | null = null;

export const settingsForm = {
  get draft() {
    return _draft;
  },

  /// (Re)initialize the draft from persisted config — call when Settings opens
  /// so edits from other sources (onboarding, sidebar theme cycle) are picked up.
  reset() {
    _draft = configStore.config ? { ...configStore.config } : null;
  },

  scheduleSave(snapshot: AppConfig) {
    if (_saveTimer) clearTimeout(_saveTimer);
    _saveTimer = setTimeout(() => {
      void configStore.save(snapshot);
    }, 300);
  },
};
