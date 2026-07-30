// Event-driven status messages fired from events.ts and the updater store:
// OS notifications shown while the window is hidden, and the in-app notices
// that ride the recording banner or the error slot.
//
// These have no on-screen review path — an OS notification fires only when a
// recording starts with the window hidden, the updater message only on a real
// update. The type check and the corpus test are their safety net.
//
// The `message` arguments are the mapped sentence from a backend `AppError`
// (via errorMessage in ../errors.ts) interpolated into a frame — no longer a
// raw backend string (docs/copy.md).

export const notifications = {
  // Notices: one line each — the title is the whole message, with the
  // answers beside it ([shell.md] §Notices).
  os: {
    recordingStarted: { title: 'Recording started' },
    switchedToLocal: { title: 'Switched to local transcription' },
    notesReady: { title: 'Meeting notes ready' },
    callDetected: {
      title: (app: string) => `${app} call detected`,
      starting: 'Starting the recording…'
    },
    stillRecording: { title: 'Still recording?' },
    startFailed: { title: "Couldn't start recording" },
    updateReady: { title: 'Update ready' }
  },

  // In-app notices shown in the recording banner or the error slot.
  notices: {
    switchedToLocal: (message: string) =>
      `Switched to local transcription (${message}).`,
    transcriptionStopped: (message: string) =>
      `Live transcription stopped: ${message}`,
    transcriptionOff: (message: string) =>
      `Transcription is off for this recording (${message}). Audio and notes continue.`
  }
};
