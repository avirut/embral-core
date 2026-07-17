# embral

Embral is a Windows desktop app for recording meetings, transcribing them
live, and turning the conversation into structured notes — entirely on your
machine.

It lives in the system tray, records microphone and system audio (so it
works even when you're on a call with headphones in), shows a live
transcript, and writes polished meeting notes when the recording ends. No
account, no API key, and no network connection are needed: speech models
are downloaded on first run and everything after that happens on-device.

## What it does

- Records meetings from both microphone input and system audio, with
  automatic call detection.
- Transcribes live, on the machine, with optional speaker labels that
  recognize saved voices across meetings.
- Generates structured meeting notes with an on-device language model.
- Dictation into any Windows app via a global hotkey, with AI cleanup.
- Searches everything — by keyword, and by meaning once the optional
  semantic-search model is downloaded.
- Mirrors finished notes into an Obsidian vault and/or POSTs a JSON summary
  to a webhook.
- Includes an MCP server so Claude, Codex, and other MCP clients can list,
  search, and read your meetings.

## This repository

This is the **offline core** of embral — the complete source of the
on-device edition. The packaged app distributed on the
[releases page](https://github.com/avirut/embral-core/releases) is the same
app plus an optional paid cloud tier (faster transcription, cloud
summaries); its cloud code is not part of this repository.

## Building the offline edition

Prerequisites (Windows):

- Rust (stable, MSVC toolchain)
- Node.js 22+ and pnpm
- The [Tauri 2 Windows prerequisites](https://tauri.app/start/prerequisites/)
  (WebView2, Visual Studio Build Tools)

```powershell
pnpm install
pnpm tauri build
```

The installer lands in `target/release/bundle/nsis/`. For development:

```powershell
pnpm tauri dev
```

Transcription, speaker, and language models are downloaded from within the
app (Settings → Models) on first use; nothing is bundled.

## License

[MIT](./LICENSE).
