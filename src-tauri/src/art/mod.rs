//! Cover art resolution (plan Task 10, FR-ART, offline-first).
//!
//! Resolves the best available cover for a game without any network: an already
//! cached path, then the Steam library-cache capsule ([`steam_cache`]), then the
//! exe icon ([`exe_icon`]), else a placeholder. The resolution order is pure
//! orchestration over the os seams, so it's unit-tested on any OS; the command
//! layer (`commands::get_cover`) supplies the real `FileSystem`/`IconExtractor`
//! and the Steam root.

pub mod exe_icon;
#[cfg(feature = "steamgriddb")]
pub mod gridindb;
pub mod steam_cache;

use crate::models::{Game, Source};
use crate::os::{FileSystem, IconExtractor};
use serde::Serialize;
use std::path::Path;

/// IPC cover reference — the discriminated union from `docs/api-contract.md`:
/// `{ type: "image", path }` or `{ type: "placeholder" }`. Never errors on a
/// missing cover; callers get [`CoverRef::Placeholder`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum CoverRef {
    Image { path: String },
    Placeholder,
}

/// Resolve a game's cover in FR-ART preference order. Pure over the seams:
/// `fs` probes the Steam cache, `icons` extracts an exe icon into
/// `icon_cache_dir`, `steam_root` is `None` when Steam isn't installed.
pub fn resolve_cover(
    game: &Game,
    fs: &dyn FileSystem,
    steam_root: Option<&Path>,
    icons: &dyn IconExtractor,
    icon_cache_dir: &Path,
) -> CoverRef {
    // 1. Already-resolved cover recorded on the game.
    if let Some(path) = game.cover_path.as_deref() {
        return CoverRef::Image {
            path: path.to_string(),
        };
    }
    // 2. Steam library-cache capsule (portrait preferred).
    if game.source == Source::Steam {
        if let Some(root) = steam_root {
            if let Some(found) = steam_cache::resolve(fs, root, &game.external_id) {
                return CoverRef::Image {
                    path: path_string(&found),
                };
            }
        }
    }
    // 3. Exe icon (mainly local games, which carry an exe_path).
    if let Some(exe) = game.exe_path.as_deref() {
        match exe_icon::resolve(icons, Path::new(exe), icon_cache_dir) {
            Ok(Some(found)) => {
                return CoverRef::Image {
                    path: path_string(&found),
                }
            }
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(exe, error = %e, "exe-icon extraction failed; using placeholder")
            }
        }
    }
    // 4. Nothing offline-available.
    CoverRef::Placeholder
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::os::fs::FakeFs;
    use crate::os::icon::FakeIcons;
    use crate::os::IconRgba;
    use std::path::PathBuf;

    const STEAM_ROOT: &str = r"C:\Program Files (x86)\Steam";

    fn steam_game(appid: &str) -> Game {
        Game {
            id: 1,
            source: Source::Steam,
            external_id: appid.into(),
            name: "Game".into(),
            install_path: r"C:\Steam\steamapps\common\Game".into(),
            exe_path: None,
            size_bytes: None,
            drive: None,
            last_played: None,
            launch_count: 0,
            cover_path: None,
            favorite: false,
            tags: Vec::new(),
        }
    }

    fn capsule(appid: &str) -> String {
        format!(r"{STEAM_ROOT}\appcache\librarycache\{appid}\library_600x900.jpg")
    }

    #[test]
    fn steam_game_resolves_to_cache_capsule() {
        let fs = FakeFs::new().with_file(capsule("440"), "jpg");
        let icons = FakeIcons::new();
        let cover = resolve_cover(
            &steam_game("440"),
            &fs,
            Some(Path::new(STEAM_ROOT)),
            &icons,
            Path::new(r"C:\cache"),
        );
        assert_eq!(
            cover,
            CoverRef::Image {
                path: capsule("440")
            }
        );
    }

    #[test]
    fn steam_game_without_cache_is_placeholder() {
        let fs = FakeFs::new();
        let icons = FakeIcons::new();
        let cover = resolve_cover(
            &steam_game("440"),
            &fs,
            Some(Path::new(STEAM_ROOT)),
            &icons,
            Path::new(r"C:\cache"),
        );
        assert_eq!(cover, CoverRef::Placeholder);
    }

    #[test]
    fn preset_cover_path_wins() {
        let mut game = steam_game("440");
        game.cover_path = Some(r"C:\cache\covers\preset.png".into());
        let fs = FakeFs::new();
        let icons = FakeIcons::new();
        let cover = resolve_cover(&game, &fs, None, &icons, Path::new(r"C:\cache"));
        assert_eq!(
            cover,
            CoverRef::Image {
                path: r"C:\cache\covers\preset.png".into()
            }
        );
    }

    #[test]
    fn local_game_falls_back_to_exe_icon() {
        let dir = tempfile::tempdir().unwrap();
        let exe = r"C:\Games\Foo\foo.exe";
        let mut game = steam_game("0");
        game.source = Source::Local;
        game.exe_path = Some(exe.into());
        let fs = FakeFs::new();
        let icons = FakeIcons::new().with_icon(
            PathBuf::from(exe),
            IconRgba {
                width: 8,
                height: 8,
                rgba: vec![0xff; 8 * 8 * 4],
            },
        );
        let cover = resolve_cover(&game, &fs, None, &icons, dir.path());
        match cover {
            CoverRef::Image { path } => assert!(path.ends_with(".png")),
            CoverRef::Placeholder => panic!("expected exe-icon image"),
        }
    }

    #[test]
    fn serializes_as_discriminated_union() {
        let img = serde_json::to_value(CoverRef::Image { path: "p".into() }).unwrap();
        assert_eq!(img["type"], "image");
        assert_eq!(img["path"], "p");
        let ph = serde_json::to_value(CoverRef::Placeholder).unwrap();
        assert_eq!(ph["type"], "placeholder");
    }
}
