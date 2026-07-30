// Settings → Synthesis. Manages the on-device LLM and search-model files
// (engine choice lives with each feature) and the local-model performance
// knobs.

export const synthesis = {
  localLlm: 'Local LLM',
  search: 'Search',

  performance: {
    _group: 'Performance',
    keepWarm: {
      label: 'Keep the built-in model loaded',
      sub: 'Responds instantly but holds ~3 GB of memory. Applies while summaries or dictation cleanup run on this device.'
    },
    unloadIdle: {
      label: 'Unload after idle',
      unit: 'minutes'
    }
  }
};
