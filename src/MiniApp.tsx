// Mini controller popover shown under the menu-bar tray icon.
import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { api } from "./api";
import { coverUrl } from "./components/coverUrl";
import { useInterpolatedPosition } from "./components/PlayerBar";
import { fmtTime, MODE_LABEL, usePlayer } from "./store";
import type { LyricsDto, TickDto } from "./types";
import {
  IconPause,
  IconPlay,
  IconNext,
  IconPrev,
  ModeIcon,
  IconExpand,
} from "./components/Icons";

export default function MiniApp() {
  const tick = usePlayer((s) => s.tick);
  const lyrics = usePlayer((s) => s.lyrics);
  const pos = useInterpolatedPosition();
  const track = tick?.track ?? null;
  const dur = tick?.duration ?? 0;

  useEffect(() => {
    const uns = [
      listen<TickDto>("player://tick", (e) => usePlayer.getState().setTick(e.payload)),
      listen<LyricsDto>("lyrics://updated", (e) => {
        const cur = usePlayer.getState().tick?.track?.id;
        if (cur === e.payload.track_id) usePlayer.getState().setLyrics(e.payload);
      }),
    ];
    return () => uns.forEach((p) => p.then((f) => f()));
  }, []);

  // current + next lyric line
  let current: string | null = null;
  let next: string | null = null;
  if (lyrics && lyrics.lines.length > 0) {
    let idx = -1;
    for (let i = 0; i < lyrics.lines.length; i++) {
      if (lyrics.lines[i].t <= pos) idx = i;
      else break;
    }
    if (idx >= 0) current = lyrics.lines[idx].text;
    if (idx + 1 < lyrics.lines.length) next = lyrics.lines[idx + 1].text;
  }

  const ratio = dur > 0 ? Math.min(1, pos / dur) : 0;

  return (
    <div className="mini-app">
      <div className="mini-top">
        {track?.cover ? (
          <img className="mini-cover" src={coverUrl(track.cover, track.cover_version)!} alt="" onClick={() => api.showMain()} />
        ) : (
          <div className="cover-placeholder mini-cover" onClick={() => api.showMain()}>
            <span>♪</span>
          </div>
        )}
        <div className="mini-meta">
          <div className="mini-title">{track?.title ?? "未在播放"}</div>
          <div className="mini-artist">{track?.artist ?? ""}</div>
        </div>
        <button className="mini-open" title="打开主窗口" onClick={() => api.showMain()}>
          <IconExpand size={14} />
        </button>
      </div>

      <div className="mini-lyrics">
        <div className="ml-current">{current ?? track?.title ?? "Sonora"}</div>
        <div className="ml-next">{next ?? track?.artist ?? ""}</div>
      </div>

      <div className="mini-seek">
        <span className="mini-time">{fmtTime(pos)}</span>
        <div className="mini-seekbar" onClick={(e) => {
          const rect = e.currentTarget.getBoundingClientRect();
          const r = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
          if (dur > 0) api.seek(r * dur);
        }}>
          <div className="mini-seekbar-fill" style={{ width: `${ratio * 100}%` }} />
        </div>
        <span className="mini-time">{fmtTime(dur)}</span>
      </div>

      <div className="mini-controls">
        <button className="mctl" title="上一首" onClick={() => api.prev()}>
          <IconPrev size={18} />
        </button>
        <button className="mctl main" title={tick?.playing ? "暂停" : "播放"} onClick={() => api.togglePlay()}>
          {tick?.playing ? <IconPause size={20} /> : <IconPlay size={20} />}
        </button>
        <button className="mctl" title="下一首" onClick={() => api.next()}>
          <IconNext size={18} />
        </button>
        <button
          className="mctl mode"
          title={MODE_LABEL[tick?.mode ?? "order"]}
          onClick={() => api.cycleMode()}
        >
          <ModeIcon mode={tick?.mode ?? "order"} size={17} />
        </button>
      </div>
    </div>
  );
}
