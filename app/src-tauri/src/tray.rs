//! The Mac's `MenuBarExtra` mini-player as a tray icon: shown while
//! `showMenuBarPlayer` is on and something is playing; a click toggles a
//! small frameless player window beside the icon that hides when it loses
//! focus.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, PhysicalPosition, Rect, WebviewUrl, WebviewWindowBuilder};

use crate::state::AppState;

const TRAY_ID: &str = "flactastic";
const MINI: &str = "mini-player";

static PLAYING: AtomicBool = AtomicBool::new(false);
/// When the mini-player last hid on blur: a click on the icon that caused
/// that blur must not immediately reopen it.
static LAST_BLUR_HIDE: Mutex<Option<Instant>> = Mutex::new(None);

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open FLACtastic", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut b = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip("FLACtastic")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, e| match e.id().as_ref() {
            "open" => open_main(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, e| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, rect, .. } = e {
                toggle_mini(tray.app_handle(), rect);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        b = b.icon(icon.clone());
    }
    let tray = b.build(app)?;
    tray.set_visible(false)?;
    Ok(())
}

pub fn open_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
    if let Some(m) = app.get_webview_window(MINI) {
        let _ = m.hide();
    }
}

/// Player state callback: re-evaluate when play/pause flips.
pub fn on_player_state(app: &AppHandle, is_playing: bool) {
    if PLAYING.swap(is_playing, Ordering::SeqCst) != is_playing {
        refresh(app);
    }
}

/// `settings.showMenuBarPlayer && player.isPlaying`.
pub fn refresh(app: &AppHandle) {
    let enabled = app.try_state::<Arc<AppState>>().map_or(true, |s| s.settings.lock().show_menu_bar_player);
    let visible = enabled && PLAYING.load(Ordering::SeqCst);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_visible(visible);
    }
    if !visible {
        if let Some(m) = app.get_webview_window(MINI) {
            let _ = m.hide();
        }
    }
}

fn toggle_mini(app: &AppHandle, icon: Rect) {
    if let Some(m) = app.get_webview_window(MINI) {
        if m.is_visible().unwrap_or(false) {
            let _ = m.hide();
            return;
        }
        if LAST_BLUR_HIDE.lock().is_some_and(|t| t.elapsed() < Duration::from_millis(300)) {
            return;
        }
        place(&m, icon);
        let _ = m.show();
        let _ = m.set_focus();
        return;
    }
    let app2 = app.clone();
    // Window creation off the event-loop callback.
    std::thread::spawn(move || {
        let built = WebviewWindowBuilder::new(&app2, MINI, WebviewUrl::App("index.html?view=mini".into()))
            .title("FLACtastic")
            .inner_size(320.0, 212.0)
            .resizable(false)
            .decorations(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible(false)
            .focused(true)
            .background_color(tauri::window::Color(14, 14, 16, 255))
            .build();
        let Ok(m) = built else { return };
        let hide = m.clone();
        m.on_window_event(move |e| match e {
            tauri::WindowEvent::Focused(false) => {
                *LAST_BLUR_HIDE.lock() = Some(Instant::now());
                let _ = hide.hide();
            }
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                let _ = hide.hide();
            }
            _ => {}
        });
        place(&m, icon);
        let _ = m.show();
        let _ = m.set_focus();
    });
}

/// Centres the window on the icon, above it when the taskbar is at the
/// bottom (below otherwise), kept inside the monitor's work area.
fn place(w: &tauri::WebviewWindow, icon: Rect) {
    let scale = w.scale_factor().unwrap_or(1.0);
    let pos = icon.position.to_physical::<f64>(scale);
    let size = icon.size.to_physical::<f64>(scale);
    let Ok(own) = w.outer_size() else { return };
    let (ww, wh) = (f64::from(own.width), f64::from(own.height));
    let gap = 8.0 * scale;
    let mut x = pos.x + size.width / 2.0 - ww / 2.0;
    let mut y = pos.y - wh - gap;
    if let Ok(Some(mon)) = w.app_handle().monitor_from_point(pos.x, pos.y) {
        let area = mon.work_area();
        let (ax, ay) = (f64::from(area.position.x), f64::from(area.position.y));
        let (aw, ah) = (f64::from(area.size.width), f64::from(area.size.height));
        if y < ay {
            y = pos.y + size.height + gap;
        }
        x = x.clamp(ax + gap, (ax + aw - ww - gap).max(ax));
        y = y.clamp(ay + gap, (ay + ah - wh - gap).max(ay));
    }
    let _ = w.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

#[tauri::command]
pub fn mini_open_main(app: AppHandle) {
    open_main(&app);
}

#[tauri::command]
pub fn mini_quit(app: AppHandle) {
    app.exit(0);
}

/// Debug builds only: toggle the mini-player as if the icon at the bottom
/// right of the primary monitor were clicked.
#[tauri::command]
pub fn debug_toggle_mini(app: AppHandle) -> Result<(), String> {
    if !cfg!(debug_assertions) {
        return Err("unavailable".into());
    }
    let mon = app.primary_monitor().map_err(|e| e.to_string())?.ok_or("no monitor")?;
    let a = mon.work_area();
    let rect = Rect {
        position: PhysicalPosition::new(a.position.x + a.size.width as i32 - 120, a.position.y + a.size.height as i32).into(),
        size: tauri::PhysicalSize::new(24u32, 24u32).into(),
    };
    toggle_mini(&app, rect);
    Ok(())
}
