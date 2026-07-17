import { invoke } from "@tauri-apps/api/core";

export async function openNotesFolder(): Promise<void> {
  try {
    await invoke("open_notes_folder");
  } catch (e) {
    console.error("Failed to open notes folder:", e);
  }
}
