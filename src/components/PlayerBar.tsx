import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import { formatAudioQuality } from "./audioQuality";
import { coverUrl } from "./coverUrl";
import { fmtTime, MODE_LABEL, useLibrary, usePlayer } from "../store";
import {
  IconPause,
  IconPlay,
  IconNext,
  IconPrev,
  ModeIcon,
  IconVolHi,
  IconVolLo,
  IconVolZero,
} from "./Icons";

/** Smooth position between 200ms engine ticks (rAF interpolation).
 *  Displayed time only moves forward: a new tick can trail the interpolated
 *  value by the IPC latency, and letting it jump back makes the active lyric
 *  line flip-flop near boundaries. Large backward jumps (seeks / track change)
 *  still snap immediately. */
export function useInterpolatedPosition(): number {
  const tick = usePlayer((s) => s.tick);
  const anchor = useRef({ pos: 0, at: 0, playing: false });
  if (tick) {
    anchor.current = { pos: tick.position, at: performance.now(), playing: tick.playing };
  }
  const [pos, setPos] = useState(0);
  useEffect(() => {
    let raf = 0;
    const loop = () => {
      const c = anchor.current;
      const v = c.playing ? c.pos + (performance.now() - c.at) / 1000 : c.pos;
      setPos((p) => {
        if (v >= p) return v - p > 0.03 ? v : p;
        if (p - v > 0.75) return v;
        return p;
      });
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, []);
  return pos;
}

function SeekBar() {
  const tick = usePlayer((s) => s.tick);
  const pos = useInterpolatedPosition();
  const dur = tick?.duration ?? 0;
  const barRef = useRef<HTMLDivElement>(null);
  const [dragRatio, setDragRatio] = useState<number | null>(null);

  const ratioFromEvent = (clientX: number) => {
    const el = barRef.current;
    if (!el) return 0;
    const rect = el.getBoundingClientRect();
    return Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
  };

  const ratio = dragRatio ?? (dur > 0 ? pos / dur : 0);

  return (
    <div className="seekbar-wrap">
      <span className="time-label">{fmtTime(pos)}</span>
      <div
        className={`seekbar ${dragRatio !== null ? "dragging" : ""}`}
        ref={barRef}
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture(e.pointerId);
          setDragRatio(ratioFromEvent(e.clientX));
        }}
        onPointerMove={(e) => {
          if (dragRatio !== null) setDragRatio(ratioFromEvent(e.clientX));
        }}
        onPointerUp={(e) => {
          const r = ratioFromEvent(e.clientX);
          setDragRatio(null);
          if (dur > 0) api.seek(r * dur);
        }}
      >
        <div className="seekbar-fill" style={{ width: `${ratio * 100}%` }} />
        <div className="seekbar-knob" style={{ left: `${ratio * 100}%` }} />
      </div>
      <span className="time-label">{fmtTime(dur)}</span>
    </div>
  );
}

function VolumeIcon({ volume }: { volume: number }) {
  if (volume <= 0.001) return <IconVolZero size={16} />;
  if (volume < 0.5) return <IconVolLo size={16} />;
  return <IconVolHi size={16} />;
}

export default function PlayerBar() {
  const tick = usePlayer((s) => s.tick);
  const setView = useLibrary((s) => s.setView);
  const track = tick?.track ?? null;
  const mode = tick?.mode ?? "order";
  const volume = tick?.volume ?? 0.8;
  const quality = track ? formatAudioQuality(track) : "";

  return (
    <footer className="playerbar">
      <div
        className="pb-left"
        onClick={() => track && setView({ kind: "nowplaying" })}
        title="打开播放页"
      >
        {track ? (
          <>
            {track.cover ? (
              <img className="pb-cover" src={coverUrl(track.cover, track.cover_version)!} alt="" />
            ) : (
              <div className="cover-placeholder pb-cover"><span>♪</span></div>
            )}
            <div className="pb-meta">
              <div className="pb-title">{track.title}</div>
              <div className="pb-artist">{track.artist}</div>
              {quality && <div className="audio-quality">{quality}</div>}
            </div>
          </>
        ) : (
          <div className="pb-meta pb-idle">未在播放</div>
        )}
      </div>

      <div className="pb-center">
        <div className="pb-controls">
          <button className="ctl dim" title="上一首" onClick={() => api.prev()}>
            <IconPrev size={17} />
          </button>
          <button
            className="ctl main"
            title={tick?.playing ? "暂停" : "播放"}
            onClick={() => api.togglePlay()}
          >
            {tick?.playing ? <IconPause size={19} /> : <IconPlay size={19} />}
          </button>
          <button className="ctl dim" title="下一首" onClick={() => api.next()}>
            <IconNext size={17} />
          </button>
          <button
            className="ctl mode"
            title={MODE_LABEL[mode]}
            onClick={() => api.cycleMode()}
          >
            <ModeIcon mode={mode} size={16} />
          </button>
        </div>
        <SeekBar />
      </div>

      <div className="pb-right">
        <span className="vol-icon">
          <VolumeIcon volume={volume} />
        </span>
        <input
          className="vol-slider"
          type="range"
          min={0}
          max={100}
          value={Math.round(volume * 100)}
          style={{ ["--val" as string]: String(Math.round(volume * 100)) }}
          onChange={(e) => api.setVolume(Number(e.target.value) / 100)}
        />
      </div>
    </footer>
  );
}
