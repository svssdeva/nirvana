//! Xbox / Microsoft Store discovery (FUTURE-PLANS Priority 1). Offline: the
//! AppX seam (`os::appx`) enumerates installed game packages; this maps them to
//! [`Game`]s, skipping any whose install folder is gone (uninstalled leftover,
//! mirrors `gog.rs`). `external_id` is the AUMID — what `launch::shell_app_url`
//! needs to launch via `shell:AppsFolder`.

use crate::error::CoreResult;
use crate::models::{Game, Source};
use crate::os::{Appx, FileSystem};
use crate::scan::drive_of;
use crate::scan::store::ScanCtx;
use std::path::Path;

/// The `scan` fn the Xbox [`crate::scan::store::Descriptor`] points at.
pub fn scan(ctx: &ScanCtx) -> CoreResult<Vec<Game>> {
    Ok(games_from_appx(ctx.appx, ctx.fs))
}

/// Map installed AppX games → [`Game`]s, dropping entries whose install folder no
/// longer exists. Pure over the seams, so it's unit-tested without a real install.
fn games_from_appx(appx: &dyn Appx, fs: &dyn FileSystem) -> Vec<Game> {
    appx.installed_games()
        .unwrap_or_default()
        .into_iter()
        .filter(|g| {
            fs.metadata(Path::new(&g.install_path))
                .map(|m| m.is_dir)
                .unwrap_or(false)
        })
        .map(|g| Game {
            id: 0,
            source: Source::Xbox,
            external_id: g.aumid,
            name: g.name,
            drive: drive_of(Path::new(&g.install_path)),
            install_path: g.install_path,
            exe_path: None,
            size_bytes: None,
            last_played: None,
            launch_count: 0,
            cover_path: None,
            favorite: false,
            tags: Vec::new(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::os::appx::FakeAppx;
    use crate::os::fs::FakeFs;

    #[test]
    fn maps_installed_xbox_game() {
        let appx =
            FakeAppx::new().with_game("Pub.Halo_8w!App", "Halo", r"D:\XboxGames\Halo\Content");
        let fs = FakeFs::new().with_dir(r"D:\XboxGames\Halo\Content", vec![]);
        let games = games_from_appx(&appx, &fs);
        assert_eq!(games.len(), 1);
        let g = &games[0];
        assert_eq!(g.source, Source::Xbox);
        assert_eq!(g.external_id, "Pub.Halo_8w!App");
        assert_eq!(g.name, "Halo");
        assert_eq!(g.drive.as_deref(), Some("D:"));
    }

    #[test]
    fn skips_xbox_game_whose_install_folder_is_gone() {
        let appx =
            FakeAppx::new().with_game("Pub.Halo_8w!App", "Halo", r"D:\XboxGames\Halo\Content");
        let fs = FakeFs::new(); // install dir NOT seeded → uninstalled leftover
        assert!(games_from_appx(&appx, &fs).is_empty());
    }

    #[test]
    fn empty_when_no_packages() {
        let fs = FakeFs::new();
        assert!(games_from_appx(&FakeAppx::new(), &fs).is_empty());
    }
}
