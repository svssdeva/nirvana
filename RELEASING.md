# Releasing Nirvana

Nirvana ships **two editions** per release, each as an installer + a portable exe:

| Edition | Installer | Portable | Network |
|---|---|---|---|
| **Offline** (default) | `Nirvana_<version>_x64-setup.exe` | `nirvana.exe` | None, ever |
| **Online** | `Nirvana-online_<version>_x64-setup.exe` | `nirvana-online.exe` | Only when SteamGridDB art is enabled |

- **Installer:** NSIS, **per-user** (`installMode: currentUser`) — installs to
  `%LOCALAPPDATA%`, no admin/UAC.
- **Portable:** the bare release binary — no install, run from anywhere. Tauri has
  no portable *bundle* target, so this is the raw `target/release` exe. Needs the
  **WebView2 runtime** (preinstalled on Windows 11).

The **online** edition is the offline build plus `--features steamgriddb`; the
release workflow overrides `productName` (via `src-tauri/tauri.online.conf.json`)
so its artifacts get the `-online` names and don't collide.

Both editions are **unsigned** (no code-signing cert), so Windows SmartScreen
warns on first run → *More info → Run anyway*. See **Code signing** below.

## Versioning

The version lives in three files and must match:

- `src-tauri/tauri.conf.json` → `version`
- `src-tauri/Cargo.toml` → `package.version`
- `package.json` → `version`

We use SemVer with a pre-release label for alpha, e.g. `0.1.0-alpha.1`. The UI
shows it (nav "alpha" badge + the version line in Settings).

The **README does not hardcode a version** — its "latest" badge is a live
shields.io query against the newest GitHub Release, so publishing a release
updates it automatically (no commit to `main`, no CI write-back, no version drift).

## Cutting a release (automated)

1. Bump the version in the three files above (e.g. `0.1.0-alpha.2`).
2. Commit, then tag and push:
   ```sh
   git tag v0.1.0-alpha.2
   git push origin v0.1.0-alpha.2
   ```
3. `.github/workflows/release.yml` runs on the tag. A serialized 2-job matrix
   (`max-parallel: 1`) builds both editions: the **offline** job creates the
   **pre-release** and uploads `Nirvana_*_setup.exe` + `nirvana.exe`; the
   **online** job reuses the same release and adds `Nirvana-online_*_setup.exe` +
   `nirvana-online.exe`.
4. Edit the release notes on GitHub if needed; keep "pre-release" ticked for alpha.

`workflow_dispatch` lets you run it manually from the Actions tab without a tag.

## Building locally

```sh
bun run tauri build                    # default offline build → installer + exe
# online edition (opt-in network cover art), with the -online product name:
bun run tauri build -- --features steamgriddb --config src-tauri/tauri.online.conf.json
```

Outputs (offline):
- Installer: `src-tauri/target/release/bundle/nsis/Nirvana_<version>_x64-setup.exe`
- Portable:  `src-tauri/target/release/nirvana.exe`

Budgets (v1): binary < 50 MB, cold start < 1 s. The release profile
(`opt-level="z"`, fat LTO, one codegen unit, stripped) keeps the binary lean.

## Code signing

Builds are currently **unsigned**, which is why SmartScreen warns on first run.
Signing is optional but removes that warning. To sign, you need:

1. **A code-signing certificate.** Either:
   - **OV** (Organization Validation) — cheaper (~$100–300/yr), but SmartScreen
     reputation builds up over time/downloads before the warning fully clears; or
   - **EV** (Extended Validation) — pricier and requires a hardware token / HSM
     (or a cloud signing service like Azure Trusted Signing), but gets instant
     SmartScreen reputation. EV usually needs a registered legal entity.
2. **The cert available to CI** — for a `.pfx`, base64-encode it into a repo
   secret (e.g. `WINDOWS_CERTIFICATE`) plus its password (`WINDOWS_CERTIFICATE_PASSWORD`).
   For Azure Trusted Signing, store the service credentials as secrets instead.
3. **Wire Tauri's signing config** — set `bundle.windows.certificateThumbprint`
   (or `signCommand` for a custom/cloud signer + `digestAlgorithm`,
   `timestampUrl`) in `tauri.conf.json`, and pass the secrets to `tauri-action`
   in the workflow. tauri-action signs the NSIS installer automatically once the
   cert env is present; the portable `nirvana.exe` would need a separate
   `signtool`/cloud-sign step.

Until a cert is in place, keep the "unsigned → Run anyway" note in the README and
release body. Tracked in `FUTURE-PLANS.md`.
