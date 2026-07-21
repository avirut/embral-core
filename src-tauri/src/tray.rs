//! The tray icon and the running window's taskbar icon. Both carry the bare
//! embral mark in whichever shade the taskbar needs — white on a dark
//! taskbar, black on a light one. The shade comes from the Windows
//! `SystemUsesLightTheme` registry value (the *taskbar* theme; the sibling
//! `AppsUseLightTheme` is the app theme users set independently), and a
//! registry watcher refreshes the icons when it changes ([shell.md]). The
//! installed icon set (Start menu, installer) is static and keeps its dark
//! tile — only these two runtime surfaces follow the theme.
//!
//! While recording, the whole tray mark — circle and lines — is tinted at
//! runtime in the Windows accent color (or the `tray_recording_color`
//! preset).

use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Manager,
};

const TRAY_ID: &str = "main";
const MARK_WHITE_32: &[u8] = include_bytes!("../icons/mark-white-32.png");
const MARK_BLACK_32: &[u8] = include_bytes!("../icons/mark-black-32.png");
const MARK_WHITE_64: &[u8] = include_bytes!("../icons/mark-white-64.png");
const MARK_BLACK_64: &[u8] = include_bytes!("../icons/mark-black-64.png");
const TRAY_SIZE: u32 = 32;

/// Windows' default accent blue — the disc color when the registry read
/// fails and no override is set.
const FALLBACK_ACCENT: [u8; 3] = [0x00, 0x78, 0xd4];

/// Whether a recording is running; the refresh derives the tray icon from it.
static RECORDING: AtomicBool = AtomicBool::new(false);

/// The `tray_recording_color` override, parsed once at config load/save.
/// `None` = follow the Windows accent color.
static RECORDING_COLOR: Mutex<Option<[u8; 3]>> = Mutex::new(None);

pub fn create_tray(app: &App) -> Result<()> {
    let show = MenuItemBuilder::with_id("show", "Show Window").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let sep = PredefinedMenuItem::separator(app)?;

    let menu = MenuBuilder::new(app).items(&[&show, &sep, &quit]).build()?;

    let idle = if taskbar_is_light() {
        MARK_BLACK_32
    } else {
        MARK_WHITE_32
    };
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(idle)?)
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("main") {
                    if w.is_visible().unwrap_or(false) {
                        let _ = w.hide();
                    } else {
                        let _ = w.show();
                        let _ = w.set_focus();
                    }
                }
            }
        })
        .build(app)?;

    refresh(app.handle())?;
    watch_theme(app.handle().clone());
    Ok(())
}

pub fn update_tray_recording_state(app: &AppHandle, recording: bool) -> Result<()> {
    RECORDING.store(recording, Ordering::Relaxed);
    refresh(app)
}

/// Re-parse the recording-disc override. Called at startup and whenever the
/// config is saved; anything that isn't `#RRGGBB` means "follow the accent".
pub fn set_recording_color(hex: &str) {
    *RECORDING_COLOR.lock().unwrap() = parse_hex(hex);
}

/// Re-derive both runtime icons from current state: the tray from the
/// recording flag, taskbar shade, and disc color; the window's taskbar icon
/// from the shade alone. The window may not exist yet during setup —
/// skipped, and the next refresh catches it.
pub fn refresh(app: &AppHandle) -> Result<()> {
    let light = taskbar_is_light();
    let tray_icon = if RECORDING.load(Ordering::Relaxed) {
        recording_icon()?
    } else if light {
        Image::from_bytes(MARK_BLACK_32)?
    } else {
        Image::from_bytes(MARK_WHITE_32)?
    };
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_icon(Some(tray_icon))?;
    }
    let window_bytes = if light { MARK_BLACK_64 } else { MARK_WHITE_64 };
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_icon(Image::from_bytes(window_bytes)?);
    }
    Ok(())
}

/// The current Windows accent color as `#RRGGBB` — the swatch beside the
/// "Windows accent" choice in settings.
#[tauri::command]
pub fn system_accent_color() -> String {
    let [r, g, b] = accent_color();
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// The recording tray icon: the whole mark tinted the override-or-accent
/// color.
fn recording_icon() -> Result<Image<'static>> {
    let color = RECORDING_COLOR
        .lock()
        .unwrap()
        .unwrap_or_else(accent_color);
    let mark = Image::from_bytes(MARK_WHITE_32)?;
    let buf = tint(mark.rgba(), color);
    Ok(Image::new_owned(buf, TRAY_SIZE, TRAY_SIZE))
}

/// `#RRGGBB` → RGB; anything else is None (= follow the accent).
fn parse_hex(hex: &str) -> Option<[u8; 3]> {
    let hex = hex.trim().strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// Recolor a mark: every pixel's RGB becomes `rgb` while its alpha — the
/// mark's shape, anti-aliased edges included — stays.
fn tint(rgba: &[u8], rgb: [u8; 3]) -> Vec<u8> {
    let mut out = rgba.to_vec();
    for px in out.chunks_exact_mut(4) {
        px[0] = rgb[0];
        px[1] = rgb[1];
        px[2] = rgb[2];
    }
    out
}

/// The DWM `AccentColor` value is ABGR (`0xAABBGGRR`).
fn accent_to_rgb(abgr: u32) -> [u8; 3] {
    [abgr as u8, (abgr >> 8) as u8, (abgr >> 16) as u8]
}

/// Whether the taskbar is light (missing value = dark, the Windows default).
#[cfg(windows)]
fn taskbar_is_light() -> bool {
    read_hkcu_dword(
        windows::core::w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
        windows::core::w!("SystemUsesLightTheme"),
    ) == Some(1)
}

#[cfg(not(windows))]
fn taskbar_is_light() -> bool {
    false
}

/// The user's Windows accent color; the stock blue when unreadable.
#[cfg(windows)]
fn accent_color() -> [u8; 3] {
    read_hkcu_dword(
        windows::core::w!("Software\\Microsoft\\Windows\\DWM"),
        windows::core::w!("AccentColor"),
    )
    .map(accent_to_rgb)
    .unwrap_or(FALLBACK_ACCENT)
}

#[cfg(not(windows))]
fn accent_color() -> [u8; 3] {
    FALLBACK_ACCENT
}

#[cfg(windows)]
fn read_hkcu_dword(
    subkey: windows::core::PCWSTR,
    value: windows::core::PCWSTR,
) -> Option<u32> {
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

    let mut data: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey,
            value,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut core::ffi::c_void),
            Some(&mut size),
        )
    };
    status.is_ok().then_some(data)
}

/// Refresh the icons whenever the taskbar theme or accent color key changes.
/// Registry notifications are one-shot, so the fired registration is
/// re-armed after each wake. Failure to set the watcher up leaves the icons
/// correct for the values read at startup — degraded, not broken.
#[cfg(windows)]
fn watch_theme(app: AppHandle) {
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Registry::{
        RegNotifyChangeKeyValue, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_NOTIFY,
        REG_NOTIFY_CHANGE_LAST_SET,
    };
    use windows::Win32::System::Threading::{CreateEventW, WaitForMultipleObjects, INFINITE};

    std::thread::spawn(move || {
        let paths = [
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("Software\\Microsoft\\Windows\\DWM"),
        ];
        let mut keys = [HKEY::default(); 2];
        let mut events = [HANDLE::default(); 2];
        for i in 0..2 {
            let opened = unsafe {
                RegOpenKeyExW(HKEY_CURRENT_USER, paths[i], Some(0), KEY_NOTIFY, &mut keys[i])
            };
            if opened.is_err() {
                tracing::warn!("tray theme watcher: failed to open a registry key");
                return;
            }
            match unsafe { CreateEventW(None, false, false, PCWSTR::null()) } {
                Ok(e) => events[i] = e,
                Err(_) => {
                    tracing::warn!("tray theme watcher: failed to create a wait event");
                    return;
                }
            }
        }
        let arm = |i: usize| -> bool {
            unsafe {
                RegNotifyChangeKeyValue(
                    keys[i],
                    false,
                    REG_NOTIFY_CHANGE_LAST_SET,
                    Some(events[i]),
                    true,
                )
            }
            .is_ok()
        };
        if !arm(0) || !arm(1) {
            tracing::warn!("tray theme watcher: failed to arm the notifications");
            return;
        }
        loop {
            let wait = unsafe { WaitForMultipleObjects(&events, false, INFINITE) };
            let idx = wait.0.wrapping_sub(WAIT_OBJECT_0.0) as usize;
            if idx >= events.len() {
                tracing::warn!("tray theme watcher: wait failed, stopping");
                return;
            }
            if let Err(e) = refresh(&app) {
                tracing::warn!("tray theme watcher: refresh failed: {e}");
            }
            if !arm(idx) {
                tracing::warn!("tray theme watcher: failed to re-arm, stopping");
                return;
            }
        }
    });
}

#[cfg(not(windows))]
fn watch_theme(_app: AppHandle) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parses_and_rejects() {
        assert_eq!(parse_hex("#cc0000"), Some([0xcc, 0, 0]));
        assert_eq!(parse_hex(" #00FF7f "), Some([0, 0xff, 0x7f]));
        assert_eq!(parse_hex(""), None);
        assert_eq!(parse_hex("cc0000"), None);
        assert_eq!(parse_hex("#cc00"), None);
        assert_eq!(parse_hex("#zzzzzz"), None);
    }

    #[test]
    fn accent_dword_is_abgr() {
        // 0xAABBGGRR: alpha ff, blue d4, green 78, red 00 — Windows blue.
        assert_eq!(accent_to_rgb(0xffd4_7800), [0x00, 0x78, 0xd4]);
    }

    #[test]
    fn tint_recolors_but_keeps_the_shape() {
        // Opaque white, half-covered edge, empty background.
        let rgba = [255, 255, 255, 255, 255, 255, 255, 128, 0, 0, 0, 0];
        let out = tint(&rgba, [0xb9, 0x1c, 0x1c]);
        assert_eq!(out[0..4], [0xb9, 0x1c, 0x1c, 255]);
        assert_eq!(out[4..8], [0xb9, 0x1c, 0x1c, 128]);
        assert_eq!(out[8..12], [0xb9, 0x1c, 0x1c, 0]);
    }
}
