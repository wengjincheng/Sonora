//! OS media session via souvlaki: keyboard media keys (F7–F9), Control Center
//! Now Playing. Subscribes to engine ticks for metadata/progress updates.

use std::sync::Mutex;
use std::time::Duration;

use souvlaki::{
    MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition, PlatformConfig,
    SeekDirection,
};
use tauri::{AppHandle, Manager};

use crate::audio::EngineCmd;
use crate::state::AppContext;

pub struct MediaSession(Mutex<MediaControls>);

pub fn setup(app: &AppHandle) {
    let config = PlatformConfig {
        dbus_name: "sonora",
        display_name: "Sonora",
        hwnd: None,
    };
    let mut controls = match MediaControls::new(config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[media] init failed: {e:?}");
            return;
        }
    };
    {
        let app2 = app.clone();
        if let Err(e) = controls.attach(move |ev| handle_event(&app2, ev)) {
            eprintln!("[media] attach failed: {e:?}");
            return;
        }
    }
    app.manage(MediaSession(Mutex::new(controls)));

    let mut rx = {
        let ctx = app.state::<AppContext>();
        let rx = ctx.ticks.subscribe();
        drop(ctx);
        rx
    };
    let app2 = app.clone();
    std::thread::spawn(move || {
        let mut last_track: Option<(i64, Option<String>)> = None;
        loop {
            let Ok(tick) = rx.blocking_recv() else { return };
            let Some(session) = app2.try_state::<MediaSession>() else {
                return;
            };
            let Ok(mut controls) = session.0.lock() else {
                return;
            };

            if tick
                .track
                .as_ref()
                .map(|t| (t.id, t.cover_version.as_deref()))
                != last_track
                    .as_ref()
                    .map(|(id, version)| (*id, version.as_deref()))
            {
                last_track = tick.track.as_ref().map(|t| (t.id, t.cover_version.clone()));
                if let Some(t) = &tick.track {
                    // souvlaki expects a URL, not a filesystem path. An NSURL
                    // without a scheme can produce a nil NSImage and abort the
                    // whole process. Keep paths out of this API entirely.
                    let cover_url = media_cover_url(t.cover.as_deref());
                    let _ = controls.set_metadata(MediaMetadata {
                        title: Some(t.title.as_str()),
                        artist: Some(t.artist.as_str()),
                        album: Some(t.album.as_str()),
                        duration: Some(Duration::from_secs_f64(t.duration.max(0.0))),
                        cover_url: cover_url.as_deref(),
                    });
                }
            }
            let progress = Some(MediaPosition(Duration::from_secs_f64(
                tick.position.max(0.0),
            )));
            let playback = if tick.playing {
                MediaPlayback::Playing { progress }
            } else {
                MediaPlayback::Paused { progress }
            };
            let _ = controls.set_playback(playback);
        }
    });
}

fn media_cover_url(path: Option<&str>) -> Option<String> {
    let path = std::path::Path::new(path?);
    if !path.is_absolute() {
        return None;
    }
    // Do not hand a missing/corrupt legacy cache to souvlaki's unchecked
    // NSImage decoder. New publications are atomic, so this cannot read a
    // half-written image from our image pipeline.
    image::open(path).ok()?;
    reqwest::Url::from_file_path(path)
        .ok()
        .map(|url| url.to_string())
}

fn handle_event(app: &AppHandle, ev: MediaControlEvent) {
    let tx = {
        let ctx = app.state::<AppContext>();
        let tx = ctx.engine.clone();
        drop(ctx);
        tx
    };
    match ev {
        MediaControlEvent::Play => {
            let _ = tx.send(EngineCmd::Play);
        }
        MediaControlEvent::Pause | MediaControlEvent::Stop => {
            let _ = tx.send(EngineCmd::Pause);
        }
        MediaControlEvent::Toggle => {
            let _ = tx.send(EngineCmd::Toggle);
        }
        MediaControlEvent::Next => {
            let _ = tx.send(EngineCmd::Next);
        }
        MediaControlEvent::Previous => {
            let _ = tx.send(EngineCmd::Prev);
        }
        MediaControlEvent::Seek(SeekDirection::Forward) => {
            let _ = tx.send(EngineCmd::Nudge(10.0));
        }
        MediaControlEvent::Seek(SeekDirection::Backward) => {
            let _ = tx.send(EngineCmd::Nudge(-10.0));
        }
        MediaControlEvent::SetPosition(MediaPosition(d)) => {
            let _ = tx.send(EngineCmd::Seek(d.as_secs_f64()));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_paths_are_encoded_as_file_urls_and_invalid_images_are_omitted() {
        let path = std::env::temp_dir().join(format!(
            "sonora-media-{}-{} 中文 #.png",
            std::process::id(),
            crate::state::now_ms()
        ));
        image::RgbImage::new(2, 2).save(&path).unwrap();
        let url = media_cover_url(path.to_str()).unwrap();
        assert!(url.starts_with("file:///"));
        assert!(url.contains("%20"));
        assert!(url.contains("%23"));
        assert_eq!(
            reqwest::Url::parse(&url).unwrap().to_file_path().unwrap(),
            path
        );
        std::fs::write(&path, b"not an image").unwrap();
        assert!(media_cover_url(path.to_str()).is_none());
        std::fs::remove_file(&path).unwrap();
        assert!(media_cover_url(path.to_str()).is_none());
        assert!(media_cover_url(Some("relative.jpg")).is_none());
        assert!(media_cover_url(None).is_none());
    }
}
