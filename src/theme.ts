// Theme persistence + application. Kept separate from `store.ts` so the Store
// stays DOM-free and unit-testable: the Store holds the chosen theme, this
// module reflects it to the document and remembers it across launches.
//
// localStorage is the persistence layer (purely local — no network, honoring
// the offline guarantee). Later this can migrate to the SQLite `setting` table.

import type { Theme } from "./store";

const STORAGE_KEY = "nirvana-theme";

/** Initial theme: the saved preference, else **dark** (the brand default — we
 *  intentionally don't follow the OS light/dark setting). */
export function readStoredTheme(): Theme {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === "light" || saved === "dark") return saved;
  } catch {
    // localStorage may be unavailable (private mode / disabled); fall through.
  }
  return "dark";
}

/** Reflect the theme to <html data-theme> and persist it. Idempotent. */
export function applyTheme(theme: Theme): void {
  document.documentElement.dataset.theme = theme;
  try {
    localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    // Persistence is best-effort; the in-memory theme still applies.
  }
}
