// Settings → Meetings. The densest settings page: transcription, auto-start,
// the record hotkey, speakers, summaries, and retention. Reads top to bottom
// in the order the groups appear on screen.

export const meetings = {
  transcription: {
    _group: 'Transcription',
    // Passed to the shared TranscriptionBlock as this page's provider prompt.
    providerLabel: 'Transcribe meetings with',
    disabledNote: 'Recording and notes continue; no transcript is written.'
  },

  autoStart: {
    _group: 'Auto-start',
    prompt: {
      label: 'When a call is detected, auto-start…',
      // Keyed by the AutoStartPolicy union.
      options: {
        always: 'Always',
        selective: 'Selectively',
        prompt: 'After asking',
        manual: 'Never'
      }
    },
    // The fixed meeting-app grid. Keys are this page's; each app's
    // process-match string stays in the component (it is detector data, not
    // copy).
    apps: {
      label: 'Meeting apps',
      names: {
        zoom: 'Zoom',
        teams: 'Teams',
        chrome: 'Chrome',
        edge: 'Edge',
        // Rendered on macOS only (the grid is platform-keyed detector data).
        safari: 'Safari',
        // Rendered on Linux only, and a separate row from Chrome rather than
        // folded into it: the detector's substring match is bidirectional,
        // but neither "chrome" nor "chromium" contains the other.
        chromium: 'Chromium',
        firefox: 'Firefox',
        slack: 'Slack',
        discord: 'Discord',
        webex: 'Webex'
      }
    },
    delay: {
      label: 'Detection delay',
      sub: 'Active microphone time before recording is triggered',
      unit: 'seconds'
    },
    autoStop: {
      label: 'Stop when the call ends',
      // Keyed by the AutoStopScope union.
      options: {
        never: 'Never',
        auto_started: 'Auto-started recordings',
        all: 'All recordings'
      }
    },
    silence: {
      label: 'Ask after silence',
      sub: 'Check in when no speech has been transcribed for this long; 0 never asks',
      unit: 'minutes'
    },
    silenceUnanswered: {
      label: 'If unanswered',
      // Keyed by the SilenceUnanswered union.
      options: {
        stop: 'Stop the recording',
        keep: 'Keep recording'
      }
    }
  },

  toggle: {
    _group: 'Meeting hotkey',
    hotkey: {
      label: 'Hotkey',
      aria: 'Meeting hotkey'
    }
  },

  speakers: {
    _group: 'Speakers',
    detect: { label: 'Detect speakers' },
    separation: {
      label: 'Speaker separation',
      // Keyed by the DiarizationSensitivity union.
      options: {
        low: 'Fewer speakers',
        medium: 'Balanced',
        high: 'More speakers'
      }
    },
    naming: {
      label: 'Name speakers from your notes',
      // Keyed by the NotesNamingMode union.
      options: {
        off: 'Off',
        suggest: 'Suggest',
        automatic: 'Automatic'
      }
    }
  },

  summaries: {
    _group: 'Summaries',
    enabled: { label: 'Summarize meetings' },
    engine: { label: 'Write summaries with' },
    prompt: {
      label: 'Summary prompt',
      customized: 'Customized.',
      edit: 'Edit prompt…'
    },
    openOn: {
      label: 'Open meetings on',
      // Keyed by the OpenMeetingTab union.
      options: {
        summary: 'Summary',
        notes: 'Notes',
        transcript: 'Transcript'
      }
    }
  },

  audio: {
    _group: 'Audio recordings',
    keep: { label: 'Keep audio files' },
    deleteAudio: {
      label: 'Delete audio automatically',
      options: {
        never: 'Never',
        d7: 'After 7 days',
        d30: 'After 30 days',
        d90: 'After 90 days'
      }
    },
    deleteMeetings: {
      label: 'Delete meetings automatically',
      options: {
        never: 'Never',
        d90: 'After 90 days',
        y1: 'After 1 year',
        y2: 'After 2 years'
      }
    }
  },

  // The "edit the summary prompt" dialog.
  promptDialog: {
    title: 'Summary prompt',
    description:
      'The full prompt sent with every meeting. Edit anything; the required output format is appended automatically.',
    // A verb-swap in the original ("{Hide|Show} the enforced output format");
    // two complete strings so a translator isn't handed a sentence with a
    // hole in it.
    showFormat: 'Show the enforced output format',
    hideFormat: 'Hide the enforced output format',
    reset: 'Reset to default',
    done: 'Done'
  }
};
