// App appearance: auto (follow system) / light / dark.
// The `dark` class on <html> drives the CSS variable palettes; the Tauri
// window theme is synced too so the vibrancy material matches the choice.
// The mini (HudWindow) window skips the window sync.
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

export type Theme = "auto" | "light" | "dark";

const KEY = "sonora.theme";
// pre-rename key, still read so an existing preference survives the rename
const LEGACY_KEY = "ownmusic.theme";
const mq = window.matchMedia("(prefers-color-scheme: dark)");

export function getTheme(): Theme {
  const v = localStorage.getItem(KEY) ?? localStorage.getItem(LEGACY_KEY);
  return v === "light" || v === "dark" ? v : "auto";
}

function applyClass() {
  const t = getTheme();
  document.documentElement.classList.toggle("dark", t === "dark" || (t === "auto" && mq.matches));
}

function applyWindow(theme: Theme) {
  void getCurrentWebviewWindow()
    .setTheme(theme === "auto" ? null : theme)
    .catch(() => {});
}

export function initTheme() {
  applyClass();
  applyWindow(getTheme());
  mq.addEventListener("change", () => {
    applyClass();
    // auto → also keep the native window appearance in sync
    if (getTheme() === "auto") applyWindow("auto");
  });
}

export function setTheme(theme: Theme) {
  localStorage.setItem(KEY, theme);
  applyClass();
  applyWindow(theme);
}
