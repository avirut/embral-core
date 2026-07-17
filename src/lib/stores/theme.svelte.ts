import type { Theme } from "$lib/types";

// Applies the color scheme by toggling `.dark` on <html> (the palettes in
// layout.css key off it). `system` follows the OS via prefers-color-scheme
// and reacts to OS changes live. `apply()` is driven from an $effect in the
// shell so it re-runs whenever config.theme changes.

let _current = $state<Theme>("system");
let media: MediaQueryList | null = null;

function systemPrefersDark(): boolean {
  return window.matchMedia("(prefers-color-scheme: dark)").matches;
}

function setClass(dark: boolean) {
  document.documentElement.classList.toggle("dark", dark);
}

function onSystemChange() {
  if (_current === "system") setClass(systemPrefersDark());
}

export const themeStore = {
  get current(): Theme {
    return _current;
  },

  apply(theme: Theme) {
    _current = theme;
    if (!media) {
      media = window.matchMedia("(prefers-color-scheme: dark)");
      media.addEventListener("change", onSystemChange);
    }
    setClass(theme === "dark" || (theme === "system" && systemPrefersDark()));
  },

  /// The next theme in the cycle system → light → dark → system.
  next(): Theme {
    return _current === "system"
      ? "light"
      : _current === "light"
        ? "dark"
        : "system";
  },
};
