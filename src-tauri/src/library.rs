//! Library filtering / sorting / search (plan Task 20).
//!
//! Pure transforms over a `Vec<Game>`, so the query logic is unit-tested; the
//! `get_library` command fetches the persisted list and applies the query. The
//! library is small (hundreds of games), so in-memory filtering is simpler and
//! avoids dynamic SQL.

use crate::models::{Game, Source};
use serde::Deserialize;

/// Sort key for the library grid.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SortBy {
    #[default]
    Name,
    Size,
    LastPlayed,
}

/// Filter + sort + search request from the UI (all fields optional).
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryQuery {
    #[serde(default)]
    pub search: Option<String>,
    #[serde(default)]
    pub source: Option<Source>,
    #[serde(default)]
    pub drive: Option<String>,
    #[serde(default)]
    pub favorites_only: bool,
    #[serde(default)]
    pub tag: Option<String>,
    /// Only games in this collection id (resolved in `get_library`, not here —
    /// membership needs the DB, so `apply_query` ignores it).
    #[serde(default)]
    pub collection: Option<i64>,
    #[serde(default)]
    pub sort: SortBy,
    #[serde(default)]
    pub descending: bool,
}

/// Filter, then sort `games` per `query`. Search is a case-insensitive substring
/// match on the name; unknown sizes/last-played sort lowest (so they trail in a
/// descending "biggest"/"recent" view).
pub fn apply_query(mut games: Vec<Game>, query: &LibraryQuery) -> Vec<Game> {
    let needle = query
        .search
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase);

    games.retain(|g| {
        query.source.is_none_or(|s| g.source == s)
            && query
                .drive
                .as_deref()
                .is_none_or(|d| g.drive.as_deref() == Some(d))
            && (!query.favorites_only || g.favorite)
            && query
                .tag
                .as_deref()
                .is_none_or(|t| g.tags.iter().any(|gt| gt.eq_ignore_ascii_case(t)))
            && needle
                .as_deref()
                .is_none_or(|n| g.name.to_lowercase().contains(n))
    });

    match query.sort {
        SortBy::Name => games.sort_by_key(|g| g.name.to_lowercase()),
        SortBy::Size => games.sort_by_key(|g| g.size_bytes.unwrap_or(-1)),
        SortBy::LastPlayed => games.sort_by_key(|g| g.last_played.unwrap_or(i64::MIN)),
    }
    if query.descending {
        games.reverse();
    }
    games
}

#[cfg(test)]
mod tests {
    use super::*;

    fn game(name: &str, source: Source, size: Option<i64>, favorite: bool) -> Game {
        Game {
            id: 0,
            source,
            external_id: name.into(),
            name: name.into(),
            install_path: format!(r"C:\g\{name}"),
            exe_path: None,
            size_bytes: size,
            drive: Some("C:".into()),
            last_played: None,
            launch_count: 0,
            cover_path: None,
            favorite,
            tags: Vec::new(),
        }
    }

    fn names(games: &[Game]) -> Vec<&str> {
        games.iter().map(|g| g.name.as_str()).collect()
    }

    fn sample() -> Vec<Game> {
        vec![
            game("Celeste", Source::Steam, Some(1_500), false),
            game("Warframe", Source::Epic, Some(45_000), true),
            game("alpha", Source::Local, None, false),
        ]
    }

    #[test]
    fn default_query_sorts_by_name_case_insensitive() {
        let out = apply_query(sample(), &LibraryQuery::default());
        assert_eq!(names(&out), ["alpha", "Celeste", "Warframe"]);
    }

    #[test]
    fn filters_by_source() {
        let q = LibraryQuery {
            source: Some(Source::Epic),
            ..Default::default()
        };
        assert_eq!(names(&apply_query(sample(), &q)), ["Warframe"]);
    }

    #[test]
    fn favorites_only_keeps_favorites() {
        let q = LibraryQuery {
            favorites_only: true,
            ..Default::default()
        };
        assert_eq!(names(&apply_query(sample(), &q)), ["Warframe"]);
    }

    #[test]
    fn filters_by_tag_case_insensitively() {
        let mut games = sample();
        games[0].tags = vec!["Indie".into(), "Platformer".into()];
        games[1].tags = vec!["Looter".into()];
        let q = LibraryQuery {
            tag: Some("indie".into()),
            ..Default::default()
        };
        assert_eq!(names(&apply_query(games, &q)), ["Celeste"]);
    }

    #[test]
    fn search_matches_name_case_insensitively() {
        let q = LibraryQuery {
            search: Some("  CEL  ".into()),
            ..Default::default()
        };
        assert_eq!(names(&apply_query(sample(), &q)), ["Celeste"]);
    }

    #[test]
    fn sort_by_size_descending_puts_biggest_first_unknown_last() {
        let q = LibraryQuery {
            sort: SortBy::Size,
            descending: true,
            ..Default::default()
        };
        assert_eq!(
            names(&apply_query(sample(), &q)),
            ["Warframe", "Celeste", "alpha"]
        );
    }
}
