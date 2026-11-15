import { useEffect, useMemo, useRef, useState } from "react";
import { api } from "../api";
import TrackEditDialog from "../components/TrackEditDialog";
import { coverUrl } from "../components/coverUrl";
import { useInterpolatedPosition } from "../components/PlayerBar";
import { fmtTime, useLibrary, usePlayer } from "../store";
import type { EditableTrack, LyricLineDto, WordDto } from "../types";

function activeLineIndex(lines: LyricLineDto[], pos: number): number {
  let lo = 0;
  let hi = lines.length;
  while (lo < hi) {
    const mid = (lo + hi) >> 1;
    if (lines[mid].t <= pos) lo = mid + 1;
    else hi = mid;
  }
  return lo - 1;
}

/** karaoke word fill: gradient split by elapsed fraction of the word */
function WordSpan({ w, pos }: { w: WordDto; pos: number }) {
  const d = w.d > 0 ? w.d : 0.25;
  const p = Math.min(1, Math.max(0, (pos - w.t) / d));
  if (p >= 1) return <span className="w w-done">{w.text}</span>;
  if (p <= 0) return <span className="w w-todo">{w.text}</span>;
  return (
    <span
      className="w w-partial"
      style={{ ["--p" as string]: `${(p * 100).toFixed(1)}%` }}
    >
      {w.text}
    </span>
  );
}

function LyricLineView({
  line,
  active,
  pos,
}: {
  line: LyricLineDto;
  active: boolean;
  pos: number;
}) {
  const hasWords = active && !!line.words && line.words.length > 0;
  return (
    <div className={`lyric-line ${active ? "active" : ""}`}>
      {hasWords
        ? line.words!.map((w, i) => <WordSpan key={i} w={w} pos={pos} />)
        : line.text}
    </div>
  );
}

function Lyrics() {
  const lyrics = usePlayer((s) => s.lyrics);
  const settings = useLibrary((s) => s.settings);
  const scale = settings?.lyrics_scale ?? 100;
  const saveScale = useLibrary((s) => s.saveLyricsScale);
  const adjustScale = (delta: number) => {
    saveScale((useLibrary.getState().settings?.lyrics_scale ?? 100) + delta);
  };
  const pos = useInterpolatedPosition();
  const containerRef = useRef<HTMLDivElement>(null);
  const lineRefs = useRef<(HTMLDivElement | null)[]>([]);

  const active = useMemo(
    () => (lyrics && lyrics.lines.length ? activeLineIndex(lyrics.lines, pos) : -1),
    [lyrics, pos]
  );

  useEffect(() => {
    if (active < 0) return;
    const el = lineRefs.current[active];
    const container = containerRef.current;
    if (!el || !container) return;
    const recenter = () => {
      const target = el.offsetTop - container.clientHeight / 2 + el.clientHeight / 2;
      container.scrollTo({ top: Math.max(0, target), behavior: "smooth" });
    };
    recenter();
    // Font-size transitions and wrapped lines continue changing geometry after
    // the effect runs. Observe layout through the final animation frame.
    const observer = new ResizeObserver(recenter);
    observer.observe(el);
    observer.observe(container);
    if (container.firstElementChild) observer.observe(container.firstElementChild);
    return () => observer.disconnect();
  }, [active, scale, lyrics]);

  if (!lyrics || lyrics.lines.length === 0) {
    return (
      <div className="lyrics-empty">
        <p>暂无歌词</p>
        <p className="sub">
          将同名 .lrc 文件放在歌曲旁，或右键歌曲「编辑歌曲信息」后重新获取
        </p>
      </div>
    );
  }

  return (
    <>
      <div className="lyrics" ref={containerRef} style={{ ["--lyrics-scale" as string]: scale / 100 }}>
        <div className="lyrics-inner">
          <div className="lyrics-pad" />
          {lyrics.lines.map((line, i) => (
            <div
              key={`${line.t}-${i}`}
              ref={(el) => {
                lineRefs.current[i] = el;
              }}
              className="lyric-row"
              onClick={() => api.seek(Math.max(0, line.t))}
            >
              <LyricLineView line={line} active={i === active} pos={pos} />
            </div>
          ))}
          <div className="lyrics-pad" />
        </div>
        <div className="lyrics-source">歌词来源：{sourceLabel(lyrics.source)}</div>
      </div>
      <div className="lyrics-tools" role="group" aria-label="歌词字号">
        <button
          type="button"
          title="缩小歌词字号"
          aria-label="缩小歌词字号"
          disabled={!settings || scale <= 80}
          onClick={() => adjustScale(-10)}
        >A−</button>
        <span aria-live="polite">{scale}%</span>
        <button
          type="button"
          title="放大歌词字号"
          aria-label="放大歌词字号"
          disabled={!settings || scale >= 300}
          onClick={() => adjustScale(10)}
        >A＋</button>
      </div>
    </>
  );
}

function sourceLabel(s: string): string {
  if (s === "netease") return "网易云音乐";
  if (s === "qq") return "QQ 音乐";
  if (s === "lrclib") return "LRCLIB";
  if (s === "amll") return "AMLL 逐字歌词";
  if (s === "file") return "本地文件";
  return s || "-";
}

export default function NowPlayingView() {
  const tick = usePlayer((s) => s.tick);
  const track = tick?.track ?? null;
  const blurUrl = coverUrl(track?.blur ?? null, track?.blur_version);
  const [editing, setEditing] = useState<EditableTrack | null>(null);

  return (
    <div className="view nowplaying">
      <div className="np-drag" data-tauri-drag-region />
      {blurUrl && <img className="np-bg" src={blurUrl} alt="" />}
      <div className="np-overlay" />
      <div className="np-body">
        <div className="np-left">
          {track?.cover ? (
            <img className="np-cover" src={coverUrl(track.cover, track.cover_version)!} alt="" />
          ) : (
            <div className="cover-placeholder np-cover"><span>♪</span></div>
          )}
          <div className="np-meta">
            <div className="np-title">{track?.title ?? "未在播放"}</div>
            <div className="np-artist">{track?.artist ?? ""}</div>
            <div className="np-album">{track?.album ?? ""}</div>
            {track && (
              <button
                className="ghost-btn np-edit"
                onClick={() =>
                  setEditing({
                    id: track.id,
                    title: track.title,
                    artist: track.artist,
                    album: track.album,
                    has_track_cover: track.has_track_cover,
                  })
                }
              >
                编辑歌曲信息
              </button>
            )}
            <div className="np-time">
              {fmtTime(tick?.position ?? 0)} / {fmtTime(tick?.duration ?? 0)}
            </div>
          </div>
        </div>
        <div className="np-right">
          <Lyrics />
        </div>
      </div>
      <TrackEditDialog track={editing} onClose={() => setEditing(null)} />
    </div>
  );
}
