use serde::{Deserialize, Serialize};

#[derive(Serialize, Clone, Debug)]
pub struct TrackDto {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    pub track_no: Option<i64>,
    /// true when this track has its own cover instead of the album's
    pub has_track_cover: bool,
}

/// Result of a manual refetch, used for the UI toast.
#[derive(Serialize, Clone, Debug, Default)]
pub struct RefetchDto {
    pub lyrics_source: Option<String>,
    pub lyric_lines: usize,
    pub cover_saved: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct AlbumDto {
    pub key: String,
    pub name: String,
    pub artist: String,
    pub track_count: i64,
    pub cover: Option<String>,
    pub blur: Option<String>,
    /// File versions are separate from filesystem paths (asset URL cache bust).
    pub cover_version: Option<String>,
    pub blur_version: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct LibraryStateDto {
    pub tracks: i64,
    pub albums: i64,
    pub scanning: bool,
    pub music_dirs: Vec<String>,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct PlayerTrackDto {
    pub id: i64,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: f64,
    /// Source-file bits per sample; lossy formats may not define one.
    pub bit_depth: Option<u8>,
    /// Source-file sample rate in Hz (not the output device's rate).
    pub sample_rate: Option<u32>,
    pub cover: Option<String>,
    pub blur: Option<String>,
    /// File versions are separate from filesystem paths (asset URL cache bust).
    pub cover_version: Option<String>,
    pub blur_version: Option<String>,
    /// true when the track has its own cover instead of the album's
    pub has_track_cover: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct TickDto {
    pub playing: bool,
    pub position: f64,
    pub duration: f64,
    pub volume: f64,
    pub mode: String,
    pub index: i64,
    pub queue_len: usize,
    pub track: Option<PlayerTrackDto>,
    /// current lyric line text, computed engine-side (for the menubar ticker)
    pub lyric_line: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct WordDto {
    /// start seconds
    pub t: f64,
    /// duration seconds
    pub d: f64,
    pub text: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct LyricLineDto {
    pub t: f64,
    pub text: String,
    /// word-level timing (TTML / AMLL); None for plain LRC
    #[serde(skip_serializing_if = "Option::is_none")]
    pub words: Option<Vec<WordDto>>,
}

#[derive(Serialize, Clone, Debug)]
pub struct LyricsDto {
    pub track_id: i64,
    pub lines: Vec<LyricLineDto>,
    pub source: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SettingsDto {
    pub music_dirs: Vec<String>,
    pub menubar_lyrics: bool,
    pub menubar_max_chars: u32,
    pub lyrics_scale: u32,
    pub online_metadata: bool,
    pub menubar_only: bool,
}

impl Default for SettingsDto {
    fn default() -> Self {
        Self {
            music_dirs: Vec::new(),
            menubar_lyrics: true,
            menubar_max_chars: 24,
            lyrics_scale: 100,
            online_metadata: true,
            menubar_only: false,
        }
    }
}
