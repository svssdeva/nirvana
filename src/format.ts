// Small presentation helpers shared across views.

/**
 * Format a byte count as a short human size (e.g. `24.3 GB`). Returns `null`
 * when the size is unknown so callers can omit it. Binary divisor (1024) with
 * conventional unit labels.
 */
export function formatBytes(bytes: number | null): string | null {
  if (bytes == null) return null;
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

/**
 * Deterministic hue (0–359) for a tag name, so a tag is always the same color.
 * Used as `--h` for a theme-agnostic translucent chip (see game-tile/library-view).
 */
export function tagHue(tag: string): number {
  let hash = 0;
  for (const ch of tag.toLowerCase()) {
    hash = (hash * 31 + ch.charCodeAt(0)) % 360;
  }
  return hash;
}
