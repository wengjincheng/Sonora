import { fmtTime, useLibrary, usePlayer } from "../store";
import { IconNote, IconAlbums, IconPlayCircle, IconGear } from "./Icons";

const NAV_ITEMS = [
  { kind: "library", label: "曲库", Icon: IconNote },
  { kind: "albums", label: "专辑", Icon: IconAlbums },
  { kind: "nowplaying", label: "正在播放", Icon: IconPlayCircle },
  { kind: "settings", label: "设置", Icon: IconGear },
] as const;

export default function Sidebar() {
  const view = useLibrary((s) => s.view);
  const setView = useLibrary((s) => s.setView);
  const libState = useLibrary((s) => s.libState);
  const tick = usePlayer((s) => s.tick);

  return (
    <aside className="sidebar" data-tauri-drag-region="deep">
      <div className="brand" data-tauri-drag-region>
        <span className="brand-icon" data-tauri-drag-region>
          <IconNote size={18} />
        </span>
        <span className="brand-name" data-tauri-drag-region>Sonora</span>
      </div>
      <nav className="nav">
        {NAV_ITEMS.map(({ kind, label, Icon }) => (
          <button
            key={kind}
            className={`nav-item ${view.kind === kind ? "active" : ""}`}
            onClick={() => setView({ kind } as never)}
          >
            <span className="nav-icon">
              <Icon size={18} />
            </span>
            <span>{label}</span>
          </button>
        ))}
      </nav>
      <div className="sidebar-footer">
        {libState?.scanning ? (
          <div className="scanning-hint">正在扫描曲库…</div>
        ) : (
          <div className="scanning-hint subtle">
            {libState ? `${libState.tracks} 首歌曲 · ${libState.albums} 张专辑` : ""}
          </div>
        )}
        {tick?.track && (
          <button
            type="button"
            className="sidebar-nowplaying"
            onClick={() => setView({ kind: "nowplaying" })}
          >
            <span className="eq" data-playing={tick.playing}>
              <i /><i /><i />
            </span>
            <span className="np-title">{tick.track.title}</span>
          </button>
        )}
      </div>
    </aside>
  );
}

export { fmtTime };
