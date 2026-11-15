import CoverImg from "../components/CoverImg";
import { IconAlbums } from "../components/Icons";
import { useLibrary } from "../store";

export default function AlbumsView() {
  const albums = useLibrary((s) => s.albums);
  const setView = useLibrary((s) => s.setView);

  return (
    <div className="view">
      <header className="view-header" data-tauri-drag-region>
        <h1 data-tauri-drag-region>专辑</h1>
      </header>
      {albums.length === 0 ? (
        <div className="empty-state">
          <div className="empty-icon">
            <IconAlbums size={32} />
          </div>
          <p>暂无专辑</p>
          <p className="sub">扫描完成后按专辑整理</p>
        </div>
      ) : (
        <div className="album-grid">
          {albums.map((a) => (
            <button
              key={a.key}
              className="album-card"
              onClick={() => setView({ kind: "album", key: a.key, name: a.name, artist: a.artist })}
            >
              <CoverImg path={a.cover} version={a.cover_version} className="album-cover" rounded={8} />
              <div className="album-name" title={a.name}>{a.name}</div>
              <div className="album-artist" title={a.artist}>
                {a.artist} · {a.track_count} 首
              </div>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
