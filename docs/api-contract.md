# Atrium — Interface Contract (IPC + OS seams)

> Output of the `api-and-interface-design` engineering skill. The **stable
> surface** of Atrium: the Tauri command/event IPC contract and the `os/` trait
> seams. Contract-first — these types are the spec; implementations follow.
> Sources verified per `source-driven-development` (citations inline).

## Principles applied
- **Contract first.** Types below are defined before implementation.
- **One error shape everywhere.** Every command returns `Result<T, AppError>`;
  `AppError = { kind: ErrorKind, message: string }`. No command returns `null`-for-
  error or throws a bare string. (Tauri rejects the JS promise on `Err`; the error
  type **must** implement `serde::Serialize`. Source:
  https://v2.tauri.app/develop/calling-rust/#error-handling)
- **Validate at the boundary (TB2).** Commands validate inputs; internal code trusts types.
- **Additive evolution (Hyrum's Law).** New fields are **optional**; never change/remove
  an existing field's type. Command names are a committed surface.
- **Naming:** commands `snake_case` verbs (`scan_library`); event names `domain://thing`;
  payload fields `camelCase` on the TS side via serde rename.

## Error contract
```rust
// error.rs — internal
#[derive(thiserror::Error, Debug)]
pub enum CoreError {
  #[error("not found: {0}")] NotFound(String),
  #[error("parse error: {0}")] Parse(String),
  #[error("registry: {0}")]   Registry(String),
  #[error("io: {0}")]         Io(#[from] std::io::Error),
  #[error("db: {0}")]         Db(String),
  #[error("gpu unavailable: {0}")] GpuUnavailable(String),
  #[error("unsupported: {0}")] Unsupported(String),
  #[error("cancelled")]        Cancelled,
}
// AppError — the serializable boundary type (commands map CoreError -> AppError)
#[derive(serde::Serialize)]
pub struct AppError { pub kind: String, pub message: String } // kind ∈ ErrorKind values
```
```ts
// ipc.ts — frontend
export type ErrorKind = 'NotFound'|'Parse'|'Registry'|'Io'|'Db'|'GpuUnavailable'|'Unsupported'|'Cancelled';
export interface AppError { kind: ErrorKind; message: string; }
```

## State access — IMPORTANT Tauri 2 gotcha
Async `#[tauri::command]`s **cannot take borrowed args**, including
`State<'_, T>`. (Source: https://v2.tauri.app/develop/calling-rust/#async-commands)
**Rule:** managed state (DB pool, sampler control) is stored as `Arc<…>` via
`.manage(AppState{ … })`; a **sync** command takes `State<AppState>` and clones the
`Arc` out, then hands it to async work — or the command is sync. Decide per command
(marked **sync/async** in the catalog). Mutable state wraps in `Mutex`/`RwLock`.

## Command catalog
Each row: name · async? · input → output · error kinds · notes.
All inputs/outputs are `serde` types; TS mirrors via `ipc.ts` wrappers.

| Command | A/S | Input | Output | Errors | Notes |
|---|---|---|---|---|---|
| `scan_library` | sync | `{ full: bool }` | `Game[]` | Db, Unsupported | **v1 (Task 8):** synchronous — runs sources concurrently (scoped threads), dedups, persists, returns the stored library; still emits `scan://progress` per source + `scan://done`. Off the UI thread (Tauri sync cmd). The streaming `ScanHandle` + cooperative cancellation below is deferred (not needed for a one-shot scan). |
| `get_library` | async | `{ filter?: LibraryFilter, sort?: SortSpec }` | `Game[]` | Db | from DB; no scan. |
| `get_game` | async | `{ id: GameId }` | `Game` | NotFound, Db | |
| `launch_game` | async | `{ id: GameId }` | `void` | NotFound, Unsupported, Io | validates exe path under install root (TB3). |
| `get_cover` | async | `{ id: GameId }` | `CoverRef` | Db | lazy, offline-first; never errors on missing → returns placeholder variant. |
| `list_drives` | async | `{}` | `Drive[]` | Io | sysinfo. |
| `compute_game_sizes` | sync* | `{ ids?: GameId[] }` | `SizeHandle` | Db | *async task; streams `size://progress`. Cancellable. |
| `get_gpus` | async | `{}` | `Gpu[]` | GpuUnavailable | WMI (+nvml). VRAM from DXGI/NVML, NOT WMI AdapterRAM (32-bit, caps ~4GB). |
| `monitor_start` | sync | `{ intervalMs?: number }` | `void` | — | starts/configures sampler; idempotent. |
| `monitor_stop` | sync | `{}` | `void` | — | pauses sampler (idle CPU ≈ 0); idempotent. |
| `cancel_task` | sync | `{ handle: TaskHandle }` | `void` | — | cooperative cancel for scan/size. |
| `get_settings` | async | `{}` | `Settings` | Db | |
| `set_setting` | async | `{ key: string, value: Json }` | `void` | Db | validates key ∈ known set. |
| `set_favorite` | async | `{ id: GameId, favorite: bool }` | `void` | NotFound, Db | |
| `set_tags` | async | `{ id: GameId, tags: string[] }` | `void` | NotFound, Db | |
| `open_install_folder` | async | `{ id: GameId }` | `void` | NotFound, Io | via `tauri-plugin-opener`. |

**Discriminated unions** (avoid shape-switching):
```ts
type CoverRef =
  | { type: 'image'; path: string }      // resolved file (steam cache / exe icon / gridindb)
  | { type: 'placeholder' };             // none found
type Source = 'steam' | 'epic' | 'local';
```

## Event contract
Payloads must be `Serialize + Clone`. **Frontend MUST call the returned `unlisten`
in the Lit element's `disconnectedCallback`** (Source:
https://v2.tauri.app/develop/calling-frontend/ — "Always use the unlisten function
when your execution context goes out of scope").

| Event | Payload | Emitted by |
|---|---|---|
| `scan://progress` | `{ source: Source, found: number, done: boolean }` | scan task |
| `scan://done` | `{ total: number }` | scan task |
| `size://progress` | `{ id: GameId, sizeBytes: number }` | sizing task |
| `monitor://sample` | `Sample` (see system-design §3) | sampler (~1Hz) |

## Cancellation
Long operations (`scan_library`, `compute_game_sizes`) return a `*Handle` and run as
async tasks holding a `tokio_util::sync::CancellationToken` (or `Arc<AtomicBool>`)
kept in managed state. `cancel_task` flips it; the task checks cooperatively and
emits a terminal event, returning `CoreError::Cancelled` to any awaiter.

## OS trait seams (ADR-0005) — the portability + testability boundary
```rust
// os/mod.rs — interfaces; real impls #[cfg(windows)], in-memory fakes for tests.
pub trait Registry {
  fn read_string(&self, hive: Hive, path: &str, name: &str) -> Result<Option<String>, CoreError>;
  fn enum_subkeys(&self, hive: Hive, path: &str) -> Result<Vec<String>, CoreError>;
}
pub trait FileSystem {
  fn read_to_string(&self, path: &Path) -> Result<String, CoreError>;
  fn read_dir(&self, path: &Path) -> Result<Vec<DirEntryInfo>, CoreError>; // incl. is_reparse_point
  fn metadata(&self, path: &Path) -> Result<FileMeta, CoreError>;
}
pub trait Wmi  { fn video_controllers(&self) -> Result<Vec<WmiGpu>, CoreError>; }
pub trait Pdh  { fn gpu_engine_util(&self) -> Result<Option<f32>, CoreError>; } // None => unavailable/gated
```
- Scanners/sizers/art take `&dyn Trait` (or generics) — **constructor injection**.
- `DirEntryInfo.is_reparse_point` lets `disk.rs` skip junctions (jwalk's `follow_links`
  defaults false but does **not** distinguish junctions natively → check
  `FILE_ATTRIBUTE_REPARSE_POINT`). (Source: https://docs.rs/jwalk + MS Learn.)

## Verification (interface review)
- [ ] Every command has typed input + output and returns `Result<_, AppError>`.
- [ ] One error format across all commands; `AppError` is `Serialize`.
- [ ] Async commands take **no** borrowed `State`; state cloned from `Arc`.
- [ ] List/stream outputs (scan/size) are event-driven + cancellable, not blocking.
- [ ] New fields would be optional/additive; command names treated as committed.
- [ ] Every event listener has a matching `unlisten` on element teardown.
