import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { appState } from "$lib/stores/app-state.svelte";

/// Pick an audio file and start a local-engine import. Progress and
/// completion flow through the global event listeners.
export async function importRecording(): Promise<void> {
  const path = await open({
    title: "Import a recording",
    filters: [
      {
        name: "Audio",
        extensions: ["mp3", "wav", "m4a", "aac", "mp4"],
      },
    ],
  });
  if (typeof path !== "string") return;
  try {
    await invoke("import_recording", { path, title: null });
  } catch (e) {
    appState.setError(e instanceof Error ? e.message : String(e));
  }
}
