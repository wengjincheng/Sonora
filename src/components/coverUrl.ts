import { convertFileSrc } from "@tauri-apps/api/core";

export function coverUrl(path: string | null, version?: string | null): string | null {
  if (!path) return null;
  const url = convertFileSrc(path);
  return version ? `${url}?v=${encodeURIComponent(version)}` : url;
}
