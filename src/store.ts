import { create } from "zustand";
import { api } from "./api";
import type {
  AlbumDto,
  LibraryStateDto,
  LyricsDto,
  SettingsDto,
  TickDto,
  TrackDto,
} from "./types";

// ---------------- player ----------------

interface PlayerStore {
  tick: TickDto | null;
  lyrics: LyricsDto | null;
  setTick: (tick: TickDto) => void;
  setLyrics: (lyrics: LyricsDto | null) => void;
}

let lyricsRevision = 0;

export const usePlayer = create<PlayerStore>((set) => ({
  tick: null,
  lyrics: null,
  setTick: (tick) => {
    const prevTrackId = usePlayer.getState().tick?.track?.id ?? null;
    const trackId = tick.track?.id ?? null;
    set({ tick });
    if (trackId !== prevTrackId) {
      const revision = ++lyricsRevision;
      set({ lyrics: null });
      if (trackId !== null) {
        void api.getLyrics(trackId).then((lyrics) => {
          if (usePlayer.getState().tick?.track?.id === trackId && revision === lyricsRevision) {
            usePlayer.getState().setLyrics(lyrics);
          }
        }).catch((error) => {
          if (usePlayer.getState().tick?.track?.id === trackId && revision === lyricsRevision) {
            useToast.getState().show(`读取歌词失败：${String(error)}`);
          }
        });
      }
    }
  },
  setLyrics: (lyrics) => {
    lyricsRevision++;
    set({ lyrics });
  },
}));

// ---------------- library / navigation ----------------

export type View =
  | { kind: "library" }
  | { kind: "albums" }
  | { kind: "settings" }
  | { kind: "nowplaying" }
  | { kind: "album"; key: string; name: string; artist: string };

interface LibraryStore {
  libState: LibraryStateDto | null;
  settings: SettingsDto | null;
  tracks: TrackDto[];
  albums: AlbumDto[];
  search: string;
  view: View;
  setSearch: (s: string) => void;
  setView: (v: View) => void;
  reload: () => Promise<void>;
  refreshSettings: () => Promise<void>;
  setSettings: (settings: SettingsDto) => void;
  saveLyricsScale: (scale: number) => void;
}

// One ordered writer survives view changes. Keep the newest optimistic scale
// visible while older slider/button saves and settings reads finish.
let settingsRevision = 0;
let settingsRead = 0;
let scaleRequest = 0;
let pendingScale: number | null = null;
let savedScale = 100;
let scaleSaves: Promise<void> = Promise.resolve();

export const useLibrary = create<LibraryStore>((set, get) => ({
  libState: null,
  settings: null,
  tracks: [],
  albums: [],
  search: "",
  view: { kind: "library" },
  setSearch: (search) => {
    set({ search });
    void get().reload();
  },
  setView: (view) => {
    set({ view });
    void get().reload();
  },
  reload: async () => {
    const { view, search } = get();
    const [libState] = await Promise.all([
      api.libraryState().catch(() => null),
      get().refreshSettings(),
    ]);
    let tracks: TrackDto[] = [];
    let albums: AlbumDto[] = [];
    if (view.kind === "library") {
      tracks = await api.listTracks({ q: search }).catch(() => []);
    } else if (view.kind === "album") {
      [tracks, albums] = await Promise.all([
        api.listTracks({ albumKey: view.key }).catch(() => []),
        api.listAlbums().catch(() => get().albums),
      ]);
    } else if (view.kind === "albums") {
      albums = await api.listAlbums().catch(() => []);
    }
    set({ libState, tracks, albums });
  },
  refreshSettings: async () => {
    const revision = settingsRevision;
    const request = ++settingsRead;
    const settings = await api.getSettings().catch(() => null);
    // A slow library/settings read must not undo a newer write or event.
    if (settings && request === settingsRead && revision === settingsRevision) get().setSettings(settings);
  },
  setSettings: (settings) => {
    savedScale = settings.lyrics_scale;
    settingsRevision++;
    set({ settings: pendingScale === null ? settings : { ...settings, lyrics_scale: pendingScale } });
  },
  saveLyricsScale: (value) => {
    const settings = get().settings;
    const scale = Math.min(300, Math.max(80, Math.round(value)));
    if (!settings || !Number.isFinite(scale) || settings.lyrics_scale === scale) return;
    const request = ++scaleRequest;
    pendingScale = scale;
    settingsRevision++;
    set({ settings: { ...settings, lyrics_scale: scale } });
    scaleSaves = scaleSaves.then(async () => {
      try {
        const snapshot = await api.setSetting("lyrics_scale", String(scale));
        if (request === scaleRequest) pendingScale = null;
        get().setSettings(snapshot);
      } catch (error) {
        if (request === scaleRequest) {
          pendingScale = null;
          settingsRevision++;
          const current = get().settings;
          if (current) set({ settings: { ...current, lyrics_scale: savedScale } });
        }
        useToast.getState().show(`歌词字号保存失败：${String(error)}`);
      }
    });
  },
}));

// ---------------- toast ----------------

interface ToastStore {
  msg: string | null;
  show: (msg: string) => void;
  clear: () => void;
}

export const useToast = create<ToastStore>((set) => ({
  msg: null,
  show: (msg) => {
    set({ msg });
    window.setTimeout(() => {
      if (useToast.getState().msg === msg) set({ msg: null });
    }, 2800);
  },
  clear: () => set({ msg: null }),
}));

// ---------------- helpers ----------------

export function fmtTime(sec: number): string {
  if (!Number.isFinite(sec) || sec < 0) return "0:00";
  const m = Math.floor(sec / 60);
  const s = Math.floor(sec % 60);
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export const MODE_LABEL: Record<string, string> = {
  order: "顺序播放",
  shuffle: "随机播放",
  loop_all: "列表循环",
  loop_one: "单曲循环",
};
