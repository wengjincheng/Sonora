//! Audio engine: owns the output device, the current track, the play queue and
//! the playback mode. Runs on its own thread; other components talk to it via
//! an mpsc command channel and listen to `player://tick` events (200ms).
//!
//! Position tracking: rodio's `Sink::get_pos` is deprecated and does not
//! account for seeks, so a counting source wrapper counts samples actually
//! pulled by the output device. `position = base + samples / (rate*channels)`,
//! where `base` is re-anchored on every seek.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rand::seq::SliceRandom;
use rodio::mixer::Mixer;
use rodio::source::SeekError;
use rodio::{Decoder, OutputStream, Sink, Source};
use rusqlite::Connection;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::broadcast;

use crate::db;
use crate::dto::{LyricLineDto, PlayerTrackDto, TickDto};
use crate::metadata::{
    cache_key, cached_lyrics, cached_lyrics_need_refresh, parse_by_provider, parse_lrc,
};
use crate::state::AppContext;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PlayMode {
    Order,
    Shuffle,
    LoopAll,
    LoopOne,
}

impl PlayMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PlayMode::Order => "order",
            PlayMode::Shuffle => "shuffle",
            PlayMode::LoopAll => "loop_all",
            PlayMode::LoopOne => "loop_one",
        }
    }
    pub fn parse(s: &str) -> Self {
        match s {
            "shuffle" => PlayMode::Shuffle,
            "loop_all" => PlayMode::LoopAll,
            "loop_one" => PlayMode::LoopOne,
            _ => PlayMode::Order,
        }
    }
    pub fn cycle(self) -> Self {
        match self {
            PlayMode::Order => PlayMode::Shuffle,
            PlayMode::Shuffle => PlayMode::LoopAll,
            PlayMode::LoopAll => PlayMode::LoopOne,
            PlayMode::LoopOne => PlayMode::Order,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            PlayMode::Order => "顺序播放",
            PlayMode::Shuffle => "随机播放",
            PlayMode::LoopAll => "列表循环",
            PlayMode::LoopOne => "单曲循环",
        }
    }
}

pub enum EngineCmd {
    /// replace the queue and start playing at `start` (queue index)
    PlayQueue {
        ids: Vec<i64>,
        start: usize,
    },
    /// jump to a queue index
    Jump(usize),
    Play,
    Pause,
    Toggle,
    Next,
    Prev,
    /// absolute seek in seconds
    Seek(f64),
    /// relative seek in seconds (media keys)
    Nudge(f64),
    SetVolume(f64),
    SetMode(PlayMode),
    CycleMode,
    SetLyrics {
        track_id: i64,
        lines: Vec<LyricLineDto>,
        source: String,
    },
    /// A failed refresh must clear pending without erasing an offline fallback.
    LyricsFetchFinished {
        track_id: i64,
    },
    /// internal: result of the one-shot seek thread
    SeekDone {
        target: f64,
        ok: bool,
    },
    /// re-read the current track row (e.g. after a cover was fetched)
    ReloadCurrent,
}

#[derive(Clone)]
struct CurrentTrack {
    id: i64,
    path: PathBuf,
    dto: PlayerTrackDto,
}

#[derive(Clone)]
struct LyricsState {
    lines: Vec<LyricLineDto>,
    #[allow(dead_code)]
    source: String,
}

/// Wraps a source and counts interleaved samples delivered to the output
/// device — the actual audio clock.
pub(crate) struct CountingSource<S> {
    inner: S,
    counter: Arc<AtomicU64>,
}

impl<S> CountingSource<S> {
    pub(crate) fn new(inner: S, counter: Arc<AtomicU64>) -> Self {
        Self { inner, counter }
    }
}

impl<S: Source> Iterator for CountingSource<S> {
    type Item = S::Item;
    fn next(&mut self) -> Option<Self::Item> {
        let v = self.inner.next();
        if v.is_some() {
            self.counter.fetch_add(1, Ordering::Relaxed);
        }
        v
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<S: Source> Source for CountingSource<S> {
    fn current_span_len(&self) -> Option<usize> {
        self.inner.current_span_len()
    }
    fn channels(&self) -> u16 {
        self.inner.channels()
    }
    fn sample_rate(&self) -> u32 {
        self.inner.sample_rate()
    }
    fn total_duration(&self) -> Option<Duration> {
        self.inner.total_duration()
    }
    fn try_seek(&mut self, pos: Duration) -> Result<(), SeekError> {
        self.inner.try_seek(pos)
    }
}

struct Engine {
    app: AppHandle,
    db: Connection,
    tx: std::sync::mpsc::Sender<EngineCmd>,
    rx: Receiver<EngineCmd>,
    ticks: broadcast::Sender<TickDto>,

    _stream: Option<OutputStream>,
    mixer: Option<Mixer>,
    /// Arc so a one-shot seek thread can hold a handle (Sink isn't Clone)
    sink: Option<Arc<Sink>>,
    counter: Arc<AtomicU64>,
    rate: u32,
    channels: u16,
    base_pos: f64,
    stopped_pos: Option<f64>,
    /// latest seek target, used to drop stale SeekDone results
    last_seek_target: Option<f64>,

    current: Option<CurrentTrack>,
    duration: f64,
    queue: Vec<i64>,
    /// play sequence: `order[pos]` is the queue index of the current track
    order: Vec<usize>,
    pos: usize,
    mode: PlayMode,
    volume: f64,

    lyrics: Option<LyricsState>,
    lyrics_pending: bool,
}

pub fn spawn_engine(
    app: AppHandle,
    rx: Receiver<EngineCmd>,
    tx: std::sync::mpsc::Sender<EngineCmd>,
    ticks: broadcast::Sender<TickDto>,
) {
    std::thread::Builder::new()
        .name("audio-engine".into())
        .spawn(move || {
            let db_path = {
                let ctx = app.state::<AppContext>();
                let p = ctx.db_path.clone();
                drop(ctx);
                p
            };
            let conn = match db::open(&db_path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("[engine] cannot open db: {e}");
                    loop {
                        if rx.recv().is_err() {
                            return;
                        }
                    }
                }
            };
            let device = rodio::OutputStreamBuilder::open_default_stream();
            let (_stream, mixer) = match device {
                Ok(s) => {
                    let m = s.mixer().clone();
                    (Some(s), Some(m))
                }
                Err(e) => {
                    eprintln!("[engine] no audio output device: {e}");
                    (None, None)
                }
            };
            let mut engine = Engine {
                app: app.clone(),
                db: conn,
                tx,
                rx,
                ticks,
                _stream,
                mixer,
                sink: None,
                counter: Arc::new(AtomicU64::new(0)),
                rate: 44100,
                channels: 2,
                base_pos: 0.0,
                stopped_pos: None,
                last_seek_target: None,
                current: None,
                duration: 0.0,
                queue: Vec::new(),
                order: Vec::new(),
                pos: 0,
                mode: PlayMode::Order,
                volume: 0.8,
                lyrics: None,
                lyrics_pending: false,
            };
            engine.run();
        })
        .expect("failed to spawn audio engine thread");
}

impl Engine {
    fn run(&mut self) {
        self.restore();
        let mut last_tick = Instant::now();
        loop {
            match self.rx.recv_timeout(Duration::from_millis(50)) {
                Ok(cmd) => self.handle(cmd),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            }
            if last_tick.elapsed() >= Duration::from_millis(200) {
                self.on_tick();
                last_tick = Instant::now();
            }
        }
    }

    // ---------- state helpers ----------

    fn playing(&self) -> bool {
        self.sink.as_ref().map(|s| !s.is_paused()).unwrap_or(false)
    }

    fn position(&self) -> f64 {
        if let Some(sp) = self.stopped_pos {
            return sp.min(self.duration);
        }
        let samples = self.counter.load(Ordering::Relaxed) as f64;
        let denom = (self.rate.max(1) * self.channels.max(1) as u32) as f64;
        (self.base_pos + samples / denom).min(self.duration)
    }

    fn queue_index(&self) -> usize {
        self.order.get(self.pos).copied().unwrap_or(0)
    }

    fn current_lyric_text(&self) -> Option<String> {
        let st = self.lyrics.as_ref()?;
        if st.lines.is_empty() {
            return None;
        }
        let pos = self.position();
        let mut lo = 0usize;
        let mut hi = st.lines.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            if st.lines[mid].t <= pos {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        st.lines.get(lo.saturating_sub(1)).map(|l| l.text.clone())
    }

    fn build_tick(&self) -> TickDto {
        TickDto {
            playing: self.playing(),
            position: self.position(),
            duration: self.duration,
            volume: self.volume,
            mode: self.mode.as_str().to_string(),
            index: if self.current.is_some() {
                self.queue_index() as i64
            } else {
                -1
            },
            queue_len: self.queue.len(),
            track: self.current.as_ref().map(|c| c.dto.clone()),
            lyric_line: self.current_lyric_text(),
        }
    }

    fn on_tick(&mut self) {
        let ended = self
            .sink
            .as_ref()
            .map(|s| !s.is_paused() && s.empty())
            .unwrap_or(false);
        if ended {
            self.track_ended();
        }
        let tick = self.build_tick();
        let _ = self.ticks.send(tick.clone());
        let _ = self.app.emit("player://tick", &tick);
    }

    // ---------- persistence ----------

    fn persist(&self) {
        let ids = serde_json::to_string(&self.queue).unwrap_or_default();
        db::set_setting(&self.db, "player.queue", &ids);
        db::set_setting(&self.db, "player.pos", &self.queue_index().to_string());
        db::set_setting(&self.db, "player.mode", self.mode.as_str());
        db::set_setting(&self.db, "player.volume", &self.volume.to_string());
    }

    fn restore(&mut self) {
        if let Some(v) = db::get_setting(&self.db, "player.queue") {
            if let Ok(ids) = serde_json::from_str::<Vec<i64>>(&v) {
                self.queue = ids;
            }
        }
        if let Some(v) = db::get_setting(&self.db, "player.mode") {
            self.mode = PlayMode::parse(&v);
        }
        if let Some(v) = db::get_setting(&self.db, "player.volume") {
            if let Ok(f) = v.parse::<f64>() {
                self.volume = f.clamp(0.0, 1.0);
            }
        }
        self.build_order(None);
        let qidx = db::get_setting(&self.db, "player.pos")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        self.pos = self.order.iter().position(|&i| i == qidx).unwrap_or(0);
        // load the last track without starting playback
        if !self.queue.is_empty() {
            let q = self.order[self.pos.min(self.order.len() - 1)];
            let id = self.queue[q];
            if let Some(row) = Self::load_track_row(&self.db, id) {
                self.duration = row.dto.duration.max(0.1);
                self.current = Some(row);
                self.load_lyrics(false);
            }
        }
    }

    // ---------- queue ----------

    fn build_order(&mut self, first: Option<usize>) {
        let n = self.queue.len();
        let mut idx: Vec<usize> = (0..n).collect();
        if self.mode == PlayMode::Shuffle && n > 1 {
            idx.shuffle(&mut rand::thread_rng());
            if let Some(f) = first {
                idx.retain(|&i| i != f);
                idx.insert(0, f);
            }
        }
        self.order = idx;
    }

    fn play_at(&mut self, opos: usize) {
        if opos >= self.order.len() {
            return;
        }
        self.pos = opos;
        let qidx = self.order[opos];
        let id = self.queue[qidx];
        self.start_track(id);
        self.persist();
    }

    fn start_track(&mut self, id: i64) {
        let Some(row) = Self::load_track_row(&self.db, id) else {
            return;
        };
        self.sink = None;
        self.current = Some(row);
        let cur = self.current.as_ref().unwrap().clone();
        self.duration = cur.dto.duration.max(0.1);
        self.base_pos = 0.0;
        self.stopped_pos = None;
        self.counter = Arc::new(AtomicU64::new(0));
        self.load_lyrics(true);

        if let Some(mixer) = &self.mixer {
            if let Ok(file) = File::open(&cur.path) {
                // try_from(File): fills byte_len + seekable from metadata,
                // which symphonia's FLAC/MP3 seek binary-search requires
                match Decoder::try_from(file) {
                    Ok(dec) => {
                        self.rate = dec.sample_rate().max(1);
                        self.channels = dec.channels().max(1);
                        let sink = Arc::new(Sink::connect_new(mixer));
                        sink.set_volume(self.volume as f32);
                        sink.append(CountingSource::new(dec, self.counter.clone()));
                        self.sink = Some(sink);
                    }
                    Err(e) => eprintln!("[engine] cannot decode {}: {e}", cur.path.display()),
                }
            }
        }
    }

    fn track_ended(&mut self) {
        if self.mode == PlayMode::LoopOne {
            let p = self.pos;
            self.play_at(p);
            return;
        }
        if self.pos + 1 < self.order.len() {
            let p = self.pos + 1;
            self.play_at(p);
        } else if self.mode != PlayMode::Order && !self.order.is_empty() {
            self.play_at(0);
        } else {
            self.stop_at_end();
        }
    }

    fn next_manual(&mut self) {
        if self.order.is_empty() {
            return;
        }
        if self.pos + 1 < self.order.len() {
            let p = self.pos + 1;
            self.play_at(p);
        } else if self.mode != PlayMode::Order {
            self.play_at(0);
        } else {
            self.stop_at_end();
        }
    }

    fn prev_manual(&mut self) {
        if self.order.is_empty() {
            return;
        }
        if self.position() > 3.0 {
            self.do_seek(0.0);
            if !self.playing() {
                // paused at end: restart cleanly
                let p = self.pos;
                self.play_at(p);
                self.do_pause();
            }
            return;
        }
        if self.pos > 0 {
            let p = self.pos - 1;
            self.play_at(p);
        } else if self.mode != PlayMode::Order {
            let p = self.order.len() - 1;
            self.play_at(p);
        } else {
            self.do_seek(0.0);
        }
    }

    fn stop_at_end(&mut self) {
        self.sink = None;
        self.stopped_pos = Some(self.duration);
    }

    fn do_play(&mut self) {
        if let Some(s) = &self.sink {
            if s.is_paused() {
                s.play();
                self.stopped_pos = None;
            }
            return;
        }
        if self.current.is_some() {
            let p = self.pos;
            self.play_at(p);
        }
    }

    fn do_pause(&mut self) {
        if let Some(s) = &self.sink {
            s.pause();
        }
    }

    fn do_seek(&mut self, p: f64) {
        if self.current.is_none() {
            return;
        }
        let p = p.clamp(0.0, (self.duration - 0.05).max(0.0));
        // optimistic re-anchor so the UI reacts instantly; the actual
        // (blocking) sink seek happens on a one-shot thread — Sink::try_seek
        // waits on the audio callback and must not block the engine loop
        // (it can stall indefinitely if the sink is paused).
        self.base_pos = p;
        self.counter.store(0, Ordering::Relaxed);
        self.stopped_pos = None;
        self.last_seek_target = Some(p);
        if let Some(sink) = self.sink.clone() {
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let ok = sink.try_seek(Duration::from_secs_f64(p)).is_ok();
                let _ = tx.send(EngineCmd::SeekDone { target: p, ok });
            });
        }
    }

    // ---------- lyrics ----------

    fn load_lyrics(&mut self, allow_online: bool) {
        self.lyrics = None;
        self.lyrics_pending = false;
        let Some(cur) = self.current.clone() else {
            return;
        };
        // 1. sibling .lrc / .LRC file
        for lrc in [
            cur.path.with_extension("lrc"),
            cur.path.with_extension("LRC"),
        ] {
            if lrc.exists() {
                if let Ok(text) = std::fs::read_to_string(&lrc) {
                    let lines = parse_lrc(&text);
                    if !lines.is_empty() {
                        eprintln!("[lyrics] using sibling file {}", lrc.display());
                        self.lyrics = Some(LyricsState {
                            lines,
                            source: "file".into(),
                        });
                        break;
                    }
                }
            }
        }
        // 2. local cache (keep even a suspicious old match as a failure/offline
        // fallback; online playback will revalidate it without deleting it).
        let mut refresh_cached_lyrics = false;
        if self.lyrics.is_none() {
            let key = cache_key(&cur.dto.title, &cur.dto.artist);
            if let Some((text, provider)) = cached_lyrics(&self.db, &key) {
                let lines = parse_by_provider(&text, &provider);
                if !lines.is_empty() {
                    refresh_cached_lyrics =
                        cached_lyrics_need_refresh(&self.db, &key, cur.dto.duration);
                    eprintln!(
                        "[lyrics] cache hit ({provider}, refresh={refresh_cached_lyrics}) for {}",
                        cur.dto.title
                    );
                    self.lyrics = Some(LyricsState {
                        lines,
                        source: provider,
                    });
                }
            }
        }
        // 3. online — lyrics when missing, cover whenever the album has none
        //    (a .lrc file or cached lyrics must not block the cover fetch)
        let need_cover = cur.dto.cover.is_none();
        let need_lyrics = self.lyrics.is_none() || refresh_cached_lyrics;
        if allow_online && (need_lyrics || need_cover) {
            let online = self
                .app
                .state::<AppContext>()
                .settings
                .lock()
                .unwrap()
                .online_metadata;
            if online {
                eprintln!(
                    "[lyrics] online fetch (lyrics={need_lyrics}, cover={need_cover}) for {} - {}",
                    cur.dto.title, cur.dto.artist
                );
                self.lyrics_pending = need_lyrics;
                self.spawn_fetch(cur, need_lyrics);
            }
        }
    }

    fn spawn_fetch(&self, cur: CurrentTrack, want_lyrics: bool) {
        let app = self.app.clone();
        let tx = self.tx.clone();
        let id = cur.id;
        let title = cur.dto.title.clone();
        let artist = cur.dto.artist.clone();
        let album = cur.dto.album.clone();
        let duration = cur.dto.duration;
        let album_key = normalized_album_key(&self.db, cur.id);
        tauri::async_runtime::spawn(async move {
            let result = crate::metadata::fetch_for_track(
                &app,
                id,
                &title,
                &artist,
                Some(album.as_str()),
                duration,
                album_key.as_deref(),
                crate::metadata::FetchOpts {
                    want_lyrics,
                    ..Default::default()
                },
            )
            .await;
            // fetch_for_track delivers lyrics before cover work, not here.
            if want_lyrics {
                let _ = tx.send(EngineCmd::LyricsFetchFinished { track_id: id });
            }
            if result.is_some_and(|r| r.cover_saved) {
                let _ = tx.send(EngineCmd::ReloadCurrent);
                crate::scanner::emit_library_state(&app);
            }
        });
    }

    // ---------- db ----------

    fn load_track_row(conn: &Connection, id: i64) -> Option<CurrentTrack> {
        conn.query_row(
            "SELECT t.path, t.title, t.artist, t.album, t.duration,
                    COALESCE(t.art_path, a.art_path), (t.art_path IS NOT NULL)
             FROM tracks t LEFT JOIN albums a ON a.key = t.album_key
             WHERE t.id = ?1",
            [id],
            |r| {
                let path: String = r.get(0)?;
                let cover: Option<String> = r.get(5)?;
                let blur = cover.as_deref().map(|c| {
                    crate::images::blur_of(Path::new(c))
                        .to_string_lossy()
                        .to_string()
                });
                Ok(CurrentTrack {
                    id,
                    path: PathBuf::from(path),
                    dto: PlayerTrackDto {
                        id,
                        title: r.get(1)?,
                        artist: r.get(2)?,
                        album: r.get(3)?,
                        duration: r.get(4)?,
                        cover_version: cover
                            .as_deref()
                            .and_then(|p| crate::images::file_version(Path::new(p))),
                        blur_version: blur
                            .as_deref()
                            .and_then(|p| crate::images::file_version(Path::new(p))),
                        blur,
                        cover,
                        has_track_cover: r.get::<_, Option<bool>>(6)?.unwrap_or(false),
                    },
                })
            },
        )
        .ok()
    }

    // ---------- command handling ----------

    fn handle(&mut self, cmd: EngineCmd) {
        match cmd {
            EngineCmd::PlayQueue { ids, start } => {
                self.queue = ids;
                let start = start.min(self.queue.len().saturating_sub(1));
                self.build_order(Some(start));
                let opos = self.order.iter().position(|&i| i == start).unwrap_or(0);
                self.play_at(opos);
            }
            EngineCmd::Jump(qidx) => {
                if let Some(opos) = self.order.iter().position(|&i| i == qidx) {
                    self.play_at(opos);
                }
            }
            EngineCmd::Play => self.do_play(),
            EngineCmd::Pause => self.do_pause(),
            EngineCmd::Toggle => {
                if self.playing() {
                    self.do_pause();
                } else {
                    self.do_play();
                }
            }
            EngineCmd::Next => self.next_manual(),
            EngineCmd::Prev => self.prev_manual(),
            EngineCmd::Seek(p) => self.do_seek(p),
            EngineCmd::Nudge(d) => {
                let p = self.position() + d;
                self.do_seek(p);
            }
            EngineCmd::SetVolume(v) => {
                self.volume = v.clamp(0.0, 1.0);
                if let Some(s) = &self.sink {
                    s.set_volume(self.volume as f32);
                }
                self.persist();
            }
            EngineCmd::SetMode(m) => {
                let cur_q = self.queue_index();
                self.mode = m;
                self.build_order(if m == PlayMode::Shuffle {
                    Some(cur_q)
                } else {
                    None
                });
                self.pos = self.order.iter().position(|&i| i == cur_q).unwrap_or(0);
                self.persist();
            }
            EngineCmd::CycleMode => {
                let m = self.mode.cycle();
                self.handle(EngineCmd::SetMode(m));
            }
            EngineCmd::LyricsFetchFinished { track_id } => {
                if self.current.as_ref().map(|c| c.id) == Some(track_id) {
                    self.lyrics_pending = false;
                }
            }
            EngineCmd::SetLyrics {
                track_id,
                lines,
                source,
            } => {
                let is_current = self.current.as_ref().map(|c| c.id) == Some(track_id);
                if is_current {
                    self.lyrics_pending = false;
                    if lines.is_empty() {
                        self.lyrics = None;
                    } else {
                        self.lyrics = Some(LyricsState { lines, source });
                    }
                }
            }
            EngineCmd::SeekDone { target, ok } => {
                // re-anchor once the real seek finished (drops buffered-sample
                // drift); ignore results superseded by a newer seek
                if self.last_seek_target == Some(target) {
                    if ok {
                        self.base_pos = target;
                        self.counter.store(0, Ordering::Relaxed);
                    } else {
                        eprintln!("[engine] seek to {target:.1}s failed on the audio device");
                    }
                }
            }
            EngineCmd::ReloadCurrent => {
                if let Some(id) = self.current.as_ref().map(|c| c.id) {
                    if let Some(row) = Self::load_track_row(&self.db, id) {
                        if let Some(cur) = &mut self.current {
                            cur.dto = row.dto;
                        }
                    }
                }
            }
        }
    }
}

/// album_key of a track row, used by the fetch task to save covers
fn normalized_album_key(conn: &Connection, track_id: i64) -> Option<String> {
    conn.query_row(
        "SELECT album_key FROM tracks WHERE id = ?1",
        [track_id],
        |r| r.get::<_, String>(0),
    )
    .ok()
}
