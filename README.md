<p align="center">
  <img src="public/icon.png" width="116" alt="Nirvana logo" />
</p>

<h1 align="center">Nirvana</h1>

<p align="center">
  A fully offline Windows launcher that unifies your Steam, Epic, and local games — with disk insight, GPU info, and a minimal system monitor.
</p>

<p align="center">
  <strong>Alpha</strong> · <code>v0.1.0-alpha.1</code> · by <a href="https://github.com/svssdeva">svssdeva</a> / beyondcodekarma
</p>

---

Nirvana brings every game you own into one fast, native window — no accounts, no
store, no telemetry, and no network calls in the default build. It discovers what
you have installed, lets you launch through the official mechanisms, and adds the
practical extras a launcher should have: per-game disk usage, a GPU/driver panel,
and a live resource monitor.

> [!NOTE]
> **Offline by default.** The standard build makes zero network requests. The
> only networked feature — SteamGridDB cover art — is opt-in, off by default, and
> compiled out entirely unless you build with the `steamgriddb` feature.

## Features

- **One library, every source** — discovers Steam (`libraryfolders.vdf` →
  `appmanifest`), Epic (`.item` manifests), and local installs (your watch
  folders), deduped into a single grid.
- **Launch natively** — Steam/Epic via their official protocols, local games via
  a validated `argv` spawn (never a shell).
- **Browse your way** — a comfortable 5-up cover grid or a compact list view;
  filter by source/favorites/tags, search, and sort by name/size/last-played.
- **Make it yours** — favorites, colored tags, and custom cover thumbnails
  (point at any local image — ideal for offline/local games).
- **Disk insight** — per-drive capacity and biggest-games, with junction-safe
  sizing. Open an install folder or the store's uninstall flow; Nirvana never
  deletes files.
- **System monitor** — ~1 Hz CPU / RAM / network / disk / GPU + VRAM sparklines,
  a system-info panel, and an Open Task Manager shortcut. Sampling pauses when
  the window isn't focused, so an idle monitor costs nothing.
- **Polished** — light/dark themes (dark by default), a sticky header, and smooth
  view transitions.

## Tech stack

| Layer | Choice |
|---|---|
| Shell | [Tauri 2](https://tauri.app) — Rust core + system WebView2 |
| Frontend | [Lit 3](https://lit.dev) + [Vite](https://vitejs.dev) + TypeScript |
| Storage | SQLite via [`rusqlite`](https://docs.rs/rusqlite) (bundled) |
| System / GPU | [`windows`](https://docs.rs/windows) (WMI · PDH · DXGI), [`sysinfo`](https://docs.rs/sysinfo) |
| Extras | `image`, `qrcode`, `keyring` (opt-in SteamGridDB) |

All scanning, parsing, disk, GPU, monitor, and persistence logic lives in the
Rust core under `src-tauri/src/`; OS access sits behind trait seams so the logic
is unit-tested with in-memory fakes. The UI is a set of Lit web components under
`src/components/`.

## Getting started

**Prerequisites:** Windows 10/11, [Bun](https://bun.sh), the
[Rust toolchain](https://rustup.rs) (MSVC), and the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) (WebView2 +
Microsoft C++ Build Tools).

```sh
bun install            # install frontend deps
bun run tauri dev      # run the app with hot reload
```

Other useful commands:

```sh
bun run tauri build    # production bundle (installer + portable exe)
bun run build          # frontend only (tsc + vite build)
cargo test             # in src-tauri/ — run the Rust test suite
cargo clippy           # in src-tauri/ — lints
```

## Install (Windows)

Grab a build from [Releases](../../releases), or build it yourself with
`bun run tauri build`. Two artifacts are produced:

- **Installer** — `Nirvana_<version>_x64-setup.exe` (NSIS, **per-user**: installs
  to `%LOCALAPPDATA%`, **no admin / UAC**).
- **Portable** — `nirvana.exe` (run from anywhere, no install; needs the WebView2
  runtime, preinstalled on Windows 11).

> [!IMPORTANT]
> Builds are **unsigned** (no code-signing certificate), so SmartScreen may warn
> "Windows protected your PC" on first run. Choose **More info → Run anyway**.
> Expected for an unsigned per-user app.

## Configuration

- **Local games** come only from folders you add under **Settings → Watch
  folders** (curated, so no installed-app noise). Steam/Epic are detected
  automatically.
- **SteamGridDB cover art** is opt-in. Toggle it in Settings and build with the
  feature flag — it adds a network dependency, so it stays off in the default,
  fully-offline build:
  ```sh
  bun run tauri build -- --features steamgriddb
  ```
  The API key is stored in the OS credential vault, never on disk or in logs.

## Documentation

- [`CHANGELOG.md`](CHANGELOG.md) — release notes
- [`RELEASING.md`](RELEASING.md) — versioning + how releases are cut
- [`FUTURE-PLANS.md`](FUTURE-PLANS.md) — roadmap (next up: more stores)
- [`docs/design.md`](docs/design.md) — the PlayStation-style design system
- [`docs/PRD-game-launcher.md`](docs/PRD-game-launcher.md) — full product spec
