import type { PlayerTrackDto } from "../types";

/** Format source-file properties, without inventing a bit depth for lossy audio. */
export function formatAudioQuality(
  track: Pick<PlayerTrackDto, "bit_depth" | "sample_rate">,
): string {
  const parts: string[] = [];
  if (track.bit_depth !== null && Number.isFinite(track.bit_depth) && track.bit_depth > 0) {
    parts.push(`${track.bit_depth}-bit`);
  }
  if (track.sample_rate !== null && Number.isFinite(track.sample_rate) && track.sample_rate > 0) {
    parts.push(`${Number((track.sample_rate / 1000).toFixed(1))} kHz`);
  }
  return parts.join(" · ");
}
