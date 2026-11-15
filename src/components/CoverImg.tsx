import { coverUrl } from "./coverUrl";

export default function CoverImg({
  path,
  version,
  className = "",
  rounded = 8,
}: {
  path: string | null;
  version?: string | null;
  className?: string;
  rounded?: number;
}) {
  const url = coverUrl(path, version);
  if (!url) {
    return (
      <div className={`cover-placeholder ${className}`} style={{ borderRadius: rounded }}>
        <span>♪</span>
      </div>
    );
  }
  return (
    <img
      className={`cover-img ${className}`}
      src={url}
      alt=""
      draggable={false}
      style={{ borderRadius: rounded }}
    />
  );
}
