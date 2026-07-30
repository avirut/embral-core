//! The platform seam: every OS-specific mechanism in the app lives behind
//! this module, `std::sys`-style — sibling directories with mirrored
//! filenames, selected at compile time. The rest of `src-tauri` calls
//! `crate::platform::…` and never names an OS API
//! ([architecture.md](../../docs/architecture.md) §Platform layer).
//!
//! ## The contract
//!
//! Each platform directory implements, in same-named files (landing over
//! the port's phases — [260725-macos-port.md]):
//!
//! - `mic_users.rs` — `processes_using_microphone(exclude_pid) ->
//!   Vec<AppId>`: the apps holding an active microphone stream
//!   (detection's signal).
//! - `input.rs` — `paste_keystroke()` (synthesize the platform paste
//!   chord into the focused app) and `focused_app() -> Option<AppId>`.
//! - `theme.rs` — the OS shell theme + accent the tray icons follow, and
//!   a change watcher.
//! - `supervisor.rs` — children die with this process however it dies.
//! - `proc.rs` — spawn decoration (`hide_console`), executable naming
//!   (`exe_name`), CLI resolution (`find_cli`).
//! - `mcp_paths.rs` — where AI clients keep their MCP configs.
//! - `overlay.rs` — `style_overlay(native_window)`: extra panel behaviors
//!   for the dictation overlay (macOS joins Spaces; Windows no-op).
//! - `notice.rs` — `style_notice(native_window)`: the notice window's
//!   never-activate styling (Windows `WS_EX_NOACTIVATE`; macOS no-op —
//!   the window never exists there).
//! - `power.rs` — `power_source() -> PowerSource`: wall power vs battery,
//!   read once per recording by the provider policy
//!   ([transcription.md](../../../docs/transcription.md)).
//! - `ocr.rs` — `recognize_text(bytes) -> Recognized`: the text inside a
//!   pasted image, read by the in-box engine
//!   (`Windows.Media.Ocr` / Vision) so nothing is downloaded or bundled.
//!   Takes bytes, not a path — both engines prefer them and file IO stays
//!   above the seam ([storage.md](../../../docs/storage.md)).
//! - `os_build()` — the OS version string telemetry reports.
//!
//! Stub rule: a platform that lacks a capability returns the inert value
//! (`None`, empty vec, no-op) — callers already degrade gracefully and
//! must never need `cfg` at the call site.
//!
//! [260725-macos-port.md]: ../../docs/plans/260725-macos-port.md

pub mod types;

/// The shell theme at this instant — what `theme_snapshot()` reports.
pub struct ThemeSnapshot {
    /// Whether the surface our icons sit on (taskbar / menu bar) is light.
    pub is_light: bool,
    /// The OS accent color, with a platform-appropriate fallback baked in.
    pub accent_rgb: [u8; 3],
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;
