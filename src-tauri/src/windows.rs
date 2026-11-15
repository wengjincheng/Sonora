//! Main window visibility and the tray mini-controller popover.

use tauri::{
    AppHandle, Manager, PhysicalPosition, Position, Size, WebviewUrl, WebviewWindowBuilder,
};

pub fn toggle_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
        } else {
            let _ = w.show();
            let _ = w.set_focus();
        }
    }
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

// ---------- mini controller (tray popover) ----------

const MINI_W: f64 = 330.0;
const MINI_H: f64 = 470.0;

pub fn toggle_mini(app: &AppHandle) {
    match app.get_webview_window("mini") {
        Some(w) => {
            if w.is_visible().unwrap_or(false) {
                let _ = w.hide();
            } else {
                position_under_tray(app, &w);
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
        None => {
            let built = WebviewWindowBuilder::new(app, "mini", WebviewUrl::default())
                .title("Sonora")
                .inner_size(MINI_W, MINI_H)
                .decorations(false)
                .transparent(true)
                .resizable(false)
                .always_on_top(true)
                .skip_taskbar(true)
                .shadow(false)
                .visible(false)
                .build();
            if let Ok(w) = built {
                #[cfg(target_os = "macos")]
                window_vibrancy::apply_vibrancy(
                    &w,
                    window_vibrancy::NSVisualEffectMaterial::HudWindow,
                    Some(window_vibrancy::NSVisualEffectState::Active),
                    None,
                )
                .ok();
                position_under_tray(app, &w);
                let _ = w.show();
                let _ = w.set_focus();
            }
        }
    }
}

fn position_under_tray(app: &AppHandle, w: &tauri::WebviewWindow) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    let Ok(Some(rect)) = tray.rect() else { return };
    let scale = w.scale_factor().unwrap_or(2.0);
    let (tx, ty, tw, th): (f64, f64, f64, f64) = match (rect.position, rect.size) {
        (Position::Physical(p), Size::Physical(s)) => {
            (p.x as f64, p.y as f64, s.width as f64, s.height as f64)
        }
        (Position::Logical(p), Size::Logical(s)) => {
            (p.x * scale, p.y * scale, s.width * scale, s.height * scale)
        }
        _ => return,
    };
    let win_w = MINI_W * scale;
    let x = (tx + tw / 2.0 - win_w / 2.0).max(8.0);
    let y = ty + th + 8.0;
    let _ = w.set_position(PhysicalPosition::new(x as i32, y as i32));
}
