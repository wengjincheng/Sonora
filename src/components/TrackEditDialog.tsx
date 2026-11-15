import { useEffect, useState } from "react";
import { api } from "../api";
import { useLibrary, useToast } from "../store";
import type { EditableTrack } from "../types";

const SOURCE_LABEL: Record<string, string> = {
  netease: "网易云音乐",
  qq: "QQ 音乐",
  lrclib: "LRCLIB",
  amll: "AMLL 逐字歌词",
  file: "本地文件",
};

export default function TrackEditDialog({
  track,
  onClose,
}: {
  track: EditableTrack | null;
  onClose: () => void;
}) {
  const [title, setTitle] = useState("");
  const [artist, setArtist] = useState("");
  const [album, setAlbum] = useState("");
  const [trackCover, setTrackCover] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!track) return;
    setTitle(track.title);
    setArtist(track.artist);
    setAlbum(track.album);
    setTrackCover(false);
    setBusy(false);
  }, [track]);

  useEffect(() => {
    if (!track) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [track, onClose]);

  if (!track) return null;

  const toast = (m: string) => useToast.getState().show(m);
  const reload = () => useLibrary.getState().reload();

  const persist = async () => {
    await api.updateTrackMeta(track.id, title.trim(), artist.trim(), album.trim());
  };

  const save = async () => {
    if (!title.trim()) return toast("标题不能为空");
    setBusy(true);
    try {
      await persist();
      await reload();
      toast("已保存，重扫后仍然有效");
      onClose();
    } catch (e) {
      toast(String(e));
    } finally {
      setBusy(false);
    }
  };

  const saveAndFetch = async () => {
    if (!title.trim()) return toast("标题不能为空");
    setBusy(true);
    try {
      await persist();
      const r = await api.refetchTrackMeta(track.id, true, trackCover);
      await reload();
      const bits: string[] = [];
      if (r.lyrics_source) {
        const src = SOURCE_LABEL[r.lyrics_source] ?? r.lyrics_source;
        bits.push(`已获取歌词（${src} · ${r.lyric_lines} 行）`);
      } else {
        bits.push("未找到歌词，原有内容保留");
      }
      if (r.cover_saved) bits.push("已更新单曲封面");
      toast(bits.join("，"));
      onClose();
    } catch (e) {
      toast(String(e));
    } finally {
      setBusy(false);
    }
  };

  const clearCover = async () => {
    setBusy(true);
    try {
      await api.clearTrackCover(track.id);
      await reload();
      toast("已恢复使用专辑封面");
      onClose();
    } catch (e) {
      toast(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      className="modal-backdrop"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div className="modal" role="dialog" aria-modal="true">
        <div className="modal-title">编辑歌曲信息</div>
        <label className="modal-field">
          <span>标题</span>
          <input
            value={title}
            autoFocus
            onChange={(e) => setTitle(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void saveAndFetch();
            }}
          />
        </label>
        <label className="modal-field">
          <span>艺人</span>
          <input value={artist} onChange={(e) => setArtist(e.target.value)} />
        </label>
        <label className="modal-field">
          <span>专辑</span>
          <input value={album} onChange={(e) => setAlbum(e.target.value)} />
        </label>
        <label className="modal-check">
          <input
            type="checkbox"
            checked={trackCover}
            onChange={(e) => setTrackCover(e.target.checked)}
          />
          <span>为这首歌单独匹配封面（不影响同专辑其他歌曲）</span>
        </label>
        <div className="modal-hint">
          封面默认跟随专辑；勾选后这首歌会有一张自己的图，可随时撤销。
        </div>
        {track.has_track_cover && (
          <button className="modal-link" disabled={busy} onClick={() => void clearCover()}>
            恢复使用专辑封面
          </button>
        )}
        <div className="modal-actions">
          <button className="ghost-btn" disabled={busy} onClick={onClose}>
            取消
          </button>
          <button className="ghost-btn" disabled={busy} onClick={() => void save()}>
            仅保存
          </button>
          <button
            className="primary-btn"
            style={{ marginTop: 0 }}
            disabled={busy}
            onClick={() => void saveAndFetch()}
          >
            {busy ? "处理中…" : "保存并重新获取"}
          </button>
        </div>
      </div>
    </div>
  );
}
