import { useEffect, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { api } from "../api";
import { useLibrary, useToast } from "../store";
import { getTheme, setTheme, type Theme } from "../theme";

const THEMES: { value: Theme; label: string }[] = [
  { value: "auto", label: "跟随系统" },
  { value: "light", label: "浅色" },
  { value: "dark", label: "深色" },
];

function Toggle({
  checked,
  onChange,
  label,
  hint,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  hint?: string;
}) {
  return (
    <label className="setting-row">
      <div>
        <div className="setting-label">{label}</div>
        {hint && <div className="setting-hint">{hint}</div>}
      </div>
      <span className="switch">
        <input
          type="checkbox"
          checked={checked}
          onChange={(e) => onChange(e.target.checked)}
        />
        <span className="sw-track">
          <span className="sw-knob" />
        </span>
      </span>
    </label>
  );
}

export default function SettingsView() {
  const libState = useLibrary((s) => s.libState);
  const reload = useLibrary((s) => s.reload);
  const settings = useLibrary((s) => s.settings);
  const lyricsScale = settings?.lyrics_scale ?? 100;
  const saveLyricsScale = useLibrary((s) => s.saveLyricsScale);
  const [menubarLyrics, setMenubarLyrics] = useState(true);
  const [menubarMax, setMenubarMax] = useState(24);
  const [online, setOnline] = useState(true);
  const [menubarOnly, setMenubarOnly] = useState(false);
  const [autostart, setAutostart] = useState(false);
  const [cacheCount, setCacheCount] = useState(0);
  const [theme, setThemeState] = useState<Theme>(() => getTheme());

  useEffect(() => {
    void api.getSettings().then((s) => {
      setMenubarLyrics(s.menubar_lyrics);
      setMenubarMax(s.menubar_max_chars);
      setOnline(s.online_metadata);
      setMenubarOnly(s.menubar_only);
    }).catch((error) => useToast.getState().show(`读取设置失败：${String(error)}`));
    void api.metaCacheCount().then(setCacheCount);
    void isEnabled().then(setAutostart).catch(() => {});
  }, []);

  const set = (key: string, value: string) => void api.setSetting(key, value)
    .then(() => reload())
    .catch((error) => useToast.getState().show(`保存设置失败：${String(error)}`));

  const addFolder = async () => {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir === "string" && dir.length > 0) {
      await api.addMusicDir(dir);
      await reload();
    }
  };

  return (
    <div className="view settings">
      <header className="view-header" data-tauri-drag-region>
        <h1 data-tauri-drag-region>设置</h1>
      </header>

      <section className="setting-card">
        <h2>外观</h2>
        <div className="setting-row">
          <div>
            <div className="setting-label">界面外观</div>
            <div className="setting-hint">跟随系统深浅色，或固定使用其中一种</div>
          </div>
          <div className="segmented">
            {THEMES.map((t) => (
              <button
                key={t.value}
                type="button"
                className={`seg-btn ${theme === t.value ? "active" : ""}`}
                onClick={() => {
                  setThemeState(t.value);
                  setTheme(t.value);
                }}
              >
                {t.label}
              </button>
            ))}
          </div>
        </div>
        <div className="setting-row">
          <div>
            <label className="setting-label" htmlFor="lyrics-scale">歌词字号</label>
            <div className="setting-hint">调整播放页歌词，100% 为标准字号</div>
          </div>
          <div className="range-row">
            <input
              id="lyrics-scale"
              type="range"
              min={80}
              max={300}
              step={5}
              value={lyricsScale}
              disabled={!settings}
              aria-valuetext={`${lyricsScale}%`}
              style={{ ["--val" as string]: String(((lyricsScale - 80) / 220) * 100) }}
              onChange={(e) => saveLyricsScale(Number(e.target.value))}
            />
            <span className="range-value">{lyricsScale}%</span>
            <button type="button" className="ghost-btn" disabled={!settings || lyricsScale === 100} onClick={() => saveLyricsScale(100)}>恢复标准</button>
          </div>
        </div>
      </section>

      <section className="setting-card">
        <h2>音乐文件夹</h2>
        <div className="setting-hint">也可以直接把文件夹拖进窗口导入</div>
        {(libState?.music_dirs.length ?? 0) === 0 && (
          <div className="setting-hint">尚未添加任何文件夹</div>
        )}
        <div className="dir-list">
          {(libState?.music_dirs ?? []).map((d) => (
            <div className="dir-row" key={d}>
              <span className="dir-path" title={d}>{d}</span>
              <button
                className="ghost-btn danger"
                onClick={async () => {
                  await api.removeMusicDir(d);
                  await reload();
                }}
              >
                移除
              </button>
            </div>
          ))}
        </div>
        <div className="setting-actions">
          <button className="primary-btn" style={{ marginTop: 0 }} onClick={() => void addFolder()}>
            添加文件夹…
          </button>
          <button className="ghost-btn" onClick={() => void api.rescan()}>
            重新扫描
          </button>
          {libState?.scanning && <span className="scanning-hint">扫描中…</span>}
        </div>
      </section>

      <section className="setting-card">
        <h2>菜单栏</h2>
        <Toggle
          label="在菜单栏显示歌词"
          hint="播放时在 macOS 状态栏滚动显示当前歌词行"
          checked={menubarLyrics}
          onChange={(v) => {
            setMenubarLyrics(v);
            set("menubar_lyrics", String(v));
          }}
        />
        <div className="setting-row">
          <div>
            <div className="setting-label">歌词最大显示宽度</div>
            <div className="setting-hint">
              状态栏空间有限（刘海屏尤其），超长行将以跑马灯滚动
            </div>
          </div>
          <div className="range-row">
            <input
              type="range"
              min={8}
              max={60}
              value={menubarMax}
              style={{
                ["--val" as string]: String(
                  Math.round(((menubarMax - 8) / (60 - 8)) * 100)
                ),
              }}
              onChange={(e) => {
                const v = Number(e.target.value);
                setMenubarMax(v);
                set("menubar_max_chars", String(v));
              }}
            />
            <span className="range-value">{menubarMax}</span>
          </div>
        </div>
        <Toggle
          label="仅菜单栏模式"
          hint="隐藏 Dock 图标，只保留状态栏（立即生效）"
          checked={menubarOnly}
          onChange={(v) => {
            setMenubarOnly(v);
            set("menubar_only", String(v));
          }}
        />
        <Toggle
          label="登录时自动启动"
          hint="通过 macOS LaunchAgent 实现"
          checked={autostart}
          onChange={async (v) => {
            setAutostart(v);
            try {
              if (v) await enable();
              else await disable();
            } catch {
              setAutostart(!v);
            }
          }}
        />
      </section>

      <section className="setting-card">
        <h2>歌词与封面</h2>
        <Toggle
          label="在线获取歌词和封面"
          hint="从网易云音乐、QQ 音乐等来源匹配歌词与封面"
          checked={online}
          onChange={(v) => {
            setOnline(v);
            set("online_metadata", String(v));
          }}
        />
        <div className="setting-row">
          <div>
            <div className="setting-label">歌词缓存</div>
            <div className="setting-hint">{cacheCount} 条已缓存的在线歌词</div>
          </div>
          <div className="setting-actions" style={{ marginTop: 0 }}>
            <button
              className="ghost-btn"
              onClick={async () => {
                await api.refreshCovers();
                useToast.getState().show("已清空封面，重新扫描并后台匹配中…");
                await reload();
              }}
            >
              重新匹配全部封面
            </button>
            <button
              className="ghost-btn danger"
              onClick={async () => {
                await api.clearMetaCache();
                setCacheCount(await api.metaCacheCount());
              }}
            >
              清除缓存
            </button>
          </div>
        </div>
      </section>

      <section className="setting-card about">
        <div>Sonora · 本地音乐播放器 · v0.1.0</div>
        <div className="setting-hint">关闭窗口不会退出播放；⌘Q 退出应用</div>
      </section>
    </div>
  );
}
