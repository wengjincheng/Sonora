import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import Sidebar from "./components/Sidebar";
import PlayerBar from "./components/PlayerBar";
import LibraryView from "./views/LibraryView";
import AlbumsView from "./views/AlbumsView";
import AlbumView from "./views/AlbumView";
import NowPlayingView from "./views/NowPlayingView";
import SettingsView from "./views/SettingsView";
import { api } from "./api";
import { useLibrary, usePlayer, useToast } from "./store";
import type { LyricsDto, SettingsDto, TickDto } from "./types";
import type { LibraryStateDto } from "./types";

function Toast() {
  const msg = useToast((s) => s.msg);
  if (!msg) return null;
  return <div className="toast">{msg}</div>;
}

/** in-window keyboard shortcuts (main window only) */
function useShortcuts() {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      if (
        t &&
        (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable)
      ) {
        return;
      }
      const tick = usePlayer.getState().tick;
      const pos = tick?.position ?? 0;
      const vol = tick?.volume ?? 0.8;
      switch (e.key) {
        case " ":
          e.preventDefault();
          void api.togglePlay();
          break;
        case "ArrowLeft":
          e.preventDefault();
          if (e.metaKey || e.ctrlKey) void api.prev();
          else void api.seek(Math.max(0, pos - 5));
          break;
        case "ArrowRight":
          e.preventDefault();
          if (e.metaKey || e.ctrlKey) void api.next();
          else void api.seek(pos + 5);
          break;
        case "ArrowUp":
          e.preventDefault();
          void api.setVolume(Math.min(1, vol + 0.05));
          break;
        case "ArrowDown":
          e.preventDefault();
          void api.setVolume(Math.max(0, vol - 0.05));
          break;
        case "l":
        case "L":
          if (e.metaKey || e.ctrlKey) {
            e.preventDefault();
            useLibrary.getState().setView({ kind: "nowplaying" });
          }
          break;
        case "f":
        case "F":
          if (e.metaKey || e.ctrlKey) {
            e.preventDefault();
            useLibrary.getState().setView({ kind: "library" });
            window.setTimeout(() => {
              document.querySelector<HTMLInputElement>(".search")?.focus();
            }, 50);
          }
          break;
        default:
          break;
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}

export default function App() {
  const view = useLibrary((s) => s.view);

  useEffect(() => {
    const unlisteners = [
      listen<TickDto>("player://tick", (e) => usePlayer.getState().setTick(e.payload)),
      listen<LibraryStateDto>("library://updated", () => {
        void useLibrary.getState().reload();
      }),
      listen<LyricsDto>("lyrics://updated", (e) => {
        const current = usePlayer.getState().tick?.track?.id;
        if (current === e.payload.track_id) {
          usePlayer.getState().setLyrics(e.payload);
        }
      }),
      listen<SettingsDto>("settings://updated", () => {
        // Re-read the current snapshot: concurrent command events can arrive
        // out of order after releasing the backend locks.
        void useLibrary.getState().refreshSettings();
      }).then((unlisten) => {
        void useLibrary.getState().refreshSettings();
        return unlisten;
      }),
      // drag & drop: folders (or audio files -> their parent folder)
      getCurrentWebviewWindow().onDragDropEvent((ev) => {
        if (ev.payload.type === "drop" && ev.payload.paths.length > 0) {
          void api.importPaths(ev.payload.paths).then((r) => {
            useToast.getState().show(r);
          });
        }
      }),
    ].map((p) => p.catch((error) => {
      useToast.getState().show(`窗口事件订阅失败：${String(error)}`);
      return () => {};
    }));
    void useLibrary.getState().reload();
    return () => {
      unlisteners.forEach((p) => void p.then((f) => f()));
    };
  }, []);

  useShortcuts();

  return (
    <div className="app">
      <Sidebar />
      <main className="content">
        {view.kind === "library" && <LibraryView />}
        {view.kind === "albums" && <AlbumsView />}
        {view.kind === "album" && <AlbumView />}
        {view.kind === "nowplaying" && <NowPlayingView />}
        {view.kind === "settings" && <SettingsView />}
      </main>
      <PlayerBar />
      <Toast />
    </div>
  );
}
