//! Menu bar: tray icon with lyric text (NSStatusItem title), right-click menu
//! (NSMenu), marquee scrolling for long lines. Driven by engine ticks on a
//! dedicated thread — independent of webview timers.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

use crate::audio::PlayMode;
use crate::dto::TickDto;
use crate::state::AppContext;

pub struct TrayUi {
    pub playpause: MenuItem<Wry>,
    pub modes: Vec<(PlayMode, CheckMenuItem<Wry>)>,
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let playpause = MenuItem::with_id(app, "playpause", "播放 / 暂停", true, None::<&str>)?;
    let prev = MenuItem::with_id(app, "prev", "上一首", true, None::<&str>)?;
    let next = MenuItem::with_id(app, "next", "下一首", true, None::<&str>)?;

    let m_order = CheckMenuItem::with_id(
        app,
        "mode_order",
        PlayMode::Order.label(),
        true,
        false,
        None::<&str>,
    )?;
    let m_shuffle = CheckMenuItem::with_id(
        app,
        "mode_shuffle",
        PlayMode::Shuffle.label(),
        true,
        false,
        None::<&str>,
    )?;
    let m_loop_all = CheckMenuItem::with_id(
        app,
        "mode_loop_all",
        PlayMode::LoopAll.label(),
        true,
        false,
        None::<&str>,
    )?;
    let m_loop_one = CheckMenuItem::with_id(
        app,
        "mode_loop_one",
        PlayMode::LoopOne.label(),
        true,
        false,
        None::<&str>,
    )?;
    let mode_menu = Submenu::with_id(app, "mode", "播放模式", true)?;
    mode_menu.append(&m_order)?;
    mode_menu.append(&m_shuffle)?;
    mode_menu.append(&m_loop_all)?;
    mode_menu.append(&m_loop_one)?;

    let mini = MenuItem::with_id(app, "mini", "迷你控制器", true, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", "显示 / 隐藏主窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 Sonora", true, None::<&str>)?;

    let menu = Menu::new(app)?;
    menu.append(&playpause)?;
    menu.append(&prev)?;
    menu.append(&next)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&mode_menu)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&mini)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&show)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&quit)?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip("Sonora")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::windows::toggle_mini(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    let tray = builder.build(app)?;
    let _ = tray.set_title(None::<&str>);

    app.manage(Mutex::new(TrayUi {
        playpause,
        modes: vec![
            (PlayMode::Order, m_order),
            (PlayMode::Shuffle, m_shuffle),
            (PlayMode::LoopAll, m_loop_all),
            (PlayMode::LoopOne, m_loop_one),
        ],
    }));

    spawn_ticker(app.clone());
    Ok(())
}

fn spawn_ticker(app: AppHandle) {
    let mut rx = {
        let ctx = app.state::<AppContext>();
        let rx = ctx.ticks.subscribe();
        drop(ctx);
        rx
    };
    std::thread::spawn(move || {
        let mut last_title = String::new();
        let mut shift: usize = 0;
        let mut last_shift = Instant::now();
        let mut last_playing: Option<bool> = None;
        let mut last_mode: Option<PlayMode> = None;
        loop {
            let Ok(tick) = rx.blocking_recv() else { return };
            let Some(tray) = app.tray_by_id("main") else {
                continue;
            };

            let (mb_on, mb_max) = {
                let ctx = app.state::<AppContext>();
                let s = ctx.settings.lock().unwrap();
                (s.menubar_lyrics, s.menubar_max_chars as usize)
            };
            let text = tray_text(&tick, mb_on, mb_max, &mut shift, &mut last_shift);
            if text != last_title {
                if text.is_empty() {
                    let _ = tray.set_title(None::<&str>);
                } else {
                    let _ = tray.set_title(Some(text.as_str()));
                }
                last_title = text;
            }

            if last_playing != Some(tick.playing) {
                if let Some(ui) = app.try_state::<Mutex<TrayUi>>() {
                    if let Ok(ui) = ui.lock() {
                        let _ =
                            ui.playpause
                                .set_text(if tick.playing { "暂停" } else { "播放" });
                    }
                }
                last_playing = Some(tick.playing);
            }
            let mode = PlayMode::parse(&tick.mode);
            if last_mode != Some(mode) {
                if let Some(ui) = app.try_state::<Mutex<TrayUi>>() {
                    if let Ok(ui) = ui.lock() {
                        for (m, item) in &ui.modes {
                            let _ = item.set_checked(*m == mode);
                        }
                    }
                }
                last_mode = Some(mode);
            }
        }
    });
}

fn tray_text(
    tick: &TickDto,
    enabled: bool,
    max_chars: usize,
    shift: &mut usize,
    last_shift: &mut Instant,
) -> String {
    if !enabled || !tick.playing {
        return String::new();
    }
    let base = tick.lyric_line.clone().or_else(|| {
        tick.track
            .as_ref()
            .map(|t| format!("{} · {}", t.title, t.artist))
    });
    let Some(base) = base else {
        return String::new();
    };
    let chars: Vec<char> = base.chars().collect();
    if chars.len() <= max_chars {
        *shift = 0;
        return base;
    }
    // marquee: advance one char every 500ms
    if last_shift.elapsed() >= Duration::from_millis(500) {
        *shift = (*shift + 1) % chars.len();
        *last_shift = Instant::now();
    }
    let mut out = String::new();
    for k in 0..max_chars {
        out.push(chars[(*shift + k) % chars.len()]);
    }
    out
}
