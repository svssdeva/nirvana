/** Next focus index for an arrow/Home/End key in a `cols`-wide grid of `count`
 *  tiles. Compact view passes cols = 1. Pure + testable (no DOM). */
export function nextIndex(
  current: number,
  key: string,
  cols: number,
  count: number,
): number {
  if (count === 0) return 0;
  switch (key) {
    case "ArrowRight":
      return Math.min(current + 1, count - 1);
    case "ArrowLeft":
      return Math.max(current - 1, 0);
    case "ArrowDown":
      return Math.min(current + cols, count - 1);
    case "ArrowUp":
      return current - cols < 0 ? current : current - cols;
    case "Home":
      return 0;
    case "End":
      return count - 1;
    default:
      return current;
  }
}
