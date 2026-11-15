import { invoke } from "@tauri-apps/api/core";
import type {
  AlbumDto,
  LibraryStateDto,
  LyricsDto,
  RefetchDto,
  SettingsDto,
  TrackDto,
} from "./types";

export const api = {
  libraryState: () => invoke<LibraryStateDto>("library_state"),
  listTracks: (opts: { q?: string; albumKey?: string } = {}) =>
    invoke<TrackDto[]>("list_tracks", { q: opts.q ?? null, albumKey: opts.albumKey ?? null }),
  listAlbums: () => invoke<AlbumDto[]>("list_albums"),

  playTracks: (ids: number[], start: number) => invoke("play_tracks", { ids, start }),
  jumpTo: (index: number) => invoke("jump_to", { index }),
  togglePlay: () => invoke("toggle_play"),
  next: () => invoke("next"),
  prev: () => invoke("prev"),
  seek: (position: number) => invoke("seek", { position }),
  setVolume: (volume: number) => invoke("set_volume", { volume }),
  setMode: (mode: string) => invoke("set_mode", { mode }),
  cycleMode: () => invoke("cycle_mode"),

  getLyrics: (trackId: number) => invoke<LyricsDto | null>("get_lyrics", { trackId }),

  updateTrackMeta: (trackId: number, title: string, artist: string, album: string) =>
    invoke<void>("update_track_meta", { trackId, title, artist, album }),
  refetchTrackMeta: (trackId: number, wantLyrics: boolean, trackCover: boolean) =>
    invoke<RefetchDto>("refetch_track_meta", { trackId, wantLyrics, trackCover }),
  clearTrackCover: (trackId: number) => invoke<void>("clear_track_cover", { trackId }),

  getSettings: () => invoke<SettingsDto>("get_settings"),
  setSetting: (key: string, value: string) => invoke<SettingsDto>("set_setting", { key, value }),
  addMusicDir: (path: string) => invoke("add_music_dir", { path }),
  removeMusicDir: (path: string) => invoke("remove_music_dir", { path }),
  rescan: () => invoke("rescan"),
  clearMetaCache: () => invoke("clear_meta_cache"),
  metaCacheCount: () => invoke<number>("meta_cache_count"),
  refreshCovers: () => invoke("refresh_covers"),

  // windows
  showMain: () => invoke("show_main"),
  importPaths: (paths: string[]) => invoke<string>("import_paths", { paths }),
};
