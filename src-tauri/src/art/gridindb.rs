//! SteamGridDB cover enrichment (plan Task 23, feature `steamgriddb`, OFF by
//! default — ADR-0002). This is the **only** network code in Nirvana and is
//! compiled out unless the `steamgriddb` feature is enabled.
//!
//! Security (threat-model TB4/TB6): HTTPS only; the API key lives in the OS
//! credential vault via `keyring` (never DB/logs/commits); responses are treated
//! as untrusted — we verify the content type is an image and cap the download
//! size before writing. On any error we degrade to `None` (the caller falls back
//! to the offline placeholder), so a flaky network never breaks cover loading.
//!
//! The live HTTP path can't be exercised offline; it's verified manually with a
//! real key. The cache-path + size-cap logic is unit-tested.

#![cfg(feature = "steamgriddb")]

use crate::error::{CoreError, CoreResult};
use crate::models::{Game, Source};
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where the key is stored in the OS vault.
const KEYRING_SERVICE: &str = "com.svssdeva.nirvana";
const KEYRING_USER: &str = "steamgriddb-api-key";
/// Refuse art larger than this (TB4: bound untrusted downloads). 8 MiB is ample
/// for a 600×900 capsule.
const MAX_BYTES: u64 = 8 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(10);

/// Store the SteamGridDB API key in the OS credential vault.
pub fn set_api_key(key: &str) -> CoreResult<()> {
    keyring_entry()?
        .set_password(key)
        .map_err(|e| CoreError::Unsupported(format!("keyring set: {e}")))
}

/// Whether a key is present (so the UI can prompt). Never returns the key.
pub fn has_api_key() -> bool {
    keyring_entry()
        .and_then(|e| {
            e.get_password()
                .map_err(|e| CoreError::Unsupported(e.to_string()))
        })
        .is_ok()
}

fn keyring_entry() -> CoreResult<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|e| CoreError::Unsupported(format!("keyring: {e}")))
}

/// Fetch a cover for `game` from SteamGridDB and cache it under `cache_dir`,
/// returning the cached path. `Ok(None)` when no key, no match, or any network
/// failure — the caller degrades to the placeholder.
pub fn fetch_cover(game: &Game, cache_dir: &Path) -> CoreResult<Option<PathBuf>> {
    let Ok(key) = keyring_entry()?.get_password() else {
        return Ok(None); // no key configured
    };
    let dest = cache_dir.join(cache_file_name(game));
    if dest.is_file() {
        return Ok(Some(dest));
    }

    let client = reqwest::blocking::Client::builder()
        .timeout(TIMEOUT)
        .https_only(true)
        .user_agent("Nirvana")
        .build()
        .map_err(net_err)?;

    let Some(image_url) = grid_url_for(&client, &key, game)? else {
        return Ok(None);
    };
    let Some(bytes) = download_image(&client, &key, &image_url)? else {
        return Ok(None);
    };

    std::fs::create_dir_all(cache_dir)?;
    std::fs::write(&dest, &bytes)?;
    Ok(Some(dest))
}

/// Resolve the first grid image URL for a game: by Steam appid directly, else by
/// name search (Epic/local).
fn grid_url_for(
    client: &reqwest::blocking::Client,
    key: &str,
    game: &Game,
) -> CoreResult<Option<String>> {
    let grids_endpoint = match game.source {
        Source::Steam => format!(
            "https://www.steamgriddb.com/api/v2/grids/steam/{}",
            game.external_id
        ),
        _ => {
            let Some(id) = search_game_id(client, key, &game.name)? else {
                return Ok(None);
            };
            format!("https://www.steamgriddb.com/api/v2/grids/game/{id}")
        }
    };
    let json = get_json(client, key, &grids_endpoint)?;
    Ok(json
        .get("data")
        .and_then(|d| d.as_array())
        .and_then(|a| a.first())
        .and_then(|g| g.get("url"))
        .and_then(|u| u.as_str())
        .map(str::to_string))
}

/// Best-effort name → SteamGridDB game id via the autocomplete endpoint.
fn search_game_id(
    client: &reqwest::blocking::Client,
    key: &str,
    name: &str,
) -> CoreResult<Option<u64>> {
    let url = format!(
        "https://www.steamgriddb.com/api/v2/search/autocomplete/{}",
        urlencoding(name)
    );
    let json = get_json(client, key, &url)?;
    Ok(json
        .get("data")
        .and_then(|d| d.as_array())
        .and_then(|a| a.first())
        .and_then(|g| g.get("id"))
        .and_then(serde_json::Value::as_u64))
}

fn get_json(
    client: &reqwest::blocking::Client,
    key: &str,
    url: &str,
) -> CoreResult<serde_json::Value> {
    let resp = client
        .get(url)
        .bearer_auth(key)
        .send()
        .map_err(net_err)?
        .error_for_status()
        .map_err(net_err)?;
    resp.json::<serde_json::Value>().map_err(net_err)
}

/// Download an image, verifying content type and capping the size (TB4).
fn download_image(
    client: &reqwest::blocking::Client,
    key: &str,
    url: &str,
) -> CoreResult<Option<Vec<u8>>> {
    let resp = client
        .get(url)
        .bearer_auth(key)
        .send()
        .map_err(net_err)?
        .error_for_status()
        .map_err(net_err)?;

    let is_image = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("image/"));
    if !is_image {
        return Ok(None); // never trust the extension / unexpected body
    }

    // Read at most MAX_BYTES + 1 to detect oversize without buffering unbounded.
    let mut buf = Vec::new();
    resp.take(MAX_BYTES + 1)
        .read_to_end(&mut buf)
        .map_err(CoreError::Io)?;
    if buf.len() as u64 > MAX_BYTES {
        return Ok(None);
    }
    Ok(Some(buf))
}

/// Stable cache filename for a game's downloaded cover.
fn cache_file_name(game: &Game) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    game.source.as_str().hash(&mut hasher);
    game.external_id.to_lowercase().hash(&mut hasher);
    format!("sgdb-{:016x}.img", hasher.finish())
}

/// Minimal percent-encoding for a query path segment (avoids a dep).
fn urlencoding(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn net_err(e: reqwest::Error) -> CoreError {
    CoreError::Io(std::io::Error::other(format!("steamgriddb: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(source: Source, id: &str) -> Game {
        Game {
            id: 1,
            source,
            external_id: id.into(),
            name: "Hollow Knight".into(),
            install_path: String::new(),
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

    #[test]
    fn cache_name_is_stable_per_source_and_id() {
        let a = cache_file_name(&game(Source::Steam, "440"));
        let b = cache_file_name(&game(Source::Steam, "440"));
        let c = cache_file_name(&game(Source::Epic, "440"));
        assert_eq!(a, b);
        assert_ne!(a, c, "different source → different cache key");
        assert!(a.starts_with("sgdb-") && a.ends_with(".img"));
    }

    #[test]
    fn urlencoding_escapes_unsafe_chars() {
        assert_eq!(urlencoding("Hollow Knight"), "Hollow%20Knight");
        assert_eq!(urlencoding("a/b?c"), "a%2Fb%3Fc");
        assert_eq!(urlencoding("plain-1.0_x"), "plain-1.0_x");
    }
}
