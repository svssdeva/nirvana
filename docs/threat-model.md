# Atrium — Threat Model & Hardening

> Output of the `security-and-hardening` engineering skill, adapted from its
> web-app framing to a **Tauri desktop** trust model. "No network" removes most
> of OWASP's surface; what remains is **untrusted on-disk data**, **executable
> launching**, **the WebView↔Rust IPC boundary**, **secrets**, and **unsafe FFI**.
> Mitigations map to tasks in `docs/plan/v1-implementation-plan.md`.

## Trust boundaries
```
[ on-disk manifests / registry / exe metadata ]  ──TB1──▶  Rust scanners
[ WebView (frontend, our code) ]                  ──TB2──▶  #[tauri::command] surface
[ a game's .exe / store protocol ]                ◀─TB3──   launch.rs (spawn)
[ SteamGridDB API (opt-in) ]                       ──TB4──▶  art/gridindb (feature)
[ user-influenced strings (names/paths/tags) ]    ──TB5──▶  SQLite
[ SteamGridDB API key ]                            ──TB6──▶  OS keychain (keyring)
```
Atrium is single-user, local, **no auth, no server** — so OWASP auth/session/CORS
items are N/A. The real adversary is **a malicious or buggy game writing crafted
manifests/registry entries** that we parse, plus footguns in launching exes.

## TB1 — Untrusted on-disk data (manifests, registry, exe icons)
**Threat:** A crafted `appmanifest_*.acf` / `*.item` / Uninstall registry value with
oversized fields, bad UTF-8, path traversal (`..\..\`), or malformed VDF triggers a
panic, OOM, or path escape.
**Mitigations:**
- Parse with maintained libraries (`keyvalues-serde` for VDF — ADR-0003; `serde_json`
  for Epic). **Never `unwrap()`/`expect()` on parsed data** — return `CoreError::Parse`.
- One bad entry must **not** abort the scan: parse per-entry, log+skip failures.
- Treat every path as untrusted: **canonicalize** and verify it stays within an
  expected root before use; bound string lengths before DB insert.
- Image/icon decode (`image` crate) of attacker-influenced files → catch decode
  errors, cap dimensions/bytes; never trust the file extension.

## TB2 — WebView ↔ Rust IPC
**Threat:** Weak CSP lets injected/remote script call privileged commands; broad
Tauri capabilities expose FS/shell to the webview.
**Mitigations:**
- **Set a restrictive CSP** (scaffold ships `csp: null` = no protection). Start:
  `default-src 'self'; img-src 'self' asset: http://asset.localhost; script-src 'self'; style-src 'self' 'unsafe-inline'; connect-src 'self'`.
  Tauri auto-appends nonces/hashes. (Source: https://v2.tauri.app/security/csp/) → **new plan Task**.
- **Least-privilege capabilities.** Tauri 2's default capability grants `core:*`
  only — **no `fs`, no `shell`**. Do NOT add broad `fs`/`shell` permissions. All
  privileged work (spawn exe, read registry, walk disk) happens in **our Rust
  commands**, never via an exposed shell/fs plugin to JS.
  (Source: https://v2.tauri.app/security/capabilities/) → **new plan Task**.
- **Never load remote content** into the webview; bundle all assets locally
  (preserves the offline guarantee and shrinks XSS surface).
- **Validate command args at the boundary** (TB2 is the system edge): every
  `#[tauri::command]` validates inputs (id exists, path under known root, enum
  values) before acting; internal fns then trust their types.

## TB3 — Launching executables  *(highest-severity local footgun)*
**Threat:** Spawning an arbitrary `.exe` → wrong/malicious binary, **argument
injection**, **DLL search-order hijack** via a bad working directory, or launching
something a crafted manifest pointed at.
**Mitigations:**
- **No shell.** Spawn via `std::process::Command` with an explicit program path and
  argv vector — **never** a shell string, never string interpolation.
- **Validate before spawn:** canonicalize `exe_path`; confirm it exists, is a file,
  has `.exe`, and resides under the game's recorded `install_path`. Reject otherwise.
- **cwd = exe's own directory** (PRD requirement) — reduces DLL-hijack ambiguity.
- **Never auto-launch on scan.** Launch only on explicit user action on a known
  library row (PRD footgun #3).
- Steam/Epic launches go through the **official URL protocol** via
  `tauri-plugin-opener`, not by spawning the store binary.

## TB4 — Optional network (SteamGridDB, feature `steamgriddb`, OFF by default)
**Threat:** Third-party responses are untrusted; MITM; key leakage.
**Mitigations:** HTTPS only; **validate/parse responses defensively** (treat as
untrusted — bound sizes, verify content-type/shape before decoding images); never
render API text as HTML; the feature is compiled out of the default build (ADR-0002).

## TB5 — SQLite injection
**Threat:** Game names/paths/tags (from manifests or user) concatenated into SQL.
**Mitigation:** **Parameterized queries only** (`rusqlite` `params!`/named params).
Never `format!` user/manifest data into SQL. (Maps to plan Task 3.)

## TB6 — Secrets (SteamGridDB API key)
**Threat:** Key in DB/config/logs/commit history.
**Mitigations:** Store **only** via `keyring` 4.x (Windows Credential Manager);
never in SQLite, never in `tracing` output (redact), never committed. `.gitignore`
already excludes env/secret files; verify no key literal before any commit.

## Privilege & unsafe
- **Do NOT run elevated** to read GPU counters. If NVIDIA `RmProfilingAdminOnly`
  gates `\GPU Engine\Utilization`, **degrade** ("GPU util unavailable") + NVML
  fallback — never request admin. (Verified: NVIDIA restricts perf counters to
  admin by default since driver ≥419.17, CVE-2018-6260.)
- **`unsafe` FFI** (PDH/DXGI/COM via `windows` crate) is confined to `os/` impls;
  validate every HRESULT/return, null-check pointers, free handles
  (`PdhCloseQuery`). No `unsafe` outside the OS seam.

## Supply chain
- Run **`cargo audit`** before each release (Rust equivalent of `npm audit`);
  triage by reachability. Run **`cargo deny`**/`cargo tree` to assert the default
  build pulls **no `reqwest`** (offline guarantee is auditable — ADR-0002).
- Scrutinize the few sensitive deps: `windows` (unsafe FFI), `keyring`,
  `reqwest` (feature-gated). Pin versions; review before bumping.

## Desktop security checklist (adapted)
```
Untrusted data
- [ ] No unwrap/expect on parsed manifest/registry/json data
- [ ] One malformed entry skips, never aborts the scan
- [ ] Paths canonicalized + bounded-to-root before use
IPC / WebView
- [ ] Restrictive CSP set (not null); no remote content loaded
- [ ] Capabilities limited to core:* + only what's needed; no broad fs/shell to JS
- [ ] Every command validates its inputs at the boundary
Launch
- [ ] Spawn via argv (no shell string); exe path validated + under install root
- [ ] cwd = exe dir; no auto-launch on scan
Data / secrets
- [ ] All SQL parameterized
- [ ] API key only in keyring; never in DB/logs/commits
- [ ] `git diff --cached` grep for secret-like strings before commit
Build
- [ ] cargo audit clean (no critical/high reachable)
- [ ] default build has zero network deps (cargo tree asserts no reqwest)
- [ ] unsafe confined to os/ seam; HRESULTs checked, handles freed
```

## Never do
- Spawn launches through a shell or interpolated command string.
- Add `shell:allow-execute`/broad `fs` capabilities to satisfy a feature — do it in Rust.
- Store or log the SteamGridDB key anywhere but the OS keychain.
- Elevate to read GPU counters.
- Load remote scripts/content into the WebView.
- `format!` untrusted strings into SQL.
