import { useMemo, useState } from "react";
import TrackList from "../components/TrackList";
import TrackEditDialog from "../components/TrackEditDialog";
import CoverImg from "../components/CoverImg";
import { IconChevronLeft, IconPlay } from "../components/Icons";
import { api } from "../api";
import { useLibrary, usePlayer } from "../store";
import type { EditableTrack } from "../types";

export default function AlbumView() {
  const view = useLibrary((s) => s.view);
  const tracks = useLibrary((s) => s.tracks);
  const albums = useLibrary((s) => s.albums);
  const setView = useLibrary((s) => s.setView);
  const activeId = usePlayer((s) => s.tick?.track?.id ?? null);
  const [editing, setEditing] = useState<EditableTrack | null>(null);
  const album = useMemo(
    () => (view.kind === "album" ? albums.find((a) => a.key === view.key) : undefined),
    [view, albums]
  );
  const cover = album?.cover ?? null;
  const ids = useMemo(() => tracks.map((t) => t.id), [tracks]);

  if (view.kind !== "album") return null;

  return (
    <div className="view">
      <header className="view-header" data-tauri-drag-region>
        <button className="back-btn" onClick={() => setView({ kind: "albums" })}>
          <IconChevronLeft size={16} />
          专辑
        </button>
      </header>
      <div className="album-hero">
        <CoverImg path={cover} version={album?.cover_version} className="album-hero-cover" rounded={10} />
        <div className="album-hero-meta">
          <div className="ah-name">{view.name}</div>
          <div className="ah-artist">{view.artist}</div>
          <div className="ah-count">{tracks.length} 首歌曲</div>
          <div className="setting-actions" style={{ marginTop: 0 }}>
            <button
              className="primary-btn"
              onClick={() => ids.length > 0 && api.playTracks(ids, 0)}
            >
              <IconPlay size={14} />
              播放
            </button>
          </div>
        </div>
      </div>
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
      <TrackEditDialog track={editing} onClose={() => setEditing(null)} />
    </div>
  );
}
