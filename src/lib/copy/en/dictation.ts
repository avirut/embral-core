// The Dictation surface: the history page and the always-on-top overlay
// window shown while a dictation runs.

export const dictation = {
  // The Dictation page (history).
  home: {
    title: 'Dictation',
    hotkeyTip: 'Change in dictation settings',
    hotkeyAria: 'Dictation hotkey — change in settings',
    setHotkey: 'Set a hotkey',
    stop: 'Stop',
    dictate: 'Dictate',
    emptyTitle: 'Speak instead of typing',
    emptyBody: 'Dictations are transcribed on this computer, optionally cleaned up by AI, pasted where your cursor is, and kept here.',
    openSettings: 'Open dictation settings',
    // The per-entry toggle between the cleaned and raw text.
    raw: 'raw',
    cleaned: 'cleaned',
    copy: 'Copy',
    delete: 'Delete',
    copied: 'Copied'
  },

  // The overlay pill's status line.
  overlay: {
    listening: 'Listening',
    finishing: 'Finishing up…'
  }
};
