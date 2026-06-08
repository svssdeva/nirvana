# Contributing to Nirvana

Thanks for your interest! Nirvana is a fully-offline Windows game launcher
(Tauri 2 + Rust core + Lit 3 frontend). This guide covers how to get set up,
the conventions the codebase follows, and how to get a change merged.

> Nirvana is in **alpha**. Expect rough edges and breaking changes between
> versions.

## Ground rules (please read first)

Two constraints are **non-negotiable** — a PR that violates either will not be
merged:

1. **No network in the default build.** The standard build makes zero network
   requests. The *only* networked feature (SteamGridDB cover art) is gated behind
   the `steamgriddb` Cargo feature, **off by default**, and compiled out entirely
   otherwise. Any new network capability must be behind a feature flag that is off
   by default.
2. **No destructive actions on the user's files or games.** Nirvana discovers and
   launches games; it never installs, downloads, or deletes game files. Launch
   only via official mechanisms (store protocols) or a validated `argv` spawn —
   never a shell.

Other expectations:

- **Secrets stay out of the repo and the DB.** The SteamGridDB API key lives in
  the OS credential vault (`keyring`) only — never in SQLite, logs, or commits.
- **Parameterize all SQL.** No string-built queries.
- **Least privilege.** Don't broaden Tauri capabilities (`src-tauri/capabilities/`)
  or the CSP without a clear reason in the PR description.

## Getting set up

**Prerequisites:** Windows 10/11, [Bun](https://bun.sh), the
[Rust toolchain](https://rustup.rs) (MSVC), and the
[Tauri prerequisites](https://tauri.app/start/prerequisites/) (WebView2 +
Microsoft C++ Build Tools).

```sh
bun install            # install frontend deps
bun run tauri dev      # run the app with hot reload
```

Run Rust commands from PowerShell (not git-bash) on Windows. While `tauri dev` is
running it locks the debug exe, so use `cargo check` / `cargo test --lib` rather
than a full `cargo build` in a second terminal.

## Project layout

- `src-tauri/src/` — the Rust core: all scanning, parsing, disk, GPU, monitor, and
  persistence logic. OS access (registry, filesystem, WMI, PDH, icons) sits behind
  **trait seams** with real Windows impls (`#[cfg(windows)]`) and in-memory fakes
  (`#[cfg(test)]`), so the logic is unit-tested on any OS.
- `src/components/` — Lit 3 web components, one per view. No React.
- `src/ipc.ts` — the **only** place the frontend touches `@tauri-apps/api`. Add a
  typed wrapper here for any new command rather than calling `invoke` directly.

## Conventions

- **Rust:** keep `cargo fmt` clean and `cargo clippy` warning-free. New core logic
  needs unit tests against the fakes. Errors flow through the `CoreError`/`AppError`
  types — don't `unwrap()` on fallible OS calls in command paths.
- **TypeScript / Lit:** components use `experimentalDecorators` +
  `useDefineForClassFields: false` (already set). Tear down every event listener
  in `disconnectedCallback`.
- **UI:** follow the existing design system (`docs/design.md`) — reuse the pill /
  8px-card / band vocabulary and the semantic color tokens; don't invent new
  values.
- **Commits:** clear, imperative subject lines. Group related changes.

## Before you open a PR

Run the full local check and make sure it's green:

```sh
bun run build                              # frontend: tsc + vite build
cargo fmt --manifest-path src-tauri/Cargo.toml
cargo test --lib --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml
```

Claim "tested"/"passing" only after you've run the check and seen the output.

If your change is user-visible, add an entry to the **`## [Unreleased]`** section
of [`CHANGELOG.md`](CHANGELOG.md).

## Pull requests

1. Fork and branch from `main`.
2. Keep PRs focused — one logical change per PR is easiest to review.
3. In the description, explain **what** and **why**, and call out anything that
   touches the network boundary, capabilities, CSP, SQL, or launch path.
4. Link any related issue.

## Reporting bugs / ideas

Open an issue with your Windows version, what you did, what you expected, and what
happened. For feature ideas, check [`FUTURE-PLANS.md`](FUTURE-PLANS.md) first — it
lists what's already planned (next up: more stores/launchers).

By contributing, you agree your contributions are licensed under the same terms as
the project.
