//! Lyrics and cover providers for the local music library. Candidate matching
//! uses track identity and duration before fetching synchronized lyrics or art.

pub mod kugou;
pub mod lrclib;
pub mod match_score;
pub mod netease;
pub mod qq;
pub mod ttml;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rusqlite::{params, Connection};
use tauri::{AppHandle, Emitter, Manager};

use crate::db;
use crate::dto::LyricLineDto;
use crate::state::{now_ms, AppContext};

pub struct FetchResult {
    pub lyrics: Option<(Vec<LyricLineDto>, String)>,
    pub cover_saved: bool,
}

pub fn http_client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15")
            .timeout(Duration::from_secs(10))
            .build()
            .expect("failed to build http client")
    })
}

/// Normalize a title/artist for cache keys: lowercase, strip bracketed
/// qualifiers, keep only alphanumeric. (Stable format — cached keys must
/// stay valid across versions.)
pub fn normalize(s: &str) -> String {
    let lowered = s.to_lowercase();
    let mut out = String::new();
    let mut depth = 0usize;
    for ch in lowered.chars() {
        match ch {
            '(' | '[' | '（' | '【' | '{' => depth += 1,
            ')' | ']' | '）' | '】' | '}' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(ch),
            _ => {}
        }
    }
    let cleaned: String = out
        .split(|c: char| c == '/' || c == ',' || c == '&' || c == ';' || c == '、')
        .next()
        .unwrap_or("")
        .to_string();
    cleaned.chars().filter(|c| c.is_alphanumeric()).collect()
}

pub fn cache_key(title: &str, artist: &str) -> String {
    format!("{}|{}", normalize(title), normalize(artist))
}

pub fn cached_lyrics(conn: &Connection, key: &str) -> Option<(String, String)> {
    conn.query_row(
        "SELECT lyrics, provider FROM meta_cache WHERE key = ?1 AND lyrics IS NOT NULL",
        [key],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
    )
    .ok()
}

/// A short lyric timeline is a reason to revalidate, not proof of truncation:
/// legitimate songs can have long instrumental endings. Keep the old cache as
/// an offline/failure fallback, and don't retry a freshly fetched short timeline
/// on every play (at most once per day).
pub fn cached_lyrics_need_refresh(conn: &Connection, key: &str, duration: f64) -> bool {
    let Some((text, provider)) = cached_lyrics(conn, key) else {
        return false;
    };
    let lines = parse_by_provider(&text, &provider);
    if !lyrics_timeline_is_suspicious(&lines, duration) {
        return false;
    }
    let fetched_at: i64 = conn
        .query_row(
            "SELECT fetched_at FROM meta_cache WHERE key = ?1",
            [key],
            |r| r.get(0),
        )
        .unwrap_or(0);
    now_ms().saturating_sub(fetched_at) >= 24 * 60 * 60 * 1000
}

fn lyrics_timeline_is_suspicious(lines: &[LyricLineDto], duration: f64) -> bool {
    if !duration.is_finite() || duration < 120.0 {
        return false;
    }
    let last = lines
        .iter()
        .filter(|l| !l.text.trim().is_empty() && l.t.is_finite())
        .map(|l| l.t)
        .reduce(f64::max);
    last.is_some_and(|last| last < duration * 0.6 && duration - last >= 60.0)
}

pub fn save_cached_lyrics(conn: &Connection, key: &str, lyrics: &str, provider: &str) {
    let _ = conn.execute(
        "INSERT INTO meta_cache(key, lyrics, provider, fetched_at) VALUES(?1, ?2, ?3, ?4)
         ON CONFLICT(key) DO UPDATE SET lyrics = excluded.lyrics, provider = excluded.provider, fetched_at = excluded.fetched_at",
        params![key, lyrics, provider, now_ms()],
    );
}

/// Parse an LRC document into timed lines (supports multiple timestamps per
/// line and mm:ss.xx / m:ss.xxx / h:mm:ss variants).
pub fn parse_lrc(text: &str) -> Vec<LyricLineDto> {
    let mut lines: Vec<LyricLineDto> = Vec::new();
    for raw in text.lines() {
        let mut rest = raw.trim();
        let mut times: Vec<f64> = Vec::new();
        while rest.starts_with('[') {
            let Some(end) = rest.find(']') else { break };
            let tag = &rest[1..end];
            if let Some(t) = parse_lrc_time(tag) {
                times.push(t);
                rest = rest[end + 1..].trim_start();
            } else {
                break; // metadata tag like [ti:...] — whole line is not a lyric
            }
        }
        if times.is_empty() {
            continue;
        }
        let txt = rest.trim();
        if txt.is_empty() {
            continue;
        }
        for t in times {
            lines.push(LyricLineDto {
                t,
                text: txt.to_string(),
                words: None,
            });
        }
    }
    lines.sort_by(|a, b| a.t.partial_cmp(&b.t).unwrap_or(std::cmp::Ordering::Equal));
    lines
}

fn parse_lrc_time(tag: &str) -> Option<f64> {
    // mm:ss.xx | mm:ss.xxx | hh:mm:ss
    let parts: Vec<&str> = tag.split(':').collect();
    if parts.len() < 2 {
        return None;
    }
    let secs_raw = parts.last()?;
    let (sec_str, frac_str) = match secs_raw.split_once('.') {
        Some((s, f)) => (s, f),
        None => (*secs_raw, ""),
    };
    let sec: f64 = sec_str.parse().ok()?;
    let frac: f64 = if frac_str.is_empty() {
        0.0
    } else {
        let digits = &frac_str[..frac_str.len().min(3)];
        let n: f64 = digits.parse().ok()?;
        n / 10f64.powi(digits.len() as i32)
    };
    let mut total = sec + frac;
    if parts.len() == 3 {
        let mm: f64 = parts[1].parse().ok()?;
        let hh: f64 = parts[0].parse().ok()?;
        total += mm * 60.0 + hh * 3600.0;
    } else {
        let mm: f64 = parts[0].parse().ok()?;
        total += mm * 60.0;
    }
    Some(total)
}

/// Parse a cached lyric document by its provider marker.
pub fn parse_by_provider(text: &str, provider: &str) -> Vec<LyricLineDto> {
    if provider == "amll" {
        ttml::parse_ttml(text)
    } else {
        parse_lrc(text)
    }
}

// ---------- core fetch ----------

/// What the caller wants out of one fetch.
#[derive(Default, Clone, Copy)]
pub struct FetchOpts {
    pub want_lyrics: bool,
    /// ignore the lyrics cache (manual refetch); a failed fetch leaves the
    /// cached lyrics untouched
    pub force_lyrics: bool,
    /// fetch a cover for this track only, instead of the album cover
    pub track_cover: bool,
}

/// One QQ search reused by both the lyrics step and the cover step.
struct QqMatch {
    songs: Vec<qq::QqSong>,
    idx: usize,
    score: f64,
}

async fn qq_lookup(
    client: &reqwest::Client,
    title: &str,
    artist: &str,
    album: Option<&str>,
    duration_ms: f64,
) -> Option<QqMatch> {
    match qq::search(client, &format!("{title} {artist}")).await {
        Ok(songs) if !songs.is_empty() => {
            let cands: Vec<match_score::Candidate> = songs
                .iter()
                .map(|s| match_score::Candidate {
                    title: s.songname.clone(),
                    artists: s.singers.clone(),
                    album: s.albumname.clone(),
                    duration_ms: s.interval_s * 1000.0,
                })
                .collect();
            match_score::best_candidate(title, artist, album, duration_ms, &cands).map(
                |(idx, m)| QqMatch {
                    songs,
                    idx,
                    score: m.score,
                },
            )
        }
        Ok(_) => None,
        Err(e) => {
            eprintln!("[meta] qq search error: {e}");
            None
        }
    }
}

/// A request's generation and exact input identity. Only the synchronous
/// publication boundary takes the slot lock; no lock survives an await.
struct FetchGate {
    slot: Arc<Mutex<u64>>,
    generation: u64,
    track_id: i64,
    title: String,
    artist: String,
    album: String,
    duration: f64,
}

impl FetchGate {
    fn begin(
        slot: Arc<Mutex<u64>>,
        track_id: i64,
        title: &str,
        artist: &str,
        album: Option<&str>,
        duration: f64,
    ) -> Self {
        let generation = {
            let mut current = slot.lock().unwrap();
            *current = current.wrapping_add(1);
            *current
        };
        Self {
            slot,
            generation,
            track_id,
            title: title.to_string(),
            artist: artist.to_string(),
            album: album.unwrap_or_default().to_string(),
            duration,
        }
    }
}

/// Serialize validation with the short synchronous write/notification, including
/// per-track cover file replacement. An edit takes this same slot before updating
/// the row and invalidating its generation. `None` is reserved for the standalone
/// core (album warm-up and scratch-provider tests), which has no track publisher.
fn with_current_fetch<T>(
    gate: Option<&FetchGate>,
    conn: &Connection,
    action: impl FnOnce() -> T,
) -> Option<T> {
    let Some(gate) = gate else {
        return Some(action());
    };
    let current = gate.slot.lock().unwrap();
    if *current != gate.generation {
        return None;
    }
    let matches = conn
        .query_row(
            "SELECT title, artist, album, duration FROM tracks WHERE id = ?1",
            [gate.track_id],
            |row| {
                Ok(row.get::<_, String>(0)? == gate.title
                    && row.get::<_, String>(1)? == gate.artist
                    && row.get::<_, String>(2)? == gate.album
                    && row.get::<_, f64>(3)? == gate.duration)
            },
        )
        .unwrap_or(false);
    if !matches {
        return None;
    }
    Some(action())
}

fn save_fetched_lyrics(
    conn: &Connection,
    gate: Option<&FetchGate>,
    key: &str,
    lyrics: &str,
    provider: &str,
) {
    let _ = with_current_fetch(gate, conn, || {
        save_cached_lyrics(conn, key, lyrics, provider);
    });
}

/// Testable core of the metadata fetch (no Tauri state).
pub struct FetchCtx {
    pub db_path: std::path::PathBuf,
    pub covers_dir: std::path::PathBuf,
}

fn persist_fetched_cover(
    ctx: &FetchCtx,
    conn: &Connection,
    gate: Option<&FetchGate>,
    track_id: Option<i64>,
    album_key: Option<&str>,
    track_cover: bool,
    bytes: &[u8],
) -> bool {
    with_current_fetch(gate, conn, || {
        if track_cover {
            if let Some(id) = track_id {
                if let Some(path) = crate::images::save_track_cover(&ctx.covers_dir, id, bytes) {
                    let _ = conn.execute(
                        "UPDATE tracks SET art_path = ?2 WHERE id = ?1",
                        params![id, path.to_string_lossy().to_string()],
                    );
                    eprintln!("[meta] track cover saved for id={id}");
                    return true;
                }
            }
        } else if let Some(album_key) = album_key {
            if let Some(path) = crate::images::save_cover(&ctx.covers_dir, album_key, bytes) {
                let _ = conn.execute(
                    "UPDATE albums SET art_path = ?2 WHERE key = ?1 AND art_path IS NULL",
                    params![album_key, path.to_string_lossy().to_string()],
                );
                eprintln!("[meta] cover saved for album {album_key}");
                return true;
            }
        }
        false
    })
    .unwrap_or(false)
}

/// Fetch metadata for one track. Lyrics and cover are independent: a cache
/// hit for lyrics never skips the cover work (and vice versa).
pub async fn fetch_core(
    ctx: &FetchCtx,
    track_id: Option<i64>,
    title: &str,
    artist: &str,
    album: Option<&str>,
    duration: f64,
    album_key: Option<&str>,
    opts: FetchOpts,
) -> Option<FetchResult> {
    fetch_core_with_lyrics(
        ctx,
        track_id,
        title,
        artist,
        album,
        duration,
        album_key,
        opts,
        None,
        |_, _| {},
    )
    .await
}

/// The callback runs as soon as lyrics are ready, before any cover lookup or
/// download. Kept separate from Tauri so delivery ordering is regression-testable.
async fn fetch_core_with_lyrics(
    ctx: &FetchCtx,
    track_id: Option<i64>,
    title: &str,
    artist: &str,
    album: Option<&str>,
    duration: f64,
    album_key: Option<&str>,
    opts: FetchOpts,
    gate: Option<&FetchGate>,
    on_lyrics: impl FnOnce(&[LyricLineDto], &str),
) -> Option<FetchResult> {
    let key = cache_key(title, artist);
    let conn = match db::open(&ctx.db_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[meta] db open failed: {e}");
            return None;
        }
    };
    let duration_ms = duration * 1000.0;

    // --- what do we still need? ---
    let mut lyrics_result: Option<(Vec<LyricLineDto>, String)> = None;
    if opts.want_lyrics && !opts.force_lyrics && !cached_lyrics_need_refresh(&conn, &key, duration)
    {
        if let Some((text, provider)) = cached_lyrics(&conn, &key) {
            let lines = parse_by_provider(&text, &provider);
            if !lines.is_empty() {
                eprintln!("[meta] lyrics cache hit ({provider}) for {title}");
                lyrics_result = Some((lines, provider));
            }
        }
    }
    let mut on_lyrics = Some(on_lyrics);
    if let Some((lines, source)) = &lyrics_result {
        // Cached lyrics must not wait even for a cover-only search.
        if let Some(notify) = on_lyrics.take() {
            notify(lines, source);
        }
    }
    let need_cover = album_key
        .map(|k| album_has_no_art(&conn, k))
        .unwrap_or(false);
    if lyrics_result.is_some() && !need_cover && !opts.track_cover {
        return Some(FetchResult {
            lyrics: lyrics_result,
            cover_saved: false,
        });
    }

    let client = http_client();
    let query = format!("{title} {artist}");

    // --- 1. Search Netease for a matching track ---
    let want_album_cover = need_cover && !opts.track_cover;
    let mut best_nc: Option<(i64, match_score::Matched)> = None;
    let mut qq_match: Option<QqMatch> = None;
    if (opts.want_lyrics && lyrics_result.is_none()) || need_cover || opts.track_cover {
        match netease::search(client, &query).await {
            Ok(songs) if !songs.is_empty() => {
                let cands: Vec<match_score::Candidate> = songs
                    .iter()
                    .map(|s| match_score::Candidate {
                        title: s.name.clone(),
                        artists: s.artists.clone(),
                        album: s.album.clone(),
                        duration_ms: s.duration_ms,
                    })
                    .collect();
                if let Some((idx, m)) =
                    match_score::best_candidate(title, artist, album, duration_ms, &cands)
                {
                    let song = &songs[idx];
                    eprintln!(
                        "[meta] netease match id={} ({}) score={:.0} for {title}",
                        song.id, song.name, m.score
                    );
                    best_nc = Some((song.id, m));
                } else {
                    eprintln!("[meta] netease candidates rejected (title mismatch) for {title}");
                }
            }
            Ok(_) => eprintln!("[meta] netease search empty for {title}"),
            Err(e) => eprintln!("[meta] netease search error: {e}"),
        }
    }

    // --- 2. lyrics from the matched netease id (AMLL word-level first) ---
    if lyrics_result.is_none() && opts.want_lyrics {
        if let Some((nid, _)) = &best_nc {
            match ttml::fetch(client, *nid).await {
                Ok(Some(ttml_text)) => {
                    let lines = ttml::parse_ttml(&ttml_text);
                    if !lines.is_empty() {
                        save_fetched_lyrics(&conn, gate, &key, &ttml_text, "amll");
                        lyrics_result = Some((lines, "amll".to_string()));
                    }
                }
                Ok(None) => {}
                Err(e) => eprintln!("[meta] amll fetch error: {e}"),
            }
            if lyrics_result.is_none() {
                match netease::lyric(client, *nid).await {
                    Ok(Some(lrc_text)) => {
                        let lines = parse_lrc(&lrc_text);
                        if !lines.is_empty() {
                            save_fetched_lyrics(&conn, gate, &key, &lrc_text, "netease");
                            lyrics_result = Some((lines, "netease".to_string()));
                        }
                    }
                    Ok(None) => eprintln!("[meta] netease has no lyric for id={nid}"),
                    Err(e) => eprintln!("[meta] netease lyric error: {e}"),
                }
            }
        }
    }

    // --- 2b. QQ lyrics — works for catalogues Netease hides from anonymous
    //         search (AMLL is keyed by a Netease id, so it cannot help there) ---
    if lyrics_result.is_none() && opts.want_lyrics {
        if qq_match.is_none() {
            qq_match = qq_lookup(client, title, artist, album, duration_ms).await;
        }
        if let Some(m) = &qq_match {
            if let Some(mid) = m.songs[m.idx].songmid.as_deref() {
                match qq::lyric(client, mid).await {
                    Ok(Some(lrc_text)) => {
                        let lines = parse_lrc(&lrc_text);
                        if !lines.is_empty() {
                            save_fetched_lyrics(&conn, gate, &key, &lrc_text, "qq");
                            lyrics_result = Some((lines, "qq".to_string()));
                        }
                    }
                    Ok(None) => eprintln!("[meta] qq has no lyric for mid={mid}"),
                    Err(e) => eprintln!("[meta] qq lyric error: {e}"),
                }
            }
        }
    }

    // --- 3. LRCLIB lyrics fallback ---
    if lyrics_result.is_none() && opts.want_lyrics {
        match lrclib::search(client, title, artist).await {
            Ok(hits) if !hits.is_empty() => {
                let cands: Vec<match_score::Candidate> = hits
                    .iter()
                    .map(|h| match_score::Candidate {
                        title: h.title.clone(),
                        artists: vec![h.artist.clone()],
                        album: h.album.clone(),
                        duration_ms: h.duration.unwrap_or(0.0) * 1000.0,
                    })
                    .collect();
                if let Some((idx, _)) =
                    match_score::best_candidate(title, artist, album, duration_ms, &cands)
                {
                    let synced = &hits[idx].synced;
                    let lines = parse_lrc(synced);
                    if !lines.is_empty() {
                        save_fetched_lyrics(&conn, gate, &key, synced, "lrclib");
                        lyrics_result = Some((lines, "lrclib".to_string()));
                    }
                }
            }
            Ok(_) => {}
            Err(e) => eprintln!("[meta] lrclib error: {e}"),
        }
    }

    // --- 4. cover: QQ (best pool, direct URL) → netease song/detail → kugou ---
    //    `track_cover` writes a per-track file that shadows the album cover;
    //    otherwise the missing album cover is filled in.
    let cover_work = async move {
        let mut cover_saved = false;
        if want_album_cover || opts.track_cover {
            let mut pic_url: Option<String> = None;

            if qq_match.is_none() {
                qq_match = qq_lookup(client, title, artist, album, duration_ms).await;
            }
            if let Some(m) = &qq_match {
                if let Some(mid) = m.songs[m.idx].albummid.as_deref() {
                    pic_url = Some(qq::cover_url(mid));
                }
                eprintln!(
                    "[meta] qq cover match '{}' by {:?} score={:.0}",
                    m.songs[m.idx].songname, m.songs[m.idx].singers, m.score
                );
            }

            if pic_url.is_none() {
                if let Some((nid, _)) = &best_nc {
                    pic_url = netease::song_detail_cover(client, *nid)
                        .await
                        .ok()
                        .flatten();
                }
            }

            if pic_url.is_none() {
                match kugou::search(client, &query).await {
                    Ok(songs) if !songs.is_empty() => {
                        let cands: Vec<match_score::Candidate> = songs
                            .iter()
                            .map(|s| match_score::Candidate {
                                title: s.name.clone(),
                                artists: s.artists.clone(),
                                album: s.album.clone(),
                                duration_ms: s.duration_s * 1000.0,
                            })
                            .collect();
                        if let Some((idx, m)) =
                            match_score::best_candidate(title, artist, album, duration_ms, &cands)
                        {
                            pic_url = songs[idx].img.clone();
                            eprintln!(
                                "[meta] kugou cover fallback score={:.0} for {title}",
                                m.score
                            );
                        }
                    }
                    Ok(_) => {}
                    Err(e) => eprintln!("[meta] kugou search error: {e}"),
                }
            }

            if let Some(url) = pic_url {
                let mut bytes = download_cover(client, &url).await;
                if bytes.is_none() {
                    // some albums only have the small variant materialized
                    let small = url.replace("R800x800", "R300x300");
                    if small != url {
                        bytes = download_cover(client, &small).await;
                    }
                }
                if let Some(bytes) = bytes {
                    cover_saved = persist_fetched_cover(
                        ctx,
                        &conn,
                        gate,
                        track_id,
                        album_key,
                        opts.track_cover,
                        &bytes,
                    );
                }
            } else {
                eprintln!("[meta] no cover found for {title}");
            }
        }

        cover_saved
    };
    deliver_lyrics_before_cover(lyrics_result, on_lyrics, cover_work).await
}

async fn deliver_lyrics_before_cover(
    lyrics: Option<(Vec<LyricLineDto>, String)>,
    on_lyrics: Option<impl FnOnce(&[LyricLineDto], &str)>,
    cover: impl std::future::Future<Output = bool>,
) -> Option<FetchResult> {
    if let (Some((lines, source)), Some(notify)) = (&lyrics, on_lyrics) {
        notify(lines, source);
    }
    let cover_saved = cover.await;
    if lyrics.is_none() && !cover_saved {
        return None;
    }
    Some(FetchResult {
        lyrics,
        cover_saved,
    })
}

fn album_has_no_art(conn: &Connection, album_key: &str) -> bool {
    conn.query_row(
        "SELECT art_path FROM albums WHERE key = ?1",
        [album_key],
        |r| r.get::<_, Option<String>>(0),
    )
    .ok()
    .flatten()
    .is_none()
}

async fn download_cover(client: &reqwest::Client, url: &str) -> Option<Vec<u8>> {
    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let b = resp.bytes().await.ok()?;
    Some(b.to_vec())
}

/// App-facing wrapper for the playing-track fetch.
pub async fn fetch_for_track(
    app: &AppHandle,
    track_id: i64,
    title: &str,
    artist: &str,
    album: Option<&str>,
    duration: f64,
    album_key: Option<&str>,
    opts: FetchOpts,
) -> Option<FetchResult> {
    let (ctx, gate) = {
        let state = app.state::<AppContext>();
        let gate = FetchGate::begin(
            state.metadata_fetch_slot(track_id),
            track_id,
            title,
            artist,
            album,
            duration,
        );
        let ctx = FetchCtx {
            db_path: state.db_path.clone(),
            covers_dir: state.covers_dir.clone(),
        };
        (ctx, gate)
    };
    let result = fetch_core_with_lyrics(
        &ctx,
        Some(track_id),
        title,
        artist,
        album,
        duration,
        album_key,
        opts,
        Some(&gate),
        |lines, source| {
            let Ok(conn) = db::open(&ctx.db_path) else {
                return;
            };
            let _ = with_current_fetch(Some(&gate), &conn, || {
                let _ = app.emit(
                    "lyrics://updated",
                    crate::dto::LyricsDto {
                        track_id,
                        lines: lines.to_vec(),
                        source: source.to_string(),
                    },
                );
                let state = app.state::<AppContext>();
                let _ = state.engine.send(crate::audio::EngineCmd::SetLyrics {
                    track_id,
                    lines: lines.to_vec(),
                    source: source.to_string(),
                });
            });
        },
    )
    .await;
    // Do not report an obsolete request as a successful manual refetch either.
    let conn = db::open(&ctx.db_path).ok()?;
    with_current_fetch(Some(&gate), &conn, || result).flatten()
}

// ---------- background cover warm-up (whole library, folia-style batches) ----------

static WARMUP_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Walk albums that have no art and fetch covers one by one (rate-limited).
/// Triggered after each library scan; respects the online_metadata setting.
pub fn spawn_cover_warmup(app: AppHandle) {
    use std::sync::atomic::Ordering;
    if WARMUP_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let (ctx, online) = {
            let ctx = app.state::<AppContext>();
            let online = ctx.settings.lock().unwrap().online_metadata;
            let c = FetchCtx {
                db_path: ctx.db_path.clone(),
                covers_dir: ctx.covers_dir.clone(),
            };
            drop(ctx);
            (c, online)
        };
        if !online {
            WARMUP_RUNNING.store(false, Ordering::SeqCst);
            return;
        }
        let Ok(conn) = db::open(&ctx.db_path) else {
            WARMUP_RUNNING.store(false, Ordering::SeqCst);
            return;
        };
        let mut covered = 0usize;
        loop {
            let online = {
                let ctx = app.state::<AppContext>();
                let s = ctx.settings.lock().unwrap();
                s.online_metadata
            };
            if !online {
                break;
            }
            let next: Option<(String, String, String, String, f64)> = conn
                .query_row(
                    "SELECT a.key, t.title, t.artist, t.album, t.duration
                     FROM albums a
                     JOIN tracks t ON t.id = (
                       SELECT id FROM tracks WHERE album_key = a.key ORDER BY track_no, title LIMIT 1
                     )
                     WHERE a.art_path IS NULL
                     LIMIT 1",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
                )
                .ok();
            let Some((key, title, artist, album, duration)) = next else {
                break;
            };
            let result = tauri::async_runtime::block_on(fetch_core(
                &ctx,
                None,
                &title,
                &artist,
                Some(&album),
                duration,
                Some(&key),
                FetchOpts::default(),
            ));
            if result.map(|r| r.cover_saved).unwrap_or(false) {
                covered += 1;
                // The warm-up may have supplied the currently playing cover.
                let _ = app
                    .state::<AppContext>()
                    .engine
                    .send(crate::audio::EngineCmd::ReloadCurrent);
                crate::scanner::emit_library_state(&app);
            }
            std::thread::sleep(Duration::from_millis(800));
        }
        WARMUP_RUNNING.store(false, Ordering::SeqCst);
        eprintln!("[warmup] cover warm-up finished, {covered} fetched this run");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE meta_cache (key TEXT PRIMARY KEY, lyrics TEXT, provider TEXT, fetched_at INTEGER)").unwrap();
        conn
    }

    fn track_db() -> Connection {
        let conn = db::open(std::path::Path::new(":memory:")).unwrap();
        conn.execute(
            "INSERT INTO tracks(id, path, title, artist, album, duration)
             VALUES(7, '/fixture.mp3', 'fixture', 'artist', 'album', 240)",
            [],
        )
        .unwrap();
        conn
    }

    fn fixture_gate(slot: Arc<Mutex<u64>>) -> FetchGate {
        FetchGate::begin(slot, 7, "fixture", "artist", Some("album"), 240.0)
    }

    struct CoverFixture(FetchCtx);

    impl CoverFixture {
        fn new() -> Self {
            static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            loop {
                let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let dir = std::env::temp_dir().join(format!(
                    "sonora-fetch-gate-{}-{sequence}",
                    std::process::id()
                ));
                match std::fs::create_dir(&dir) {
                    Ok(()) => {
                        return Self(FetchCtx {
                            db_path: dir.join("unused.db"),
                            covers_dir: dir,
                        });
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    Err(error) => panic!("create fixture directory: {error}"),
                }
            }
        }
    }

    impl Drop for CoverFixture {
        fn drop(&mut self) {
            // Exclusively created by this fixture, never an existing directory.
            let _ = std::fs::remove_dir_all(&self.0.covers_dir);
        }
    }

    fn cover_png(color: [u8; 3]) -> Vec<u8> {
        let image =
            image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(8, 8, image::Rgb(color)));
        let mut output = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut output, image::ImageFormat::Png)
            .unwrap();
        output.into_inner()
    }

    #[test]
    fn older_same_identity_fetch_cannot_overwrite_newer_cache_cover_or_publish() {
        let conn = track_db();
        let slot = Arc::new(Mutex::new(0));
        let old_auto = fixture_gate(slot.clone());
        let new_manual = fixture_gate(slot.clone());
        let key = cache_key("fixture", "artist");
        let mut publications = Vec::new();
        let cover = CoverFixture::new();

        // R2 wins even though both requests have exactly the same identity.
        save_fetched_lyrics(&conn, Some(&new_manual), &key, "[00:01]new", "qq");
        with_current_fetch(Some(&new_manual), &conn, || {
            assert!(
                slot.try_lock().is_err(),
                "check and publication are serialized"
            );
            publications.push("new");
        })
        .unwrap();
        assert!(persist_fetched_cover(
            &cover.0,
            &conn,
            Some(&new_manual),
            Some(7),
            None,
            true,
            &cover_png([255, 0, 0]),
        ));
        let path: String = conn
            .query_row("SELECT art_path FROM tracks WHERE id = 7", [], |r| r.get(0))
            .unwrap();
        let main = std::fs::read(&path).unwrap();
        let blur_path = crate::images::blur_of(std::path::Path::new(&path));
        let blur = std::fs::read(&blur_path).unwrap();

        // Deterministically release R1 after R2's commits; no sleeps/network.
        save_fetched_lyrics(&conn, Some(&old_auto), &key, "[00:01]old", "netease");
        assert!(with_current_fetch(Some(&old_auto), &conn, || {
            publications.push("old");
        })
        .is_none());
        assert!(!persist_fetched_cover(
            &cover.0,
            &conn,
            Some(&old_auto),
            Some(7),
            None,
            true,
            &cover_png([0, 0, 255]),
        ));
        assert_eq!(
            cached_lyrics(&conn, &key),
            Some(("[00:01]new".into(), "qq".into()))
        );
        assert_eq!(publications, vec!["new"]);
        assert_eq!(std::fs::read(&path).unwrap(), main);
        assert_eq!(std::fs::read(&blur_path).unwrap(), blur);
        let current_path: String = conn
            .query_row("SELECT art_path FROM tracks WHERE id = 7", [], |r| r.get(0))
            .unwrap();
        assert_eq!(current_path, path);
    }

    #[test]
    fn metadata_edit_without_refetch_invalidates_even_an_unchanged_identity() {
        for edited_title in ["fixture", "edited"] {
            let conn = track_db();
            let slot = Arc::new(Mutex::new(0));
            let pending = fixture_gate(slot.clone());
            let key = cache_key("fixture", "artist");
            let cover = CoverFixture::new();
            save_cached_lyrics(&conn, &key, "[00:01]fallback", "qq");

            // Same slot/write/invalidation sequence used by update_track_meta.
            // No R2 is started, including for the exact same title/artist/album.
            {
                let mut generation = slot.lock().unwrap();
                assert!(
                    db::apply_track_metadata(&conn, 7, edited_title, "artist", "album")
                        .unwrap()
                        .is_some()
                );
                *generation = generation.wrapping_add(1);
            }
            save_fetched_lyrics(&conn, Some(&pending), &key, "[00:01]obsolete", "netease");
            let mut published = false;
            assert!(with_current_fetch(Some(&pending), &conn, || published = true).is_none());
            assert!(!published);
            assert!(!persist_fetched_cover(
                &cover.0,
                &conn,
                Some(&pending),
                Some(7),
                None,
                true,
                &cover_png([0, 0, 255]),
            ));
            let art_path: Option<String> = conn
                .query_row("SELECT art_path FROM tracks WHERE id = 7", [], |r| r.get(0))
                .unwrap();
            assert!(art_path.is_none());
            assert_eq!(std::fs::read_dir(&cover.0.covers_dir).unwrap().count(), 0);
            if edited_title == "fixture" {
                assert_eq!(
                    cached_lyrics(&conn, &key),
                    Some(("[00:01]fallback".into(), "qq".into()))
                );
            } else {
                assert!(
                    cached_lyrics(&conn, &key).is_none(),
                    "must not recreate the edited-away cache"
                );
            }
        }
    }

    #[test]
    fn changed_or_removed_track_identity_blocks_publication_without_a_new_request() {
        for sql in [
            "UPDATE tracks SET title = 'changed' WHERE id = 7",
            "UPDATE tracks SET artist = 'changed' WHERE id = 7",
            "UPDATE tracks SET album = 'changed' WHERE id = 7",
            "UPDATE tracks SET duration = 241 WHERE id = 7",
            "DELETE FROM tracks WHERE id = 7",
        ] {
            let conn = track_db();
            let pending = fixture_gate(Arc::new(Mutex::new(0)));
            conn.execute(sql, []).unwrap();
            let key = cache_key("fixture", "artist");
            save_fetched_lyrics(&conn, Some(&pending), &key, "[00:01]obsolete", "qq");
            assert!(cached_lyrics(&conn, &key).is_none(), "{sql}");
            assert!(
                with_current_fetch(Some(&pending), &conn, || panic!("stale publish: {sql}"))
                    .is_none()
            );
        }
    }

    #[test]
    fn old_short_wrong_version_is_revalidated_without_deleting_fallback() {
        let conn = cache_db();
        let key = cache_key("霍元甲", "周杰伦");
        save_cached_lyrics(
            &conn,
            &key,
            "[00:12]fixture start\n[02:11.65]fixture end",
            "netease",
        );
        conn.execute("UPDATE meta_cache SET fetched_at=0", [])
            .unwrap();
        assert!(cached_lyrics_need_refresh(&conn, &key, 278.32));
        assert!(
            cached_lyrics(&conn, &key).is_some(),
            "offline fallback must remain"
        );
        save_cached_lyrics(
            &conn,
            &key,
            "[00:12]fixture start\n[04:07]fixture end",
            "qq",
        );
        assert!(!cached_lyrics_need_refresh(&conn, &key, 278.32));
    }

    #[test]
    fn timeline_heuristic_is_conservative_and_rate_limited() {
        let conn = cache_db();
        let key = "fixture|artist";
        let short = "[00:10]fixture start\n[01:10]fixture end";
        save_cached_lyrics(&conn, key, short, "qq");
        assert!(
            !cached_lyrics_need_refresh(&conn, key, 240.0),
            "fresh short timelines are not retried every play"
        );
        conn.execute("UPDATE meta_cache SET fetched_at=0", [])
            .unwrap();
        assert!(cached_lyrics_need_refresh(&conn, key, 240.0));
        assert!(
            !cached_lyrics_need_refresh(&conn, key, 90.0),
            "short tracks excluded"
        );
        assert!(!cached_lyrics_need_refresh(&conn, key, f64::NAN));
        assert!(!cached_lyrics_need_refresh(&conn, "missing", 240.0));
        save_cached_lyrics(
            &conn,
            key,
            "[00:10]fixture start\n[03:21.72]fixture end\n[03:43.54]",
            "netease",
        );
        conn.execute("UPDATE meta_cache SET fetched_at=0", [])
            .unwrap();
        assert!(
            !cached_lyrics_need_refresh(&conn, key, 227.0),
            "心墙-shaped timeline is not suspicious"
        );
    }

    #[test]
    fn lyrics_are_delivered_before_a_blocked_cover_finishes() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let (tx, rx) = tokio::sync::oneshot::channel();
            let result = tokio::time::timeout(
                Duration::from_secs(1),
                deliver_lyrics_before_cover(
                    Some((parse_lrc("[00:01]fixture"), "fixture".into())),
                    Some(move |lines: &[LyricLineDto], source: &str| {
                        assert_eq!(lines.len(), 1);
                        assert_eq!(source, "fixture");
                        tx.send(()).unwrap();
                    }),
                    async move {
                        // If production awaits cover before notifying, this never
                        // resolves and the timeout makes the regression falsifiable.
                        rx.await.unwrap();
                        false // failed/slow cover must not discard the lyrics
                    },
                ),
            )
            .await
            .unwrap()
            .unwrap();
            assert!(!result.cover_saved);
            assert_eq!(result.lyrics.unwrap().0.len(), 1);
        });
    }

    #[test]
    #[ignore = "live provider acceptance; scratch DB only, no user data writes"]
    fn live_huoyuanjia_replaces_suspicious_cache_in_scratch_db() {
        let dir =
            std::env::temp_dir().join(format!("sonora-live-{}-{}", std::process::id(), now_ms()));
        std::fs::create_dir(&dir).unwrap();
        let ctx = FetchCtx {
            db_path: dir.join("library.db"),
            covers_dir: dir.join("covers"),
        };
        let conn = db::open(&ctx.db_path).unwrap();
        let key = cache_key("霍元甲", "周杰伦");
        save_cached_lyrics(
            &conn,
            &key,
            "[00:12]fixture start\n[02:11.65]fixture end",
            "netease",
        );
        conn.execute("UPDATE meta_cache SET fetched_at=0", [])
            .unwrap();
        let result = tokio::runtime::Runtime::new().unwrap().block_on(fetch_core(
            &ctx,
            None,
            "霍元甲",
            "周杰伦",
            Some("霍元甲"),
            278.32,
            None,
            FetchOpts {
                want_lyrics: true,
                ..Default::default()
            },
        ));
        let (lines, source) = result
            .expect("providers returned no replacement")
            .lyrics
            .unwrap();
        let last = lines.iter().map(|line| line.t).reduce(f64::max).unwrap();
        eprintln!(
            "HUOYUANJIA source={source} lines={} last={last:.2}s scratch={}",
            lines.len(),
            dir.display()
        );
        assert!(
            last > 200.0,
            "replacement must advance beyond old 2:11 timeline"
        );
        assert!(!cached_lyrics_need_refresh(&conn, &key, 278.32));
        // Keep the isolated evidence DB in the OS temp directory for inspection.
    }

    /// QQ returns an LRC with `[ti:]` / `[ar:]` / `[offset:0]` metadata lines;
    /// none of them may become a lyric line.
    #[test]
    fn parse_lrc_skips_qq_metadata_header() {
        let text = "[ti:潮汐来信]\n[ar:港湾四重奏/沿岸合唱团]\n[offset:0]\n\
                    [00:15.16]风从旧码头带来一封信\n[00:21.70]我在灯火亮起时把它打开";
        let lines = parse_lrc(text);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "风从旧码头带来一封信");
        assert!((lines[0].t - 15.16).abs() < 1e-6);
    }
}
