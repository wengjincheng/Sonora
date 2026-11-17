// Mirrors src-tauri/src/dto.rs (serde keeps snake_case field names).

export interface TrackDto {
  id: number;
  title: string;
  artist: string;
  album: string;
  duration: number;
  track_no: number | null;
  /** true when this track has its own cover instead of the album's */
  has_track_cover: boolean;
}

/** Minimal shape needed by the edit dialog. */
export interface EditableTrack {
  id: number;
  title: string;
  artist: string;
  album: string;
  has_track_cover: boolean;
}

export interface AlbumDto {
  key: string;
  name: string;
  artist: string;
  track_count: number;
  cover: string | null;
  blur: string | null;
  cover_version?: string | null;
  blur_version?: string | null;
}

export interface LibraryStateDto {
  tracks: number;
  albums: number;
  scanning: boolean;
  music_dirs: string[];
}

export interface PlayerTrackDto {
  id: number;
  title: string;
  artist: string;
  album: string;
  duration: number;
  /** Source-file properties; lossy formats may have no fixed bit depth. */
  bit_depth: number | null;
  sample_rate: number | null;
  cover: string | null;
  blur: string | null;
  cover_version?: string | null;
  blur_version?: string | null;
  has_track_cover: boolean;
}

export interface TickDto {
  playing: boolean;
  position: number;
  duration: number;
  volume: number;
  mode: "order" | "shuffle" | "loop_all" | "loop_one";
  index: number;
  queue_len: number;
  track: PlayerTrackDto | null;
  lyric_line: string | null;
}

export interface WordDto {
  t: number;
  d: number;
  text: string;
}

export interface LyricLineDto {
  t: number;
  text: string;
  words?: WordDto[] | null;
}

export interface LyricsDto {
  track_id: number;
  lines: LyricLineDto[];
  source: string;
}

export interface SettingsDto {
  music_dirs: string[];
  menubar_lyrics: boolean;
  menubar_max_chars: number;
  lyrics_scale: number;
  online_metadata: boolean;
  menubar_only: boolean;
}

/** Result of a manual metadata refetch. */
export interface RefetchDto {
  lyrics_source: string | null;
  lyric_lines: number;
  cover_saved: boolean;
}
