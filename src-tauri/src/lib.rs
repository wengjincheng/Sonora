mod audio;
mod commands;
mod db;
mod dto;
mod images;
mod media;
mod metadata;
mod scanner;
mod state;
mod tray;
mod windows;

use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc, Mutex};

use tauri::Manager;
use tokio::sync::broadcast;

use crate::state::AppContext;

/// menubar-only mode: hide Dock icon (macOS ActivationPolicy::Accessory)
#[cfg(target_os = "macos")]
pub fn apply_menubar_only(app: &tauri::AppHandle, on: bool) {
    let policy = if on {
        tauri::ActivationPolicy::Accessory
    } else {
        tauri::ActivationPolicy::Regular
    };
    if let Err(e) = app.set_activation_policy(policy) {
        eprintln!("[app] set_activation_policy failed: {e:?}");
    }
}

#[cfg(not(target_os = "macos"))]
pub fn apply_menubar_only(_app: &tauri::AppHandle, _on: bool) {}

pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .setup(|app| {
            let handle = app.handle().clone();

            let data_dir = handle.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let db_path = data_dir.join("library.db");
            let covers_dir = data_dir.join("covers");
            std::fs::create_dir_all(&covers_dir)?;
            let conn = db::open(&db_path)?;
            let settings = db::load_settings(&conn);

            // macOS: vibrancy behind the (transparent) main window
            if let Some(win) = handle.get_webview_window("main") {
                #[cfg(target_os = "macos")]
                window_vibrancy::apply_vibrancy(
                    &win,
                    window_vibrancy::NSVisualEffectMaterial::UnderWindowBackground,
                    Some(window_vibrancy::NSVisualEffectState::Active),
                    None,
                )
                .ok();
                let _ = win;
            }

            if settings.menubar_only {
                apply_menubar_only(&handle, true);
            }

            let (engine_tx, engine_rx) = mpsc::channel();
            let ticks = broadcast::channel::<dto::TickDto>(64).0;
            app.manage(AppContext {
                db: Mutex::new(conn),
                db_path,
                covers_dir,
                engine: engine_tx.clone(),
                ticks: ticks.clone(),
                settings: Mutex::new(settings),
                scanning: Arc::new(AtomicBool::new(false)),
                metadata_fetches: Mutex::new(std::collections::HashMap::new()),
            });

            audio::spawn_engine(handle.clone(), engine_rx, engine_tx, ticks);
            tray::setup(&handle)?;
            media::setup(&handle);
            scanner::spawn(handle.clone());

            // tray / app menu actions
            handle.on_menu_event(|app, event| {
                let tx = {
                    let ctx = app.state::<AppContext>();
                    let tx = ctx.engine.clone();
                    drop(ctx);
                    tx
                };
                match event.id().as_ref() {
                    "playpause" => {
                        let _ = tx.send(audio::EngineCmd::Toggle);
                    }
                    "prev" => {
                        let _ = tx.send(audio::EngineCmd::Prev);
                    }
                    "next" => {
                        let _ = tx.send(audio::EngineCmd::Next);
                    }
                    "mode_order" => {
                        let _ = tx.send(audio::EngineCmd::SetMode(audio::PlayMode::Order));
                    }
                    "mode_shuffle" => {
                        let _ = tx.send(audio::EngineCmd::SetMode(audio::PlayMode::Shuffle));
                    }
                    "mode_loop_all" => {
                        let _ = tx.send(audio::EngineCmd::SetMode(audio::PlayMode::LoopAll));
                    }
                    "mode_loop_one" => {
                        let _ = tx.send(audio::EngineCmd::SetMode(audio::PlayMode::LoopOne));
                    }
                    "mini" => windows::toggle_mini(app),
                    "show" => windows::toggle_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                }
            });

            Ok(())
        })
        .on_window_event(|window, event| {
            let label = window.label();
            match (label, event) {
                // macOS convention: closing the window keeps the app (and
                // playback) alive in the menu bar; Cmd+Q quits.
                ("main", tauri::WindowEvent::CloseRequested { api, .. }) => {
                    api.prevent_close();
                    let _ = window.hide();
                }
                // mini popover hides when it loses focus
                ("mini", tauri::WindowEvent::Focused(false)) => {
                    let _ = window.hide();
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::library_state,
            commands::list_tracks,
            commands::list_albums,
            commands::play_tracks,
            commands::jump_to,
            commands::toggle_play,
            commands::next,
            commands::prev,
            commands::seek,
            commands::set_volume,
            commands::set_mode,
            commands::cycle_mode,
            commands::get_lyrics,
            commands::update_track_meta,
            commands::refetch_track_meta,
            commands::clear_track_cover,
            commands::get_settings,
            commands::set_setting,
            commands::add_music_dir,
            commands::remove_music_dir,
            commands::rescan,
            commands::clear_meta_cache,
            commands::meta_cache_count,
            commands::refresh_covers,
            commands::show_main,
            commands::import_paths,
        ])
        .build(tauri::generate_context!())
        .expect("error while building Sonora");

    app.run(|handle, event| {
        // macOS: clicking the Dock icon reopens the main window
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = event {
            windows::show_main_window(handle);
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (handle, &event);
        }
    });
}
