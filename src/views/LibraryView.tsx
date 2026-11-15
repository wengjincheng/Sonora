import { useMemo, useState } from "react";
import TrackList from "../components/TrackList";
import TrackEditDialog from "../components/TrackEditDialog";
import { IconNote, IconSearch } from "../components/Icons";
import { api } from "../api";
import { useLibrary, usePlayer } from "../store";
import type { EditableTrack } from "../types";

export default function LibraryView() {
  const tracks = useLibrary((s) => s.tracks);
  const search = useLibrary((s) => s.search);
  const setSearch = useLibrary((s) => s.setSearch);
  const setView = useLibrary((s) => s.setView);
  const libState = useLibrary((s) => s.libState);
  const activeId = usePlayer((s) => s.tick?.track?.id ?? null);
  const [editing, setEditing] = useState<EditableTrack | null>(null);

  const ids = useMemo(() => tracks.map((t) => t.id), [tracks]);
  const noDirs = (libState?.music_dirs.length ?? 0) === 0 && !libState?.scanning;

  return (
    <div className="view">
      <header className="view-header" data-tauri-drag-region>
        <h1 data-tauri-drag-region>曲库</h1>
        <div className="search-wrap">
          <span className="search-ico">
            <IconSearch size={15} />
          </span>
          <input
            className="search"
            placeholder="搜索歌曲 / 艺人 / 专辑…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
      </header>

      {noDirs ? (
        <div className="empty-state">
          <div className="empty-icon">
            <IconNote size={32} />
          </div>
          <p>还没有音乐库</p>
          <p className="sub">添加一个存放音乐的文件夹，自动扫描标签、封面与歌词</p>
          <button className="primary-btn" onClick={() => setView({ kind: "settings" })}>
            前往设置添加文件夹
          </button>
        </div>
      ) : (
        <TrackList
          tracks={tracks}
          activeId={activeId}
          onPlay={(i) => api.playTracks(ids, i)}
          onEditTrack={(t) =>
            setEditing({
              id: t.id,
              title: t.title,
              artist: t.artist,
              album: t.album,
              has_track_cover: t.has_track_cover,
            })
          }
        />
      )}
      <TrackEditDialog track={editing} onClose={() => setEditing(null)} />
    </div>
  );
}
