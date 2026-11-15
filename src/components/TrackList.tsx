import { useMemo, useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import type { TrackDto } from "../types";
import { fmtTime, usePlayer } from "../store";

export default function TrackList({
  tracks,
  activeId,
  onPlay,
  onEditTrack,
}: {
  tracks: TrackDto[];
  activeId?: number | null;
  onPlay: (index: number) => void;
  /** right-click a row to edit its metadata */
  onEditTrack?: (track: TrackDto) => void;
}) {
  const parentRef = useRef<HTMLDivElement>(null);
  const playing = usePlayer((s) => s.tick?.playing ?? false);
  const virtualizer = useVirtualizer({
    count: tracks.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => 44,
    overscan: 12,
  });

  const playingSet = useMemo(() => new Set([activeId ?? -1]), [activeId]);

  if (tracks.length === 0) {
    return <div className="empty-list">没有匹配的歌曲</div>;
  }

  return (
    <div className="tracklist" ref={parentRef}>
      <div className="tracklist-header">
        <span className="col-index">#</span>
        <span className="col-title">标题</span>
        <span className="col-artist">艺人</span>
        <span className="col-album">专辑</span>
        <span className="col-duration">时长</span>
      </div>
      <div
        style={{
          height: virtualizer.getTotalSize(),
          position: "relative",
          width: "100%",
        }}
      >
        {virtualizer.getVirtualItems().map((vi) => {
          const t = tracks[vi.index];
          const isActive = playingSet.has(t.id);
          return (
            <div
              className={`track-row ${isActive ? "active" : ""}`}
              key={t.id}
              style={{
                position: "absolute",
                top: 0,
                left: 0,
                width: "100%",
                height: vi.size,
                transform: `translateY(${vi.start}px)`,
              }}
              onDoubleClick={() => onPlay(vi.index)}
              onContextMenu={(e) => {
                if (!onEditTrack) return;
                e.preventDefault();
                onEditTrack(t);
              }}
            >
              <span className="col-index">
                {isActive ? (
                  <span className="row-playing">
                    <span className="eq" data-playing={playing}>
                      <i /><i /><i />
                    </span>
                  </span>
                ) : (
                  vi.index + 1
                )}
              </span>
              <span className="col-title" title={t.title}>{t.title}</span>
              <span className="col-artist" title={t.artist}>{t.artist}</span>
              <span className="col-album" title={t.album}>{t.album}</span>
              <span className="col-duration">{fmtTime(t.duration)}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}
