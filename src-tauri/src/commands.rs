//! Tauri IPC commands — the typed bridge between the React frontend and the
//! Rust core. Field names arrive camelCase from JS (Tauri convention).

use std::path::Path;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::audio::{EngineCmd, PlayMode};
use crate::db;
use crate::dto::{AlbumDto, LibraryStateDto, LyricsDto, RefetchDto, SettingsDto, TrackDto};
use crate::images;
use crate::metadata::{cache_key, cached_lyrics, parse_by_provider, parse_lrc, FetchOpts};
use crate::scanner;
use crate::state::AppContext;

#[tauri::command]
pub fn library_state(state: State<AppContext>) -> LibraryStateDto {
    let (tracks, albums) = {
        let conn = state.db.lock().unwrap();
        db::counts(&conn)
    };
    LibraryStateDto {
        tracks,
        albums,
        scanning: state.scanning.load(std::sync::atomic::Ordering::SeqCst),
        music_dirs: state.settings.lock().unwrap().music_dirs.clone(),
    }
}

#[tauri::command]
pub fn list_tracks(
    q: Option<String>,
    album_key: Option<String>,
    state: State<AppContext>,
) -> Vec<TrackDto> {
    let conn = state.db.lock().unwrap();
    let q_trimmed = q.unwrap_or_default().trim().to_string();
    let has_q = !q_trimmed.is_empty();
    let album = album_key.unwrap_or_default();
    let has_album = !album.is_empty();
    let like = format!("%{q_trimmed}%");

    let (sql, params): (&str, Vec<&dyn rusqlite::ToSql>) = match (has_q, has_album) {
        (false, false) => (
            "SELECT id, title, artist, album, duration, track_no, (art_path IS NOT NULL) FROM tracks ORDER BY artist, album, track_no, title",
            vec![],
        ),
        (true, false) => (
            "SELECT id, title, artist, album, duration, track_no, (art_path IS NOT NULL) FROM tracks WHERE (title LIKE ?1 OR artist LIKE ?1 OR album LIKE ?1) ORDER BY artist, album, track_no, title",
            vec![&like],
        ),
        (false, true) => (
            "SELECT id, title, artist, album, duration, track_no, (art_path IS NOT NULL) FROM tracks WHERE album_key = ?1 ORDER BY track_no, title",
            vec![&album],
        ),
        (true, true) => (
            "SELECT id, title, artist, album, duration, track_no, (art_path IS NOT NULL) FROM tracks WHERE album_key = ?1 AND (title LIKE ?2 OR artist LIKE ?2 OR album LIKE ?2) ORDER BY track_no, title",
            vec![&album, &like],
        ),
    };

    conn.prepare(sql)
        .and_then(|mut stmt| {
            let rows = stmt.query_map(params.as_slice(), map_track_row)?;
            Ok(rows.filter_map(|r| r.ok()).collect())
        })
        .unwrap_or_default()
}

fn map_track_row(r: &rusqlite::Row) -> rusqlite::Result<TrackDto> {
    Ok(TrackDto {
        id: r.get(0)?,
        title: r.get(1)?,
        artist: r.get(2)?,
        album: r.get(3)?,
        duration: r.get(4)?,
        track_no: r.get(5)?,
        has_track_cover: r.get::<_, Option<bool>>(6)?.unwrap_or(false),
    })
}

#[tauri::command]
pub fn list_albums(state: State<AppContext>) -> Vec<AlbumDto> {
    let conn = state.db.lock().unwrap();
    let mut out = Vec::new();
    let ok = conn
        .prepare(
            "SELECT a.key, a.name, a.artist, COUNT(t.id), a.art_path
             FROM albums a LEFT JOIN tracks t ON t.album_key = a.key
             GROUP BY a.key ORDER BY a.artist, a.name",
        )
        .and_then(|mut stmt| {
            let rows = stmt.query_map([], |r| {
                let cover: Option<String> = r.get(4)?;
                let blur = cover
                    .as_deref()
                    .map(|c| images::blur_of(Path::new(c)).to_string_lossy().to_string());
                Ok(AlbumDto {
                    key: r.get(0)?,
                    name: r.get(1)?,
                    artist: r.get(2)?,
                    track_count: r.get(3)?,
                    cover_version: cover
                        .as_deref()
                        .and_then(|p| images::file_version(Path::new(p))),
                    blur_version: blur
                        .as_deref()
                        .and_then(|p| images::file_version(Path::new(p))),
                    cover,
                    blur,
                })
            });
            for row in rows?.flatten() {
                out.push(row);
            }
            Ok(())
        })
        .is_ok();
    let _ = ok;
    out
}

// ---------- player ----------

fn send(state: &State<AppContext>, cmd: EngineCmd) {
    let _ = state.engine.send(cmd);
}

#[tauri::command]
pub fn play_tracks(ids: Vec<i64>, start: usize, state: State<AppContext>) {
    send(&state, EngineCmd::PlayQueue { ids, start });
}

#[tauri::command]
pub fn jump_to(index: usize, state: State<AppContext>) {
    send(&state, EngineCmd::Jump(index));
}

#[tauri::command]
pub fn toggle_play(state: State<AppContext>) {
    send(&state, EngineCmd::Toggle);
}

#[tauri::command]
pub fn next(state: State<AppContext>) {
    send(&state, EngineCmd::Next);
}

#[tauri::command]
pub fn prev(state: State<AppContext>) {
    send(&state, EngineCmd::Prev);
}

#[tauri::command]
pub fn seek(position: f64, state: State<AppContext>) {
    send(&state, EngineCmd::Seek(position));
}

#[tauri::command]
pub fn set_volume(volume: f64, state: State<AppContext>) {
    send(&state, EngineCmd::SetVolume(volume));
}

#[tauri::command]
pub fn set_mode(mode: String, state: State<AppContext>) {
    send(&state, EngineCmd::SetMode(PlayMode::parse(&mode)));
}

#[tauri::command]
pub fn cycle_mode(state: State<AppContext>) {
    send(&state, EngineCmd::CycleMode);
}

// ---------- lyrics ----------

#[tauri::command]
pub fn get_lyrics(track_id: i64, state: State<AppContext>) -> Option<LyricsDto> {
    let conn = state.db.lock().unwrap();
    let row: Option<(String, String, String)> = conn
        .query_row(
            "SELECT path, title, artist FROM tracks WHERE id = ?1",
            [track_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok();
    let (path, title, artist) = row?;

    // 1. sibling .lrc file
    let p = Path::new(&path);
    for lrc in [p.with_extension("lrc"), p.with_extension("LRC")] {
        if let Ok(text) = std::fs::read_to_string(&lrc) {
            let lines = parse_lrc(&text);
            if !lines.is_empty() {
                return Some(LyricsDto {
                    track_id,
                    lines,
                    source: "file".into(),
                });
            }
        }
    }
    // 2. cache
    let key = cache_key(&title, &artist);
    if let Some((text, provider)) = cached_lyrics(&conn, &key) {
        let lines = parse_by_provider(&text, &provider);
        if !lines.is_empty() {
            return Some(LyricsDto {
                track_id,
                lines,
                source: provider,
            });
        }
    }
    None
}

// ---------- settings ----------

// ---------- user-edited metadata / manual refetch ----------

#[tauri::command]
pub fn update_track_meta(
    track_id: i64,
    title: String,
    artist: String,
    album: String,
    app: AppHandle,
    state: State<AppContext>,
) -> Result<(), String> {
    let title = title.trim().to_string();
    let artist = artist.trim().to_string();
    let album = album.trim().to_string();
    if title.is_empty() {
        return Err("标题不能为空".into());
    }
    let fetch_slot = state.metadata_fetch_slot(track_id);
    let update = {
        // Same lock order as fetch publication: slot, then the short DB write.
        // Invalidate even an unchanged identity; an earlier auto request must
        // not publish after this explicit edit, even without a follow-up fetch.
        let mut generation = fetch_slot.lock().unwrap();
        let conn = state.db.lock().unwrap();
        let update = db::apply_track_metadata(&conn, track_id, &title, &artist, &album)
            .map_err(|e| format!("保存失败: {e}"))?;
        if update.is_some() {
            *generation = generation.wrapping_add(1);
        }
        update
    };
    let Some(update) = update else {
        return Err("曲目不存在".into());
    };
    // reflect the new metadata in the player / tray / menubar right away
    let _ = state.engine.send(EngineCmd::ReloadCurrent);
    if update.lyrics_key_changed {
        // stale lyrics for the old title must not keep showing
        let _ = state.engine.send(EngineCmd::SetLyrics {
            track_id,
            lines: Vec::new(),
            source: String::new(),
        });
    }
    if update.album_key_changed {
        // the track moved to an album that has no cover yet
        crate::metadata::spawn_cover_warmup(app.clone());
    }
    crate::scanner::emit_library_state(&app);
    Ok(())
}

/// Re-fetch lyrics (and optionally a per-track cover) for one song. Nothing is
/// cleared up-front, so a failed fetch keeps whatever the track already had.
#[tauri::command]
pub async fn refetch_track_meta(
    track_id: i64,
    want_lyrics: bool,
    track_cover: bool,
    app: AppHandle,
) -> Result<RefetchDto, String> {
    let row: Option<(String, String, String, f64, String)> = {
        let state = app.state::<AppContext>();
        let conn = state.db.lock().unwrap();
        conn.query_row(
            "SELECT title, artist, album, duration, album_key FROM tracks WHERE id = ?1",
            [track_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .ok()
    };
    let Some((title, artist, album, duration, album_key)) = row else {
        return Err("曲目不存在".into());
    };

    let mut dto = RefetchDto::default();
    if !want_lyrics && !track_cover {
        return Ok(dto);
    }

    let opts = FetchOpts {
        want_lyrics,
        force_lyrics: want_lyrics,
        track_cover,
    };
    let result = crate::metadata::fetch_for_track(
        &app,
        track_id,
        &title,
        &artist,
        Some(&album),
        duration,
        Some(&album_key),
        opts,
    )
    .await;

    let engine = {
        let state = app.state::<AppContext>();
        state.engine.clone()
    };
    if let Some(r) = result {
        if let Some((lines, source)) = r.lyrics {
            dto.lyric_lines = lines.len();
            dto.lyrics_source = Some(source);
            // Already published by fetch_for_track, before cover download.
        }
        if r.cover_saved {
            dto.cover_saved = true;
            let _ = engine.send(EngineCmd::ReloadCurrent);
            crate::scanner::emit_library_state(&app);
        }
    }
    Ok(dto)
}

/// Drop the per-track cover so the album cover applies again.
#[tauri::command]
pub fn clear_track_cover(
    track_id: i64,
    app: AppHandle,
    state: State<AppContext>,
) -> Result<(), String> {
    {
        let conn = state.db.lock().unwrap();
        db::clear_track_cover(&conn, track_id).map_err(|e| e.to_string())?;
    }
    let _ = state.engine.send(EngineCmd::ReloadCurrent);
    crate::scanner::emit_library_state(&app);
    Ok(())
}

#[tauri::command]
pub fn get_settings(state: State<AppContext>) -> SettingsDto {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
pub fn set_setting(
    key: String,
    value: String,
    app: AppHandle,
    state: State<AppContext>,
) -> Result<SettingsDto, String> {
    let snapshot = {
        // Match scanner's db -> settings lock order. Keep both until persistence
        // succeeds, so concurrent writes cannot save an older full snapshot.
        let conn = state.db.lock().unwrap();
        let mut settings = state.settings.lock().unwrap();
        let mut next = settings.clone();
        match key.as_str() {
            "menubar_lyrics" => next.menubar_lyrics = value == "true",
            "menubar_max_chars" => {
                next.menubar_max_chars = value
                    .parse::<u32>()
                    .map_err(|_| "Invalid menubar_max_chars".to_string())?
                    .clamp(8, 60);
            }
            "lyrics_scale" => {
                next.lyrics_scale = value
                    .parse::<u32>()
                    .map_err(|_| "Invalid lyrics_scale".to_string())?
                    .clamp(80, 300);
            }
            "online_metadata" => next.online_metadata = value == "true",
            "menubar_only" => next.menubar_only = value == "true",
            _ => return Err(format!("Unknown setting: {key}")),
        }
        db::save_settings(&conn, &next).map_err(|e| e.to_string())?;
        *settings = next.clone();
        next
    };
    if key == "menubar_only" {
        crate::apply_menubar_only(&app, snapshot.menubar_only);
    }
    // A missed event must not turn a successful durable save into a UI failure:
    // the initiating WebView also receives the authoritative snapshot directly.
    if let Err(error) = app.emit("settings://updated", &snapshot) {
        eprintln!("Failed to broadcast saved settings: {error}");
    }
    Ok(snapshot)
}

#[tauri::command]
pub fn add_music_dir(path: String, app: AppHandle, state: State<AppContext>) {
    {
        let conn = state.db.lock().unwrap();
        let mut s = state.settings.lock().unwrap();
        let mut next = s.clone();
        if !next.music_dirs.contains(&path) {
            next.music_dirs.push(path);
        }
        if let Err(e) = db::save_settings(&conn, &next) {
            eprintln!("[settings] add music directory failed: {e}");
            return;
        }
        *s = next;
    }
    scanner::spawn(app);
}

#[tauri::command]
pub fn remove_music_dir(path: String, app: AppHandle, state: State<AppContext>) {
    {
        let conn = state.db.lock().unwrap();
        let mut s = state.settings.lock().unwrap();
        let mut next = s.clone();
        next.music_dirs.retain(|d| d != &path);
        if let Err(e) = db::save_settings(&conn, &next) {
            eprintln!("[settings] remove music directory failed: {e}");
            return;
        }
        *s = next;
    }
    scanner::spawn(app);
}

#[tauri::command]
pub fn rescan(app: AppHandle) {
    scanner::spawn(app);
}

#[tauri::command]
pub fn clear_meta_cache(state: State<AppContext>) {
    let conn = state.db.lock().unwrap();
    let _ = conn.execute("DELETE FROM meta_cache", []);
}

/// Drop all album art (including bad online matches) and force a full rescan;
/// embedded art is re-extracted by the scanner, the rest re-fetched by the
/// warm-up with the improved matching.
#[tauri::command]
pub fn refresh_covers(app: AppHandle, state: State<AppContext>) {
    {
        let conn = state.db.lock().unwrap();
        let _ = conn.execute("UPDATE albums SET art_path = NULL", []);
        let _ = conn.execute("UPDATE tracks SET mtime = 0", []);
    }
    crate::scanner::spawn(app);
}

#[tauri::command]
pub fn meta_cache_count(state: State<AppContext>) -> i64 {
    let conn = state.db.lock().unwrap();
    conn.query_row("SELECT COUNT(*) FROM meta_cache", [], |r| r.get(0))
        .unwrap_or(0)
}

// ---------- windows ----------

#[tauri::command]
pub fn show_main(app: AppHandle) {
    crate::windows::show_main_window(&app);
}

// ---------- drag & drop import ----------

#[tauri::command]
pub fn import_paths(paths: Vec<String>, app: AppHandle, state: State<AppContext>) -> String {
    let mut added = 0usize;
    {
        let conn = state.db.lock().unwrap();
        let mut current = state.settings.lock().unwrap();
        let mut s = current.clone();
        for p in &paths {
            let pb = std::path::PathBuf::from(p);
            let target = if pb.is_dir() {
                Some(pb)
            } else {
                let ext = pb
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase());
                match ext.as_deref() {
                    Some(e) if crate::scanner::AUDIO_EXTS.contains(&e) => {
                        pb.parent().map(|x| x.to_path_buf())
                    }
                    _ => None,
                }
            };
            if let Some(dir) = target {
                let ds = dir.to_string_lossy().to_string();
                if !ds.is_empty() && !s.music_dirs.contains(&ds) {
                    s.music_dirs.push(ds);
                    added += 1;
                }
            }
        }
        if added > 0 {
            if let Err(e) = db::save_settings(&conn, &s) {
                return format!("保存音乐文件夹失败: {e}");
            }
            *current = s;
        }
    }
    if added > 0 {
        crate::scanner::spawn(app);
        format!("已添加 {added} 个音乐文件夹，开始扫描")
    } else {
        "没有可导入的音乐路径".to_string()
    }
}
