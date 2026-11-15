use std::path::Path;

use rusqlite::{params, Connection};

use crate::dto::SettingsDto;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS tracks(
  id INTEGER PRIMARY KEY,
  path TEXT UNIQUE NOT NULL,
  title TEXT NOT NULL DEFAULT '',
  artist TEXT NOT NULL DEFAULT '',
  album TEXT NOT NULL DEFAULT '',
  album_artist TEXT NOT NULL DEFAULT '',
  album_key TEXT NOT NULL DEFAULT '',
  track_no INTEGER,
  disc_no INTEGER,
  duration REAL NOT NULL DEFAULT 0,
  format TEXT NOT NULL DEFAULT '',
  size INTEGER NOT NULL DEFAULT 0,
  mtime INTEGER NOT NULL DEFAULT 0,
  art_path TEXT
);
CREATE INDEX IF NOT EXISTS idx_tracks_album ON tracks(album_key);
CREATE INDEX IF NOT EXISTS idx_tracks_sort ON tracks(artist, album, track_no, title);
CREATE TABLE IF NOT EXISTS albums(
  key TEXT PRIMARY KEY,
  name TEXT NOT NULL DEFAULT '',
  artist TEXT NOT NULL DEFAULT '',
  art_path TEXT
);
CREATE TABLE IF NOT EXISTS meta_cache(
  key TEXT PRIMARY KEY,
  lyrics TEXT,
  provider TEXT NOT NULL DEFAULT '',
  fetched_at INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS settings(
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL DEFAULT ''
);
-- user-edited metadata, keyed by file path (stable across rescans, unlike
-- the track id). Empty string = no override for that field.
CREATE TABLE IF NOT EXISTS track_overrides(
  path TEXT PRIMARY KEY,
  title TEXT NOT NULL DEFAULT '',
  artist TEXT NOT NULL DEFAULT '',
  album TEXT NOT NULL DEFAULT '',
  updated_at INTEGER NOT NULL DEFAULT 0
);
"#;

pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.execute_batch(SCHEMA)?;
    migrate(&conn)?;
    Ok(conn)
}

/// Idempotent column additions for libraries created by older builds.
fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let has_art_path = {
        let mut stmt = conn.prepare("PRAGMA table_info(tracks)")?;
        let names: Vec<String> = stmt
            .query_map([], |r| r.get::<_, String>(1))?
            .filter_map(Result::ok)
            .collect();
        names.iter().any(|n| n == "art_path")
    };
    if !has_art_path {
        conn.execute("ALTER TABLE tracks ADD COLUMN art_path TEXT", [])?;
    }
    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| {
        r.get(0)
    })
    .ok()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) {
    let _ = conn.execute(
        "INSERT INTO settings(key, value) VALUES(?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    );
}

pub fn load_settings(conn: &Connection) -> SettingsDto {
    let mut s = SettingsDto::default();
    if let Some(v) = get_setting(conn, "music_dirs") {
        if let Ok(dirs) = serde_json::from_str::<Vec<String>>(&v) {
            s.music_dirs = dirs;
        }
    }
    if let Some(v) = get_setting(conn, "menubar_lyrics") {
        s.menubar_lyrics = v != "false";
    }
    if let Some(v) = get_setting(conn, "menubar_max_chars") {
        if let Ok(n) = v.parse::<u32>() {
            s.menubar_max_chars = n.clamp(8, 60);
        }
    }
    if let Some(v) = get_setting(conn, "lyrics_scale") {
        if let Ok(n) = v.parse::<u32>() {
            s.lyrics_scale = n.clamp(80, 300);
        }
    }
    if let Some(v) = get_setting(conn, "online_metadata") {
        s.online_metadata = v != "false";
    }
    if let Some(v) = get_setting(conn, "menubar_only") {
        s.menubar_only = v == "true";
    }
    s
}

pub fn save_settings(conn: &Connection, s: &SettingsDto) -> rusqlite::Result<()> {
    // Do not publish a partially saved snapshot, or silently accept a failed write.
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO settings(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )?;
        let values = [
            (
                "music_dirs",
                serde_json::to_string(&s.music_dirs).unwrap_or_default(),
            ),
            ("menubar_lyrics", s.menubar_lyrics.to_string()),
            ("menubar_max_chars", s.menubar_max_chars.to_string()),
            ("lyrics_scale", s.lyrics_scale.clamp(80, 300).to_string()),
            ("online_metadata", s.online_metadata.to_string()),
            ("menubar_only", s.menubar_only.to_string()),
        ];
        for (key, value) in values {
            stmt.execute(params![key, value])?;
        }
    }
    // The desktop floating-lyrics feature was removed; retire its inert keys
    // as part of the next successful settings transaction.
    tx.execute(
        "DELETE FROM settings WHERE key IN ('floating_lyrics', 'floating_pos')",
        [],
    )?;
    tx.commit()
}

pub fn counts(conn: &Connection) -> (i64, i64) {
    let tracks = conn
        .query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get(0))
        .unwrap_or(0);
    let albums = conn
        .query_row("SELECT COUNT(*) FROM albums", [], |r| r.get(0))
        .unwrap_or(0);
    (tracks, albums)
}

// ---------- user metadata overrides ----------

/// Overrides stored for a file, `None` when it was never edited.
pub fn load_override(conn: &Connection, path: &str) -> Option<(String, String, String)> {
    conn.query_row(
        "SELECT title, artist, album FROM track_overrides WHERE path = ?1",
        [path],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .ok()
}

pub fn save_override(conn: &Connection, path: &str, title: &str, artist: &str, album: &str) {
    if title.is_empty() && artist.is_empty() && album.is_empty() {
        let _ = conn.execute("DELETE FROM track_overrides WHERE path = ?1", [path]);
        return;
    }
    let _ = conn.execute(
        "INSERT INTO track_overrides(path, title, artist, album, updated_at)
         VALUES(?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(path) DO UPDATE SET
           title = excluded.title, artist = excluded.artist,
           album = excluded.album, updated_at = excluded.updated_at",
        params![path, title, artist, album, crate::state::now_ms()],
    );
}

pub fn delete_orphan_overrides(conn: &Connection) {
    let _ = conn.execute(
        "DELETE FROM track_overrides WHERE path NOT IN (SELECT path FROM tracks)",
        [],
    );
}

pub struct MetaUpdate {
    /// title or artist changed, so the cached lyrics no longer address this track
    pub lyrics_key_changed: bool,
    /// the track moved to another album row
    pub album_key_changed: bool,
}

/// Apply a user edit: persist the override, rewrite the effective row and keep
/// the album grouping consistent.
pub fn apply_track_metadata(
    conn: &Connection,
    id: i64,
    title: &str,
    artist: &str,
    album: &str,
) -> rusqlite::Result<Option<MetaUpdate>> {
    let old: Option<(String, String, String, String, String)> = conn
        .query_row(
            "SELECT path, title, artist, album_artist, album_key FROM tracks WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .ok();
    let Some((path, old_title, old_artist, old_album_artist, old_album_key)) = old else {
        return Ok(None);
    };

    save_override(conn, &path, title, artist, album);

    // follow the new artist only when album_artist was derived from it,
    // otherwise a compilation would be split apart
    let album_artist = if old_album_artist == old_artist {
        artist
    } else {
        old_album_artist.as_str()
    };
    let album_key = format!(
        "{}|{}",
        crate::metadata::normalize(album),
        crate::metadata::normalize(album_artist)
    );

    conn.execute(
        "INSERT INTO albums(key, name, artist) VALUES(?1, ?2, ?3)
         ON CONFLICT(key) DO UPDATE SET name = excluded.name, artist = excluded.artist",
        params![album_key, album, album_artist],
    )?;
    conn.execute(
        "UPDATE tracks SET title = ?2, artist = ?3, album = ?4, album_artist = ?5,
                           album_key = ?6
         WHERE id = ?1",
        params![id, title, artist, album, album_artist, album_key],
    )?;

    let old_cache = crate::metadata::cache_key(&old_title, &old_artist);
    let new_cache = crate::metadata::cache_key(title, artist);
    let lyrics_key_changed = old_cache != new_cache;
    if lyrics_key_changed {
        let _ = conn.execute(
            "DELETE FROM meta_cache WHERE key = ?1",
            [old_cache.as_str()],
        );
    }

    let _ = conn.execute(
        "DELETE FROM albums WHERE key NOT IN (SELECT DISTINCT album_key FROM tracks)",
        [],
    );

    Ok(Some(MetaUpdate {
        lyrics_key_changed,
        album_key_changed: album_key != old_album_key,
    }))
}

/// Drop a per-track cover so the album cover applies again.
pub fn clear_track_cover(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("UPDATE tracks SET art_path = NULL WHERE id = ?1", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static SEQ: AtomicUsize = AtomicUsize::new(0);

    fn temp_path(tag: &str) -> std::path::PathBuf {
        let n = SEQ.fetch_add(1, Ordering::SeqCst);
        let mut p = std::env::temp_dir();
        p.push(format!("sonora-test-{tag}-{}-{n}.db", std::process::id()));
        let _ = std::fs::remove_file(&p);
        p
    }

    #[test]
    fn lyrics_scale_defaults_to_standard_for_old_libraries() {
        let path = temp_path("scale-default");
        let conn = open(&path).unwrap();
        assert_eq!(load_settings(&conn).lyrics_scale, 100);
        drop(conn);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lyrics_scale_survives_reopening_the_database() {
        let path = temp_path("scale-roundtrip");
        {
            let conn = open(&path).unwrap();
            let mut settings = SettingsDto::default();
            settings.lyrics_scale = 135;
            save_settings(&conn, &settings).unwrap();
        }
        let conn = open(&path).unwrap();
        assert_eq!(load_settings(&conn).lyrics_scale, 135);
        drop(conn);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lyrics_scale_load_clamps_bounds_and_defaults_on_invalid_input() {
        let path = temp_path("scale-bounds");
        let conn = open(&path).unwrap();
        for (value, expected) in [
            ("10", 80),
            ("999", 300),
            ("80", 80),
            ("160", 160),
            ("300", 300),
            ("abc", 100),
            ("-1", 100),
            ("100.5", 100),
        ] {
            set_setting(&conn, "lyrics_scale", value);
            assert_eq!(load_settings(&conn).lyrics_scale, expected, "{value}");
        }
        drop(conn);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn lyrics_scale_save_clamps_bounds() {
        let path = temp_path("scale-save-bounds");
        let conn = open(&path).unwrap();
        let mut settings = SettingsDto::default();
        for (value, expected) in [(0, 80), (300, 300), (999, 300)] {
            settings.lyrics_scale = value;
            save_settings(&conn, &settings).unwrap();
            assert_eq!(load_settings(&conn).lyrics_scale, expected);
        }
        drop(conn);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn removed_desktop_lyrics_settings_are_retired_without_changing_menu_lyrics() {
        let path = temp_path("retired-desktop-lyrics");
        let conn = open(&path).unwrap();
        set_setting(&conn, "floating_lyrics", "true");
        set_setting(&conn, "floating_pos", "40,80");
        let settings = load_settings(&conn);
        assert!(settings.menubar_lyrics);
        save_settings(&conn, &settings).unwrap();
        assert!(get_setting(&conn, "floating_lyrics").is_none());
        assert!(get_setting(&conn, "floating_pos").is_none());
        assert!(load_settings(&conn).menubar_lyrics);
        drop(conn);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn settings_write_failure_is_reported_and_rolls_back_the_snapshot() {
        let path = temp_path("settings-rollback");
        let conn = open(&path).unwrap();
        let mut settings = SettingsDto::default();
        save_settings(&conn, &settings).unwrap();
        conn.execute_batch(
            "CREATE TRIGGER reject_scale BEFORE UPDATE ON settings
             WHEN NEW.key = 'lyrics_scale'
             BEGIN SELECT RAISE(ABORT, 'simulated settings failure'); END;",
        )
        .unwrap();
        settings.menubar_lyrics = false;
        settings.lyrics_scale = 135;
        assert!(save_settings(&conn, &settings).is_err());
        let actual = load_settings(&conn);
        assert!(actual.menubar_lyrics);
        assert_eq!(actual.lyrics_scale, 100);
        drop(conn);
        let _ = std::fs::remove_file(&path);
    }

    fn seed(conn: &Connection) {
        conn.execute(
            "INSERT INTO tracks(id, path, title, artist, album, album_artist, album_key)
             VALUES(1, '/tmp/a.flac', '旧标题', '旧艺人', '旧专辑', '旧艺人', 'old|key')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO albums(key, name, artist) VALUES('old|key', '旧专辑', '旧艺人')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO meta_cache(key, lyrics, provider) VALUES(?1, '[00:01.00]hi', 'netease')",
            [crate::metadata::cache_key("旧标题", "旧艺人")],
        )
        .unwrap();
    }

    #[test]
    fn migrates_old_track_table() {
        let path = temp_path("migrate");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE tracks(id INTEGER PRIMARY KEY, path TEXT UNIQUE NOT NULL,
                   title TEXT NOT NULL DEFAULT '', artist TEXT NOT NULL DEFAULT '',
                   album TEXT NOT NULL DEFAULT '', album_artist TEXT NOT NULL DEFAULT '',
                   album_key TEXT NOT NULL DEFAULT '', track_no INTEGER, disc_no INTEGER,
                   duration REAL NOT NULL DEFAULT 0, format TEXT NOT NULL DEFAULT '',
                   size INTEGER NOT NULL DEFAULT 0, mtime INTEGER NOT NULL DEFAULT 0);",
            )
            .unwrap();
        }
        let conn = open(&path).unwrap();
        let cols: Vec<String> = conn
            .prepare("PRAGMA table_info(tracks)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .filter_map(Result::ok)
            .collect();
        assert!(cols.iter().any(|c| c == "art_path"));
        drop(conn);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn apply_metadata_rebuilds_album_and_invalidates_cache() {
        let path = temp_path("apply");
        let conn = open(&path).unwrap();
        seed(&conn);

        let update = apply_track_metadata(&conn, 1, "新标题", "新艺人", "新专辑")
            .unwrap()
            .expect("track exists");
        assert!(update.lyrics_key_changed);
        assert!(update.album_key_changed);

        let (title, artist, album, art, key): (String, String, String, Option<String>, String) =
            conn.query_row(
                "SELECT title, artist, album, art_path, album_key FROM tracks WHERE id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .unwrap();
        assert_eq!(
            (title.as_str(), artist.as_str(), album.as_str()),
            ("新标题", "新艺人", "新专辑")
        );
        assert!(art.is_none());
        assert_eq!(key, "新专辑|新艺人");

        // old empty album row is gone, the new one exists with no art
        let (count, name, album_art): (i64, String, Option<String>) = conn
            .query_row(
                "SELECT COUNT(*), MAX(name), MAX(art_path) FROM albums",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(count, 1);
        assert_eq!(name, "新专辑");
        assert!(album_art.is_none());

        // stale lyrics for the old title must not survive
        let cached: i64 = conn
            .query_row("SELECT COUNT(*) FROM meta_cache", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cached, 0);

        // override is stored for the next rescan
        assert_eq!(
            load_override(&conn, "/tmp/a.flac").unwrap(),
            (
                "新标题".to_string(),
                "新艺人".to_string(),
                "新专辑".to_string()
            )
        );

        drop(conn);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn album_only_edit_keeps_lyrics_cache() {
        let path = temp_path("album");
        let conn = open(&path).unwrap();
        seed(&conn);

        let update = apply_track_metadata(&conn, 1, "旧标题", "旧艺人", "另一张专辑")
            .unwrap()
            .expect("track exists");
        assert!(!update.lyrics_key_changed);
        assert!(update.album_key_changed);

        let cached: i64 = conn
            .query_row("SELECT COUNT(*) FROM meta_cache", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cached, 1);

        drop(conn);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn clearing_all_fields_removes_the_override() {
        let path = temp_path("clear");
        let conn = open(&path).unwrap();
        seed(&conn);

        apply_track_metadata(&conn, 1, "", "", "").unwrap();
        assert!(load_override(&conn, "/tmp/a.flac").is_none());

        drop(conn);
        let _ = std::fs::remove_file(&path);
    }
}
