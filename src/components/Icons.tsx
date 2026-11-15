// SF-Symbols-inspired SVG icon set for the Sonora UI.
// Solid icons use fill=currentColor; line icons use stroke with round caps.
// Geometry targets ~19px of ink in a 24px box so icons read well at 15-18px.
import type { ReactNode } from "react";

type IconProps = { size?: number; className?: string };

function Solid({ size = 16, className = "", children }: IconProps & { children: ReactNode }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="currentColor"
      className={`icn ${className}`}
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

function Line({
  size = 16,
  className = "",
  sw = 2.1,
  children,
}: IconProps & { sw?: number; children: ReactNode }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={sw}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={`icn ${className}`}
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

/* ---------- transport (solid) ---------- */

export function IconPlay({ size, className }: IconProps) {
  return (
    <Solid size={size} className={className}>
      <path d="M7.4 4.9v14.2c0 .5.55.8.97.53l10.9-7.1a.63.63 0 0 0 0-1.06L8.37 4.37a.63.63 0 0 0-.97.53z" />
    </Solid>
  );
}

export function IconPause({ size, className }: IconProps) {
  return (
    <Solid size={size} className={className}>
      <rect x="6.1" y="4.4" width="4.1" height="15.2" rx="1.5" />
      <rect x="13.8" y="4.4" width="4.1" height="15.2" rx="1.5" />
    </Solid>
  );
}

export function IconPrev({ size, className }: IconProps) {
  return (
    <Solid size={size} className={className}>
      <rect x="4.9" y="4.4" width="2.3" height="15.2" rx="1.15" />
      <path d="M19.9 5.6v12.8a.63.63 0 0 1-.97.53l-9.7-6.4a.63.63 0 0 1 0-1.06l9.7-6.4a.63.63 0 0 1 .97.53z" />
    </Solid>
  );
}

export function IconNext({ size, className }: IconProps) {
  return (
    <Solid size={size} className={className}>
      <rect x="16.8" y="4.4" width="2.3" height="15.2" rx="1.15" />
      <path d="M4.1 5.6v12.8a.63.63 0 0 0 .97.53l9.7-6.4a.63.63 0 0 0 0-1.06l-9.7-6.4A.63.63 0 0 0 4.1 5.6z" />
    </Solid>
  );
}

/* ---------- play modes (line) ---------- */

export function IconQueue({ size, className }: IconProps) {
  return (
    <Line size={size} className={className}>
      <path d="M9.4 6.2h11.2M9.4 12h11.2M9.4 17.8h11.2" />
      <circle cx="4.4" cy="6.2" r="1.5" fill="currentColor" stroke="none" />
      <circle cx="4.4" cy="12" r="1.5" fill="currentColor" stroke="none" />
      <circle cx="4.4" cy="17.8" r="1.5" fill="currentColor" stroke="none" />
    </Line>
  );
}

export function IconShuffle({ size, className }: IconProps) {
  return (
    <Line size={size} className={className}>
      <polyline points="16.5 3.5 20.5 3.5 20.5 7.5" />
      <line x1="4" y1="20" x2="20.5" y2="3.5" />
      <polyline points="20.5 16.5 20.5 20.5 16.5 20.5" />
      <line x1="14.7" y1="14.7" x2="20.5" y2="20.5" />
      <line x1="4" y1="4" x2="9.3" y2="9.3" />
    </Line>
  );
}

export function IconRepeat({ size, className }: IconProps) {
  return (
    <Line size={size} className={className}>
      <polyline points="17 2.5 21 6.5 17 10.5" />
      <path d="M3.5 12.9v-2.4a4 4 0 0 1 4-4H21" />
      <polyline points="7 21.5 3 17.5 7 13.5" />
      <path d="M20.5 11.1v2.4a4 4 0 0 1-4 4H3" />
    </Line>
  );
}

export function IconRepeat1({ size, className }: IconProps) {
  return (
    <Line size={size} className={className}>
      <polyline points="17 2.5 21 6.5 17 10.5" />
      <path d="M3.5 12.9v-2.4a4 4 0 0 1 4-4H21" />
      <polyline points="7 21.5 3 17.5 7 13.5" />
      <path d="M20.5 11.1v2.4a4 4 0 0 1-4 4H3" />
      <path d="M11.3 10.9 13.2 9.55v6.05" />
    </Line>
  );
}

export function ModeIcon({ mode, size = 16 }: { mode: string; size?: number }) {
  if (mode === "shuffle") return <IconShuffle size={size} />;
  if (mode === "loop_all") return <IconRepeat size={size} />;
  if (mode === "loop_one") return <IconRepeat1 size={size} />;
  return <IconQueue size={size} />;
}

/* ---------- library / navigation ---------- */

export function IconNote({ size, className }: IconProps) {
  return (
    <Solid size={size} className={className}>
      <ellipse cx="6.4" cy="17.5" rx="3.3" ry="2.8" />
      <rect x="8.6" y="6.6" width="1.8" height="11.1" rx="0.9" />
      <ellipse cx="16.8" cy="15.8" rx="3.3" ry="2.8" />
      <rect x="18.9" y="4.6" width="1.8" height="11.4" rx="0.9" />
      <path d="M8.6 6.6 20.7 4.6v3.5L8.6 10.1z" />
    </Solid>
  );
}

export function IconAlbums({ size, className }: IconProps) {
  return (
    <Line size={size} className={className} sw={2}>
      <rect x="7" y="7" width="13.4" height="13.4" rx="3.6" />
      <path d="M17 4.1H9A4.9 4.9 0 0 0 4.1 9v8" />
    </Line>
  );
}

export function IconPlayCircle({ size, className }: IconProps) {
  return (
    <Line size={size} className={className} sw={2}>
      <circle cx="12" cy="12" r="9" />
      <path
        d="M10 8.1v7.8c0 .44.49.71.85.46l5.9-3.9a.54.54 0 0 0 0-.92l-5.9-3.9A.54.54 0 0 0 10 8.1z"
        fill="currentColor"
        stroke="none"
      />
    </Line>
  );
}

export function IconGear({ size, className }: IconProps) {
  // solid gear: ring + 8 rounded teeth rotated around the center
  return (
    <Solid size={size} className={className}>
      <circle cx="12" cy="12" r="8.3" fill="none" stroke="currentColor" strokeWidth="3.5" />
      {Array.from({ length: 8 }, (_, i) => (
        <rect
          key={i}
          x="10.55"
          y="1.3"
          width="2.9"
          height="3.8"
          rx="1.25"
          transform={`rotate(${i * 45} 12 12)`}
        />
      ))}
    </Solid>
  );
}

export function IconSearch({ size, className }: IconProps) {
  return (
    <Line size={size} className={className} sw={2.2}>
      <circle cx="10.5" cy="10.5" r="7.1" />
      <path d="M15.8 15.8 20.5 20.5" />
    </Line>
  );
}

export function IconChevronLeft({ size, className }: IconProps) {
  return (
    <Line size={size} className={className} sw={2.3}>
      <path d="M15.3 5.1 8.1 12l7.2 6.9" />
    </Line>
  );
}

/* ---------- misc ---------- */

export function IconClose({ size, className }: IconProps) {
  return (
    <Line size={size} className={className} sw={2.3}>
      <path d="M6.9 6.9 17.1 17.1M17.1 6.9 6.9 17.1" />
    </Line>
  );
}

export function IconExpand({ size, className }: IconProps) {
  return (
    <Line size={size} className={className} sw={2.2}>
      <path d="M14.2 4.5h5.3v5.3M19.5 4.5l-7.1 7.1M9.8 19.5H4.5v-5.3M4.5 19.5l7.1-7.1" />
    </Line>
  );
}

/* ---------- volume (solid body + line waves) ---------- */

function VolumeBody() {
  return (
    <path d="M11.9 4.4 7.4 8.1H3.6a.7.7 0 0 0-.7.7v6.4a.7.7 0 0 0 .7.7h3.8l4.5 3.7a.7.7 0 0 0 1.16-.54V5.14A.7.7 0 0 0 11.9 4.4z" />
  );
}

export function IconVolHi({ size, className }: IconProps) {
  return (
    <Solid size={size} className={className}>
      <VolumeBody />
      <path
        d="M15.2 9.3a4.4 4.4 0 0 1 0 5.4M18.2 6.5a8.7 8.7 0 0 1 0 11"
        fill="none"
        stroke="currentColor"
        strokeWidth={2}
        strokeLinecap="round"
      />
    </Solid>
  );
}

export function IconVolLo({ size, className }: IconProps) {
  return (
    <Solid size={size} className={className}>
      <VolumeBody />
      <path
        d="M15.2 9.3a4.4 4.4 0 0 1 0 5.4"
        fill="none"
        stroke="currentColor"
        strokeWidth={2}
        strokeLinecap="round"
      />
    </Solid>
  );
}

export function IconVolZero({ size, className }: IconProps) {
  return (
    <Solid size={size} className={className}>
      <VolumeBody />
      <path
        d="M15.4 9.8l4.5 4.5M19.9 9.8l-4.5 4.5"
        fill="none"
        stroke="currentColor"
        strokeWidth={2}
        strokeLinecap="round"
      />
    </Solid>
  );
}
