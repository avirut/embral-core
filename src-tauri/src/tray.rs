use anyhow::Result;
use tauri::{
    image::Image,
    menu::{MenuBuilder, MenuItemBuilder, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Manager,
};

const TRAY_ID: &str = "main";
const IDLE_ICON_BYTES: &[u8] = include_bytes!("../icons/tray-idle.png");
const RECORDING_ICON_BYTES: &[u8] = include_bytes!("../icons/tray-recording.png");

pub fn create_tray(app: &App) -> Result<()> {
    let show = MenuItemBuilder::with_id("show", "Show Window").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;
    let sep = PredefinedMenuItem::separator(app)?;

    let menu = MenuBuilder::new(app).items(&[&show, &sep, &quit]).build()?;

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(Image::from_bytes(IDLE_ICON_BYTES)?)
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

    Ok(())
}

pub fn update_tray_recording_state(app: &AppHandle, recording: bool) -> Result<()> {
    let bytes = if recording {
        RECORDING_ICON_BYTES
    } else {
        IDLE_ICON_BYTES
    };
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_icon(Some(Image::from_bytes(bytes)?))?;
    }
    Ok(())
}
