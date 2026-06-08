// Typed seam over Tauri IPC (`docs/api-contract.md`). Every command returns
// `Result<T, AppError>` on the Rust side; Tauri rejects the JS promise with the
// serialized `AppError` on `Err`. This module is the ONLY place the rest of the
// UI touches `@tauri-apps/api` — views call these wrappers, never `invoke`.

import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type EventCallback, type UnlistenFn } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { getCurrentWindow, Window } from "@tauri-apps/api/window";

// ── Error contract (mirrors error.rs `ErrorKind` / `AppError`) ───────────────
export type ErrorKind =
  | "NotFound"
  | "Parse"
  | "Registry"
  | "Io"
  | "Db"
  | "GpuUnavailable"
  | "Unsupported"
  | "Cancelled";

export interface AppError {
  kind: ErrorKind;
  message: string;
}

/** Narrow an unknown rejection to the `AppError` boundary shape. */
export function isAppError(e: unknown): e is AppError {
  return (
    typeof e === "object" &&
    e !== null &&
    "kind" in e &&
    "message" in e &&
    typeof (e as AppError).message === "string"
  );
}

/** Coerce any thrown value into an `AppError` so callers handle one shape. */
export function toAppError(e: unknown): AppError {
  if (isAppError(e)) return e;
  return { kind: "Unsupported", message: typeof e === "string" ? e : String(e) };
}

// ── Domain types (mirror models.rs serde camelCase) ──────────────────────────
export type Source = "steam" | "epic" | "local" | "gog";

/** Store metadata for theming badges/filters (mirrors the `SourceInfo` command). */
export interface SourceInfo {
  source: Source;
  display: string;
  /** Brand accent, hex (e.g. GOG `#a23fff`). */
  color: string;
}

export interface Game {
  id: number;
  source: Source;
  externalId: string;
  name: string;
  installPath: string;
  exePath: string | null;
  sizeBytes: number | null;
  drive: string | null;
  lastPlayed: number | null;
  launchCount: number;
  coverPath: string | null;
  favorite: boolean;
  tags: string[];
}

/** Library filter/sort/search request (mirrors library.rs `LibraryQuery`). */
export type SortBy = "name" | "size" | "lastPlayed";
export interface LibraryQuery {
  search?: string;
  source?: Source;
  drive?: string;
  favoritesOnly?: boolean;
  tag?: string;
  sort?: SortBy;
  descending?: boolean;
}

/** Discriminated cover reference (api-contract §"Discriminated unions"). */
export type CoverRef = { type: "image"; path: string } | { type: "placeholder" };

/** Donation details for the Settings "Support Nirvana" section. */
export interface DonationInfo {
  upiId: string;
  /** Self-contained SVG QR of the UPI payment link. */
  qrSvg: string;
}

/** A storage volume (mirrors models.rs `Drive`). Sizes in bytes. */
export interface Drive {
  mount: string;
  letter: string | null;
  label: string;
  totalBytes: number;
  freeBytes: number;
}

/** A video adapter (mirrors models.rs `Gpu`). */
export interface Gpu {
  name: string;
  driverVersion: string;
  driverDate: string | null;
  vramBytes: number | null;
}

/** One monitor reading (mirrors models.rs `Sample`). Rates are per-second. */
export interface Sample {
  cpuPercent: number;
  memUsedBytes: number;
  memTotalBytes: number;
  netRxBps: number;
  netTxBps: number;
  diskReadBps: number;
  diskWriteBps: number;
  gpuPercent: number | null;
  vramUsedBytes: number | null;
  vramTotalBytes: number | null;
}

// ── Command invocation ───────────────────────────────────────────────────────
/**
 * Invoke a Rust command, normalizing any rejection to an `AppError`. Prefer the
 * named wrappers below; this generic exists for commands not yet wrapped.
 */
export async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw toAppError(e);
  }
}

/** Round-trips the Rust `ping` command — the smoke test for the IPC bridge. */
export function ping(fail = false): Promise<string> {
  return call<string>("ping", { fail });
}

/** App version from tauri.conf (e.g. `0.1.0-alpha.1`). */
export function appVersion(): Promise<string> {
  return getVersion();
}

/**
 * Window controls for the custom (borderless) title bar — drag, the min/max/close
 * buttons, and maximize-state tracking. Keeps `@tauri-apps/api/window` behind the
 * ipc seam like everything else.
 */
export const win = {
  minimize: () => getCurrentWindow().minimize(),
  toggleMaximize: () => getCurrentWindow().toggleMaximize(),
  close: () => getCurrentWindow().close(),
  startDragging: () => getCurrentWindow().startDragging(),
  isMaximized: () => getCurrentWindow().isMaximized(),
  onResized: (cb: () => void): Promise<UnlistenFn> => getCurrentWindow().onResized(cb),
};

/**
 * Reveal the (initially hidden) main window and close the splashscreen window.
 * Called once the UI has painted. Best-effort: each step is independently
 * guarded so a missing splash window or permission never blocks the app.
 */
export async function dismissSplash(): Promise<void> {
  try {
    await getCurrentWindow().show();
  } catch {
    // already visible / not in a Tauri context (e.g. plain web dev)
  }
  try {
    const splash = await Window.getByLabel("splashscreen");
    await splash?.close();
  } catch {
    // no splash window — nothing to close
  }
}

/**
 * Discover installed games across all sources, dedup, persist, and resolve to the
 * stored library. Emits `scan://progress` per source + `scan://done` while it
 * runs (subscribe before calling to show live counts). `full` is reserved for a
 * future incremental mode; v1 always does a full scan.
 */
export function scanLibrary(full = true): Promise<Game[]> {
  return call<Game[]>("scan_library", { full });
}

/** Read the persisted library (no scan), optionally filtered/sorted/searched. */
export function getLibrary(query?: LibraryQuery): Promise<Game[]> {
  return call<Game[]>("get_library", { query: query ?? null });
}

/** List every known store with its display name + brand color (for theming). */
export function listSources(): Promise<SourceInfo[]> {
  return call<SourceInfo[]>("list_sources");
}

// The store set is static for the app's lifetime, so memoize one fetch and share
// it across the many tiles/filters that need colors. Cleared on failure so a
// later call can retry rather than caching a rejection forever.
let _sourcesCache: Promise<SourceInfo[]> | undefined;
export function sources(): Promise<SourceInfo[]> {
  if (!_sourcesCache) {
    _sourcesCache = listSources().catch((e) => {
      _sourcesCache = undefined;
      throw e;
    });
  }
  return _sourcesCache;
}

/** Toggle a game's favorite flag (persisted). */
export function setFavorite(id: number, favorite: boolean): Promise<void> {
  return call<void>("set_favorite", { id, favorite });
}

/** Replace a game's tags (persisted). */
export function setTags(id: number, tags: string[]): Promise<void> {
  return call<void>("set_tags", { id, tags });
}

/** Wipe the entire local database (games, tags, settings). Destructive. */
export function resetDatabase(): Promise<void> {
  return call<void>("reset_database");
}

/**
 * Pick a local image (native dialog) as a game's custom cover; copies it into the
 * app cache and persists it. Resolves to the new cover path, or null if cancelled.
 */
export function setCover(id: number): Promise<string | null> {
  return call<string | null>("set_cover", { id });
}

/** UPI ID + offline-generated QR for the donation section. */
export function getDonationInfo(): Promise<DonationInfo> {
  return call<DonationInfo>("get_donation_info");
}

/** List storage volumes with capacity (live, from sysinfo). */
export function listDrives(): Promise<Drive[]> {
  return call<Drive[]>("list_drives");
}

/**
 * Compute accurate on-disk sizes (all games, or the given ids), persisting each
 * and emitting `size://progress` per game. Resolves when every target is done.
 */
export function computeGameSizes(ids?: number[]): Promise<void> {
  return call<void>("compute_game_sizes", { ids: ids ?? null });
}

/** Open a game's install folder in the OS file manager. */
export function openInstallFolder(id: number): Promise<void> {
  return call<void>("open_install_folder", { id });
}

/** Open the store's uninstall flow for a game (Steam only; never deletes files). */
export function uninstallGame(id: number): Promise<void> {
  return call<void>("uninstall_game", { id });
}

/** DB-backed preferences (mirrors the `Settings` command struct). */
export interface Settings {
  monitorIntervalMs: number;
  watchFolders: string[];
  steamgriddbEnabled: boolean;
}

/** Read all persisted settings (with defaults). */
export function getSettings(): Promise<Settings> {
  return call<Settings>("get_settings");
}

/** Persist one setting (value is a string; JSON-encode arrays/bools as needed). */
export function setSetting(key: string, value: string): Promise<void> {
  return call<void>("set_setting", { key, value });
}

/** DEV ONLY: seed ~50 dummy games into the DB for local testing (no-op in release). */
export function seedDummyGames(): Promise<number> {
  return call<number>("seed_dummy_games");
}

/** Host OS/CPU/memory info (mirrors the `SystemInfo` command struct). */
export interface SystemInfo {
  osName: string;
  osVersion: string;
  kernelVersion: string;
  hostname: string;
  cpu: string;
  cpuThreads: number;
  memTotalBytes: number;
}

/** Read host system info (one-shot; not a live metric). */
export function getSystemInfo(): Promise<SystemInfo> {
  return call<SystemInfo>("get_system_info");
}

/** Open the Windows Task Manager (Nirvana never manages processes itself). */
export function openTaskManager(): Promise<void> {
  return call<void>("open_task_manager");
}

/** Video adapters with model + driver (WMI). */
export function getGpus(): Promise<Gpu[]> {
  return call<Gpu[]>("get_gpus");
}

/** Start (or restart) the 1Hz system-monitor sampler (emits `monitor://sample`). */
export function monitorStart(intervalMs?: number): Promise<void> {
  return call<void>("monitor_start", { intervalMs: intervalMs ?? null });
}

/** Stop the sampler (idle CPU ≈ 0). */
export function monitorStop(): Promise<void> {
  return call<void>("monitor_stop");
}

/**
 * Launch a game via its official mechanism (Steam protocol in v1) and record the
 * launch. Rejects with an `AppError` (e.g. `Unsupported` for sources not yet
 * wired, `NotFound`, `Io`).
 */
export function launchGame(id: number): Promise<void> {
  return call<void>("launch_game", { id });
}

/**
 * Resolve a game's cover (offline-first: Steam cache → exe icon → placeholder).
 * Lazy, per tile. Never rejects on a missing cover — returns the placeholder
 * variant.
 */
export function getCover(id: number): Promise<CoverRef> {
  return call<CoverRef>("get_cover", { id });
}

/**
 * Turn a backend cover file path into a WebView-loadable URL via Tauri's asset
 * protocol (the CSP allows `asset:`; the dir is scoped at startup). Keeping
 * `convertFileSrc` here preserves the rule that ipc.ts is the only `@tauri-apps`
 * touch-point.
 */
export function coverSrc(path: string): string {
  return convertFileSrc(path);
}

// ── Events (api-contract §"Event contract") ──────────────────────────────────
// Payloads emitted by the Rust side. Listeners MUST be torn down: `subscribe`
// returns the `unlisten` fn, which callers invoke in `disconnectedCallback`.
export interface EventMap {
  "scan://progress": { source: Source; found: number; done: boolean };
  "scan://done": { total: number };
  "size://progress": { id: number; sizeBytes: number };
  "monitor://sample": Sample;
}

/**
 * Subscribe to a backend event. The returned promise resolves to the
 * `unlisten` function — call it on element teardown to avoid leaks.
 */
export function subscribe<K extends keyof EventMap>(
  event: K,
  handler: EventCallback<EventMap[K]>,
): Promise<UnlistenFn> {
  return listen<EventMap[K]>(event, handler);
}
