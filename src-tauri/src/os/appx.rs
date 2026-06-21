//! AppX/MSIX seam (ADR-0005, ADR-0006) for Xbox / Microsoft Store games.
//!
//! Discovery is **offline**: it reads the installed-package set the OS already
//! holds via WinRT `Windows.Management.Deployment.PackageManager` — no network.
//! The real impl is confined here (the only WinRT/COM surface in the crate); the
//! scanner consumes the trait so its mapping logic is unit-tested against a fake.

use crate::error::CoreResult;

/// One launchable Store/Xbox app: the AUMID (for `shell:AppsFolder` launch), a
/// display name, and the package install path (existence-checked by the scanner).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppxGame {
    /// Application User Model ID, `<PackageFamilyName>!<AppId>`.
    pub aumid: String,
    pub name: String,
    pub install_path: String,
}

/// Read-only enumeration of installed Store/Xbox **games** (already filtered).
pub trait Appx {
    /// Installed game packages, or `Ok(vec![])` when none / the API is
    /// unavailable. Best-effort: never errors on a missing or flaky package.
    fn installed_games(&self) -> CoreResult<Vec<AppxGame>>;
}

/// Whether a package is a game we surface. Conservative on purpose: a missed
/// game is better than polluting the library with non-game Store apps (Spotify,
/// Netflix…). Signal = Store-signed, not a framework, and installed under an
/// `XboxGames` directory (where the Xbox app puts PC games).
///
/// ASSUMPTION (verify on a real install): Xbox-app / Game Pass titles report an
/// `InstalledPath` under `…\XboxGames\…`. Pure-UWP Store games under
/// `WindowsApps` are out of scope for v2.
pub(crate) fn is_game_package(
    install_path: &str,
    is_framework: bool,
    signature_is_store: bool,
) -> bool {
    signature_is_store && !is_framework && install_path.to_ascii_lowercase().contains("xboxgames")
}

#[cfg(windows)]
pub use windows_impl::WindowsAppx;

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use crate::error::CoreError;
    use windows::ApplicationModel::PackageSignatureKind;
    use windows::Management::Deployment::PackageManager;
    use windows::Win32::System::Com::{CoInitializeEx, CoUninitialize, COINIT_MULTITHREADED};

    /// Real AppX access. Zero-sized so it shares by `&` across the scan scope
    /// (like `WindowsRegistry`/`WindowsFs`/`WindowsWmi`). Not unit-tested —
    /// exercised manually on Windows; the testable logic lives in `is_game_package`
    /// and the scanner.
    pub struct WindowsAppx;

    impl Appx for WindowsAppx {
        fn installed_games(&self) -> CoreResult<Vec<AppxGame>> {
            // WinRT `.get()` on the async AppListEntries call needs an MTA thread;
            // the Tauri command thread may be STA, where it would deadlock. A fresh
            // thread initialized MTA avoids that (mirrors `os::wmi`'s rationale).
            std::thread::scope(|s| {
                s.spawn(enumerate)
                    .join()
                    .unwrap_or_else(|_| Err(CoreError::Unsupported("appx thread panicked".into())))
            })
        }
    }

    fn enumerate() -> CoreResult<Vec<AppxGame>> {
        // SAFETY: balanced Co(Un)Initialize on this thread; HRESULT ignored —
        // a prior init (S_FALSE) is fine, we just need an MTA apartment.
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let result = collect();
        unsafe { CoUninitialize() };
        result
    }

    fn collect() -> CoreResult<Vec<AppxGame>> {
        let pm = PackageManager::new().map_err(appx_err)?;
        // All installed packages (offline, no network). May require elevation on
        // some systems; best-effort — denied/empty just yields no Xbox games.
        let packages = pm.FindPackages().map_err(appx_err)?;

        let mut games = Vec::new();
        for pkg in packages {
            let is_framework = pkg.IsFramework().unwrap_or(true);
            let is_store = pkg
                .SignatureKind()
                .map(|k| k == PackageSignatureKind::Store)
                .unwrap_or(false);
            let install_path = pkg
                .InstalledPath()
                .map(|h| h.to_string())
                .unwrap_or_default();
            if !is_game_package(&install_path, is_framework, is_store) {
                continue;
            }
            // AUMID + display name come from the package's app-list entries.
            let Ok(op) = pkg.GetAppListEntriesAsync() else {
                continue;
            };
            let Ok(entries) = op.get() else { continue };
            for entry in entries {
                let Ok(aumid) = entry.AppUserModelId() else {
                    continue;
                };
                let name = entry
                    .DisplayInfo()
                    .and_then(|d| d.DisplayName())
                    .map(|h| h.to_string())
                    .unwrap_or_default();
                games.push(AppxGame {
                    aumid: aumid.to_string(),
                    name,
                    install_path: install_path.clone(),
                });
            }
        }
        Ok(games)
    }

    fn appx_err(e: windows::core::Error) -> CoreError {
        CoreError::Unsupported(format!("appx: {e}"))
    }
}

#[cfg(test)]
pub use fake::FakeAppx;

#[cfg(test)]
mod fake {
    use super::*;

    /// In-memory AppX source for tests.
    #[derive(Default)]
    pub struct FakeAppx {
        games: Vec<AppxGame>,
    }

    impl FakeAppx {
        pub fn new() -> Self {
            Self::default()
        }
        pub fn with_game(mut self, aumid: &str, name: &str, install_path: &str) -> Self {
            self.games.push(AppxGame {
                aumid: aumid.into(),
                name: name.into(),
                install_path: install_path.into(),
            });
            self
        }
    }

    impl Appx for FakeAppx {
        fn installed_games(&self) -> CoreResult<Vec<AppxGame>> {
            Ok(self.games.clone())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn game_package_accepts_store_game_under_xboxgames() {
        assert!(is_game_package(r"D:\XboxGames\Halo\Content", false, true));
        // Case-insensitive on the directory marker.
        assert!(is_game_package(r"c:\xboxgames\forza\content", false, true));
    }

    #[test]
    fn game_package_rejects_framework_non_store_and_non_xbox_paths() {
        assert!(!is_game_package(r"D:\XboxGames\Halo\Content", true, true)); // framework
        assert!(!is_game_package(r"D:\XboxGames\Halo\Content", false, false)); // not store
        assert!(!is_game_package(
            r"C:\Program Files\WindowsApps\Spotify",
            false,
            true
        )); // not xbox
    }

    #[test]
    fn fake_returns_seeded_games() {
        let appx =
            FakeAppx::new().with_game("Pub.Game_8w!App", "Halo", r"D:\XboxGames\Halo\Content");
        let games = appx.installed_games().unwrap();
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].aumid, "Pub.Game_8w!App");
    }

    #[test]
    fn fake_empty_by_default() {
        assert!(FakeAppx::new().installed_games().unwrap().is_empty());
    }
}
