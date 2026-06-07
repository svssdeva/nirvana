# Releasing Nirvana

Nirvana ships two Windows artifacts per release:

| Artifact | File | Notes |
|---|---|---|
| **Installer** | `Nirvana_<version>_x64-setup.exe` | NSIS, **per-user** (`installMode: currentUser`) — installs to `%LOCALAPPDATA%`, no admin/UAC. |
| **Portable** | `nirvana.exe` | The bare release binary — no install, run from anywhere. Tauri has no portable *bundle* target, so this is the raw `target/release` exe. Needs the **WebView2 runtime** (preinstalled on Windows 11). |

Both are **unsigned** (no code-signing cert), so Windows SmartScreen warns on
first run → *More info → Run anyway*. Expected for an unsigned per-user app.

## Versioning

The version lives in three files and must match:

- `src-tauri/tauri.conf.json` → `version`
- `src-tauri/Cargo.toml` → `package.version`
- `package.json` → `version`

We use SemVer with a pre-release label for alpha, e.g. `0.1.0-alpha.1`. The UI
shows it (nav "alpha" badge + the version line in Settings).

## Cutting a release (automated)

1. Bump the version in the three files above (e.g. `0.1.0-alpha.2`).
2. Commit, then tag and push:
   ```sh
   git tag v0.1.0-alpha.2
   git push origin v0.1.0-alpha.2
   ```
3. `.github/workflows/release.yml` runs on the tag: it builds the release,
   creates a **pre-release** GitHub Release named `Nirvana v0.1.0-alpha.2`,
   uploads the **installer** (via `tauri-action`), then attaches the **portable**
   `nirvana.exe`.
4. Edit the release notes on GitHub if needed; keep "pre-release" ticked for alpha.

`workflow_dispatch` lets you run it manually from the Actions tab without a tag.

## Building locally

```sh
bun run tauri build                    # default offline build → installer + exe
bun run tauri build -- --features steamgriddb   # opt-in network cover art
```

Outputs:
- Installer: `src-tauri/target/release/bundle/nsis/Nirvana_<version>_x64-setup.exe`
- Portable:  `src-tauri/target/release/nirvana.exe`

Budgets (v1): binary < 50 MB, cold start < 1 s. The release profile
(`opt-level="z"`, fat LTO, one codegen unit, stripped) keeps the binary lean.
