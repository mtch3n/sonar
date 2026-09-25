const MINUTE = 60;
const HOUR = 60 * MINUTE;
const DAY = 24 * HOUR;

/** How long ago a unix timestamp was, e.g. `3d ago`. */
export function age(unixSeconds: number): string {
  const seconds = Math.max(0, Date.now() / 1000 - unixSeconds);
  if (seconds < MINUTE) return "just now";
  if (seconds < HOUR) return `${Math.floor(seconds / MINUTE)}m ago`;
  if (seconds < DAY) return `${Math.floor(seconds / HOUR)}h ago`;
  if (seconds < 30 * DAY) return `${Math.floor(seconds / DAY)}d ago`;
  if (seconds < 365 * DAY) return `${Math.floor(seconds / (30 * DAY))}mo ago`;
  return `${Math.floor(seconds / (365 * DAY))}y ago`;
}

const UNITS = ["B", "KB", "MB", "GB", "TB"];

/** A byte count for people, e.g. `2.4 MB`. */
export function size(bytes: number): string {
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return unit === 0 ? `${bytes} B` : `${value.toFixed(1)} ${UNITS[unit]}`;
}
