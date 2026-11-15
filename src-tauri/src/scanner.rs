//! Library scanner: walks configured music dirs, reads tags with lofty,
//! upserts into SQLite, extracts embedded album art. Runs on its own thread.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::SystemTime;

use lofty::prelude::*;
use lofty::tag::ItemKey;
use rusqlite::{params, Connection};
use tauri::{AppHandle, Emitter, Manager};
use walkdir::WalkDir;

use crate::db;
use crate::dto::LibraryStateDto;
use crate::images;
use crate::metadata::normalize;
use crate::state::AppContext;

pub const AUDIO_EXTS: &[&str] = &[
    "mp3", "flac", "wav", "m4a", "mp4", "ogg", "oga", "aiff", "aif", "opus",
];

pub fn emit_library_state(app: &AppHandle) {
    let ctx = app.state::<AppContext>();
    let state = {
        let conn = ctx.db.lock().unwrap();
        let (tracks, albums) = db::counts(&conn);
        LibraryStateDto {
            tracks,
            albums,
            scanning: ctx.scanning.load(Ordering::SeqCst),
            music_dirs: ctx.settings.lock().unwrap().music_dirs.clone(),
        }
    };
    drop(ctx);
    let _ = app.emit("library://updated", state);
}

pub fn spawn(app: AppHandle) {
    let ctx = app.state::<AppContext>();
    if ctx.scanning.swap(true, Ordering::SeqCst) {
        return; // already scanning
    }
    let dirs = ctx.settings.lock().unwrap().music_dirs.clone();
    let db_path = ctx.db_path.clone();
    let covers = ctx.covers_dir.clone();
    let scanning = ctx.scanning.clone();
    drop(ctx);
    std::thread::spawn(move || {
        let conn = match db::open(&db_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("[scanner] cannot open db: {e}");
                scanning.store(false, Ordering::SeqCst);
                return;
            }
        };
        run_scan(&app, &conn, &dirs, &covers);
        scanning.store(false, Ordering::SeqCst);
        // Embedded art can replace an existing cover at the same path. Refresh
        // the playing DTO's file versions too, not only the album grid.
        let _ = app
            .state::<AppContext>()
            .engine
            .send(crate::audio::EngineCmd::ReloadCurrent);
        emit_library_state(&app);
        // folia-style: after scanning, backfill missing album covers in the
        // background (rate-limited; respects the online_metadata setting)
        crate::metadata::spawn_cover_warmup(app.clone());
    });
}

fn run_scan(app: &AppHandle, conn: &Connection, dirs: &[String], covers: &Path) {
    // existing rows: path -> (id, size, mtime)
    let mut existing: HashMap<PathBuf, (i64, i64, i64)> = HashMap::new();
    if let Ok(mut stmt) = conn.prepare("SELECT id, size, mtime, path FROM tracks") {
        let rows = stmt.query_map([], |r| {
            let path: String = r.get(3)?;
            Ok((
                PathBuf::from(path),
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
            ))
        });
        if let Ok(rows) = rows {
            for row in rows.flatten() {
                existing.insert(row.0, (row.1, row.2, row.3));
            }
        }
    }

    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut processed: usize = 0;
    for dir in dirs {
        for entry in WalkDir::new(dir).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let Some(ext) = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
            else {
                continue;
            };
            if !AUDIO_EXTS.contains(&ext.as_str()) {
                continue;
            }
            seen.insert(path.to_path_buf());
            let Ok(meta) = fs::metadata(path) else {
                continue;
            };
            let size = meta.len() as i64;
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let unchanged = existing
                .get(path)
                .map(|(_, s, m)| *s == size && *m == mtime)
                .unwrap_or(false);
            if unchanged {
                continue;
            }
            upsert_track(conn, covers, path, size, mtime, &ext);
            processed += 1;
            if processed % 300 == 0 {
                emit_library_state(app);
            }
        }
    }

    // delete rows whose files disappeared
    let missing: Vec<i64> = existing
        .iter()
        .filter(|(p, _)| !seen.contains(*p))
        .map(|(_, (id, _, _))| *id)
        .collect();
    for id in missing {
        let _ = conn.execute("DELETE FROM tracks WHERE id = ?1", params![id]);
    }
    let _ = conn.execute(
        "DELETE FROM albums WHERE key NOT IN (SELECT DISTINCT album_key FROM tracks)",
        [],
    );
    db::delete_orphan_overrides(conn);
    if processed > 0 {
        emit_library_state(app);
    }
}

fn upsert_track(conn: &Connection, covers: &Path, path: &Path, size: i64, mtime: i64, ext: &str) {
    let (title, artist, album, album_artist, track_no, disc_no, duration, art) =
        match lofty::read_from_path(path) {
            Ok(tagged) => {
                let duration = tagged.properties().duration().as_secs_f64();
                let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
                let mut title: Option<String> = None;
                let mut artist: Option<String> = None;
                let mut album: Option<String> = None;
                let mut album_artist: Option<String> = None;
                let mut track_no: Option<i64> = None;
                let mut disc_no: Option<i64> = None;
                let mut art: Option<Vec<u8>> = None;
                if let Some(t) = tag {
                    title = t.title().map(|s| s.trim().to_string());
                    artist = t.artist().map(|s| s.trim().to_string());
                    album = t.album().map(|s| s.trim().to_string());
                    album_artist = t
                        .get_string(&ItemKey::AlbumArtist)
                        .map(|s| s.trim().to_string());
                    track_no = t.track().map(|v| v as i64);
                    disc_no = t.disk().map(|v| v as i64);
                    if let Some(pic) = t.pictures().first() {
                        art = Some(pic.data().to_vec());
                    }
                }
                (
                    title,
                    artist,
                    album,
                    album_artist,
                    track_no,
                    disc_no,
                    duration,
                    art,
                )
            }
            Err(_) => (None, None, None, None, None, None, 0.0, None),
        };

    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
        .to_string();
    let title = title.filter(|s| !s.is_empty()).unwrap_or(stem);
    let artist = artist
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "未知艺术家".to_string());
    let album = album
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "未知专辑".to_string());

    // user edits win over tags — a rescan must not undo them
    let (title, artist, album) = match db::load_override(conn, &path.to_string_lossy()) {
        Some((ot, oa, ob)) => (
            if ot.is_empty() { title } else { ot },
            if oa.is_empty() { artist } else { oa },
            if ob.is_empty() { album } else { ob },
        ),
        None => (title, artist, album),
    };

    let album_artist = album_artist
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| artist.clone());
    let album_key = format!("{}|{}", normalize(&album), normalize(&album_artist));

    // album row + embedded art
    let existing_art: Option<String> = conn
        .query_row(
            "SELECT art_path FROM albums WHERE key = ?1",
            params![album_key],
            |r| r.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten();
    let art_path = match existing_art {
        Some(p) => Some(p),
        None => art
            .as_ref()
            .and_then(|bytes| images::save_cover(covers, &album_key, bytes))
            .map(|p| p.to_string_lossy().to_string()),
    };
    let _ = conn.execute(
        "INSERT INTO albums(key, name, artist, art_path) VALUES(?1, ?2, ?3, ?4)
         ON CONFLICT(key) DO UPDATE SET
           name = excluded.name,
           artist = excluded.artist,
           art_path = COALESCE(albums.art_path, excluded.art_path)",
        params![album_key, album, album_artist, art_path],
    );

    let _ = conn.execute(
        "INSERT INTO tracks(path, title, artist, album, album_artist, album_key, track_no, disc_no, duration, format, size, mtime)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)
         ON CONFLICT(path) DO UPDATE SET
           title=excluded.title, artist=excluded.artist, album=excluded.album,
           album_artist=excluded.album_artist, album_key=excluded.album_key,
           track_no=excluded.track_no, disc_no=excluded.disc_no,
           duration=excluded.duration, format=excluded.format,
           size=excluded.size, mtime=excluded.mtime",
        params![
            path.to_string_lossy(),
            title,
            artist,
            album,
            album_artist,
            album_key,
            track_no,
            disc_no,
            duration,
            ext,
            size,
            mtime
        ],
    );
}
