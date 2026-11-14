//! The open game's package list: which index it shows, and whether the cache
//! is being read, a refresh is running, or the last refresh failed.

use crate::{Command, Effect, State};
use std::sync::Arc;
use thunderstore_engine::models::PackageIndex;
use thunderstore_engine::query::{self, BrowseQuery};

/// A package index shared between the commands thread and the state.
///
/// Two are equal when they are the same index, not when their contents match,
/// so comparing messages never walks tens of thousands of packages.
#[derive(Debug, Clone)]
pub struct Index(pub Arc<PackageIndex>);

impl PartialEq for Index {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.0, &other.0)
  }
}

impl Eq for Index {}

impl From<PackageIndex> for Index {
  fn from(index: PackageIndex) -> Self {
    Self(Arc::new(index))
  }
}

/// One fetch of a community's package index. `id` is unique per app run, so an
/// answer can be matched to the fetch that is still awaited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refresh {
  pub community: String,
  pub id: u64,
}

/// The package list of the open game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Browse {
  /// The Thunderstore community whose packages are listed.
  pub community: String,
  /// The latest index, from the cache or a refresh. `None` until one arrives.
  pub index: Option<Index>,
  /// The packages to show, as positions in `index`, in display order.
  pub rows: Vec<usize>,
  pub status: Status,
}

/// Where the package list is in loading or refreshing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
  /// The cached index is being read. No network request has been made.
  ReadingCache,
  /// The index is being fetched from Thunderstore, by the [`Refresh`] with
  /// this id.
  Refreshing(u64),
  /// The last fetch succeeded.
  Refreshed,
  /// The last fetch failed, with the reason.
  Failed(String),
}

/// What the package list area shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View<'a> {
  /// No index yet, and one is on its way from the cache or the network.
  Loading,
  /// No index is cached and the fetch failed, with the reason.
  Unavailable(&'a str),
  /// The rows of the index.
  List {
    /// A refresh is running behind the list.
    refreshing: bool,
    /// The last refresh failed, with the reason, so the list may be out of
    /// date.
    stale: Option<&'a str>,
  },
}

impl Browse {
  /// A list for `community` that is about to read its cache.
  pub fn new(community: String) -> Self {
    Self {
      community,
      index: None,
      rows: Vec::new(),
      status: Status::ReadingCache,
    }
  }

  /// What the package list area shows.
  pub fn view(&self) -> View<'_> {
    let busy = matches!(self.status, Status::ReadingCache | Status::Refreshing(_));
    let failure = match &self.status {
      Status::Failed(reason) => Some(reason.as_str()),
      _ => None,
    };

    match (&self.index, failure) {
      (Some(_), stale) => View::List {
        refreshing: busy,
        stale,
      },
      (None, Some(reason)) => View::Unavailable(reason),
      (None, None) => View::Loading,
    }
  }

  /// Shows `index`, listing its packages with r2modman's browse defaults.
  fn show(&mut self, index: Index) {
    self.rows = query::browse(&index.0, &BrowseQuery::new());
    self.index = Some(index);
  }

  /// Starts a fetch, numbered from `refreshes`, the count of fetches started
  /// so far.
  fn refresh(&mut self, refreshes: &mut u64, fetch: Fetch) -> Vec<Effect> {
    *refreshes += 1;
    self.status = Status::Refreshing(*refreshes);

    vec![Effect::Run(Command::RefreshIndex {
      refresh: Refresh {
        community: self.community.clone(),
        id: *refreshes,
      },
      fetch,
    })]
  }
}

/// How much of the index a refresh downloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fetch {
  /// Download it only when Thunderstore has a newer one than the cache.
  /// Answered with no index when the cache is current.
  IfStale,
  /// Download it whatever the cache holds.
  Always,
}

/// Applies a cache read for `community`, then refreshes from the network.
pub(crate) fn cache_read(
  state: &mut State,
  community: String,
  index: Result<Option<Index>, String>,
) -> Vec<Effect> {
  let Some(browse) = state
    .browse
    .as_mut()
    .filter(|browse| browse.community == community)
  else {
    return Vec::new();
  };

  if browse.status != Status::ReadingCache {
    return Vec::new();
  }

  // An unreadable cache is dropped rather than reported: the full download
  // that follows replaces it, and reports its own failure if it cannot. A
  // staleness check would not do, as it can call a corrupt cache current.
  let fetch = match index {
    Ok(Some(index)) => {
      browse.show(index);

      Fetch::IfStale
    }
    Ok(None) | Err(_) => Fetch::Always,
  };

  browse.refresh(&mut state.refreshes, fetch)
}

/// Fetches the list again, unless it is already loading.
pub(crate) fn refresh_pressed(state: &mut State) -> Vec<Effect> {
  let Some(browse) = state.browse.as_mut() else {
    return Vec::new();
  };

  if matches!(browse.status, Status::ReadingCache | Status::Refreshing(_)) {
    return Vec::new();
  }

  browse.refresh(&mut state.refreshes, Fetch::Always)
}

/// Applies the answer to `refresh`: a new index, `None` when the cached one
/// is current, or why the fetch failed.
pub(crate) fn refreshed(
  state: &mut State,
  refresh: Refresh,
  index: Result<Option<Index>, String>,
) -> Vec<Effect> {
  let Some(browse) = state.browse.as_mut().filter(|browse| {
    browse.community == refresh.community && browse.status == Status::Refreshing(refresh.id)
  }) else {
    return Vec::new();
  };

  match index {
    Ok(Some(index)) => {
      browse.show(index);
      browse.status = Status::Refreshed;
    }
    Ok(None) if browse.index.is_none() => {
      return browse.refresh(&mut state.refreshes, Fetch::Always);
    }
    Ok(None) => browse.status = Status::Refreshed,
    Err(reason) => browse.status = Status::Failed(reason),
  }

  Vec::new()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::settings::{GameSettings, Settings};
  use crate::{Command, Effect, Msg, State, test_ecosystem, update};
  use std::path::PathBuf;

  /// A state that started with `game` open, its cache not yet read.
  fn opened(game: &str) -> State {
    let mut state = State::new(test_ecosystem());
    let settings = Settings {
      last_game: Some(game.to_string()),
      games: [(
        game.to_string(),
        GameSettings {
          folder: PathBuf::from("/games").join(game),
        },
      )]
      .into(),
    };

    update(&mut state, Msg::Started);
    update(&mut state, Msg::SettingsLoaded(Ok(settings)));

    state
  }

  fn view(state: &State) -> Option<View<'_>> {
    state.browse.as_ref().map(Browse::view)
  }

  /// A one-version package last updated on `updated_day` of January 2024.
  fn package(full_name: &str, updated_day: u32) -> serde_json::Value {
    let (owner, name) = full_name.split_once('-').unwrap();

    serde_json::json!({
      "name": name,
      "full_name": full_name,
      "owner": owner,
      "date_created": "2024-01-01T00:00:00Z",
      "date_updated": format!("2024-01-{updated_day:02}T00:00:00Z"),
      "categories": [],
      "versions": [{
        "version_number": "1.0.0",
        "dependencies": [],
        "date_created": "2024-01-01T00:00:00Z"
      }]
    })
  }

  fn index(packages: Vec<serde_json::Value>) -> Index {
    let packages: Vec<thunderstore_engine::models::Package> =
      serde_json::from_value(serde_json::Value::Array(packages)).unwrap();

    PackageIndex::from(packages).into()
  }

  fn two_packages() -> Index {
    index(vec![package("Owner-Older", 1), package("Owner-Newer", 2)])
  }

  /// The full names of the listed packages, in display order.
  fn listed(state: &State) -> Vec<String> {
    let browse = state.browse.as_ref().unwrap();
    let index = &browse.index.as_ref().unwrap().0;

    browse
      .rows
      .iter()
      .map(|&row| index.full_name_at(row).unwrap().to_string())
      .collect()
  }

  fn cache_read(community: &str, index: Result<Option<Index>, &str>) -> Msg {
    Msg::CachedIndexRead {
      community: community.to_string(),
      index: index.map_err(str::to_string),
    }
  }

  fn refresh_of(community: &str, id: u64) -> Refresh {
    Refresh {
      community: community.to_string(),
      id,
    }
  }

  fn refreshed(community: &str, id: u64, index: Result<Index, &str>) -> Msg {
    Msg::IndexRefreshed {
      refresh: refresh_of(community, id),
      index: index.map(Some).map_err(str::to_string),
    }
  }

  fn unchanged(community: &str, id: u64) -> Msg {
    Msg::IndexRefreshed {
      refresh: refresh_of(community, id),
      index: Ok(None),
    }
  }

  fn refresh(community: &str, id: u64) -> Effect {
    Effect::Run(Command::RefreshIndex {
      refresh: refresh_of(community, id),
      fetch: Fetch::Always,
    })
  }

  fn refresh_if_stale(community: &str, id: u64) -> Effect {
    Effect::Run(Command::RefreshIndex {
      refresh: refresh_of(community, id),
      fetch: Fetch::IfStale,
    })
  }

  #[test]
  fn an_unchanged_index_keeps_the_cached_list() {
    let mut state = opened("valheim");

    update(&mut state, cache_read("valheim", Ok(Some(two_packages()))));

    let effects = update(&mut state, unchanged("valheim", 1));

    assert_eq!(effects, vec![]);
    assert_eq!(listed(&state), ["Owner-Newer", "Owner-Older"]);
    assert_eq!(
      view(&state),
      Some(View::List {
        refreshing: false,
        stale: None
      })
    );
  }

  #[test]
  fn an_unchanged_answer_with_no_list_to_keep_fetches_it_in_full() {
    let mut state = opened("valheim");

    update(&mut state, cache_read("valheim", Ok(None)));

    let effects = update(&mut state, unchanged("valheim", 1));

    assert_eq!(effects, vec![refresh("valheim", 2)]);
    assert_eq!(view(&state), Some(View::Loading));
  }

  #[test]
  fn a_refresh_from_an_earlier_visit_to_the_game_is_ignored() {
    let mut state = opened("valheim");

    update(&mut state, cache_read("valheim", Ok(Some(two_packages()))));
    update(&mut state, Msg::SwitchGamePressed);
    update(&mut state, Msg::GamePicked("valheim".to_string()));

    let effects = update(&mut state, cache_read("valheim", Ok(Some(two_packages()))));

    assert_eq!(effects, vec![refresh_if_stale("valheim", 2)]);

    let earlier = update(&mut state, refreshed("valheim", 1, Err("offline")));

    assert_eq!(earlier, vec![]);
    assert_eq!(
      view(&state),
      Some(View::List {
        refreshing: true,
        stale: None
      })
    );

    update(&mut state, refreshed("valheim", 2, Ok(two_packages())));

    assert_eq!(
      view(&state),
      Some(View::List {
        refreshing: false,
        stale: None
      })
    );
  }

  #[test]
  fn opening_a_game_shows_loading_while_its_cache_is_read() {
    let state = opened("valheim");

    assert_eq!(view(&state), Some(View::Loading));
  }

  #[test]
  fn a_cached_list_is_shown_at_once_and_refreshed_behind_it() {
    let mut state = opened("valheim");

    let effects = update(&mut state, cache_read("valheim", Ok(Some(two_packages()))));

    assert_eq!(listed(&state), ["Owner-Newer", "Owner-Older"]);
    assert_eq!(
      view(&state),
      Some(View::List {
        refreshing: true,
        stale: None
      })
    );
    assert_eq!(effects, vec![refresh_if_stale("valheim", 1)]);
  }

  #[test]
  fn without_a_cache_loading_shows_until_the_first_fetch_arrives() {
    let mut state = opened("valheim");

    let effects = update(&mut state, cache_read("valheim", Ok(None)));

    assert_eq!(view(&state), Some(View::Loading));
    assert_eq!(effects, vec![refresh("valheim", 1)]);

    let effects = update(&mut state, refreshed("valheim", 1, Ok(two_packages())));

    assert_eq!(listed(&state), ["Owner-Newer", "Owner-Older"]);
    assert_eq!(
      view(&state),
      Some(View::List {
        refreshing: false,
        stale: None
      })
    );
    assert_eq!(effects, vec![]);
  }

  #[test]
  fn a_failed_refresh_keeps_the_cached_list_and_says_why() {
    let mut state = opened("valheim");

    update(&mut state, cache_read("valheim", Ok(Some(two_packages()))));
    update(&mut state, refreshed("valheim", 1, Err("offline")));

    assert_eq!(listed(&state), ["Owner-Newer", "Owner-Older"]);
    assert_eq!(
      view(&state),
      Some(View::List {
        refreshing: false,
        stale: Some("offline")
      })
    );
  }

  #[test]
  fn a_failed_first_fetch_leaves_the_list_unavailable() {
    let mut state = opened("valheim");

    update(&mut state, cache_read("valheim", Ok(None)));
    update(&mut state, refreshed("valheim", 1, Err("offline")));

    assert_eq!(view(&state), Some(View::Unavailable("offline")));
  }

  #[test]
  fn the_refresh_button_fetches_the_list_again() {
    let mut state = opened("valheim");

    update(&mut state, cache_read("valheim", Ok(Some(two_packages()))));
    update(&mut state, refreshed("valheim", 1, Err("offline")));

    let effects = update(&mut state, Msg::RefreshPressed);

    assert_eq!(effects, vec![refresh("valheim", 2)]);
    assert_eq!(
      view(&state),
      Some(View::List {
        refreshing: true,
        stale: None
      })
    );
  }

  #[test]
  fn the_refresh_button_does_nothing_while_the_list_is_loading() {
    let mut state = opened("valheim");

    assert_eq!(update(&mut state, Msg::RefreshPressed), vec![]);

    update(&mut state, cache_read("valheim", Ok(None)));

    assert_eq!(update(&mut state, Msg::RefreshPressed), vec![]);
  }

  #[test]
  fn answers_for_a_game_no_longer_open_are_ignored() {
    let mut state = opened("valheim");

    update(&mut state, Msg::SwitchGamePressed);
    update(&mut state, Msg::GamePicked("riskofrain2".to_string()));
    update(
      &mut state,
      Msg::FolderChosen(Some(PathBuf::from("/games/ror2"))),
    );

    let effects = update(
      &mut state,
      Msg::FolderListed {
        folder: PathBuf::from("/games/ror2"),
        entries: Ok(vec!["Risk of Rain 2.exe".to_string()]),
      },
    );

    assert!(effects.contains(&Effect::Run(Command::ReadCachedIndex(
      "riskofrain2".to_string()
    ))));

    let from_cache = update(&mut state, cache_read("valheim", Ok(Some(two_packages()))));
    let from_network = update(&mut state, refreshed("valheim", 1, Ok(two_packages())));

    assert_eq!(from_cache, vec![]);
    assert_eq!(from_network, vec![]);
    assert_eq!(view(&state), Some(View::Loading));
  }

  #[test]
  fn the_list_follows_r2modmans_browse_defaults() {
    let mut deprecated = package("Owner-Deprecated", 9);
    let mut nsfw = package("Owner-Nsfw", 9);
    let mut pinned = package("Owner-Pinned", 1);

    deprecated["is_deprecated"] = true.into();
    nsfw["has_nsfw_content"] = true.into();
    pinned["is_pinned"] = true.into();

    let mut state = opened("valheim");

    update(
      &mut state,
      cache_read(
        "valheim",
        Ok(Some(index(vec![
          package("Owner-Older", 2),
          deprecated,
          pinned,
          nsfw,
          package("Owner-Newer", 3),
        ]))),
      ),
    );

    assert_eq!(
      listed(&state),
      ["Owner-Pinned", "Owner-Newer", "Owner-Older"]
    );
  }

  #[test]
  fn leaving_the_game_drops_its_list() {
    let mut state = opened("valheim");

    update(&mut state, cache_read("valheim", Ok(Some(two_packages()))));
    update(&mut state, Msg::SwitchGamePressed);

    assert_eq!(state.browse, None);
  }

  #[test]
  fn an_unreadable_cache_is_replaced_by_a_fetch() {
    let mut state = opened("valheim");

    let effects = update(&mut state, cache_read("valheim", Err("corrupt cache")));

    assert_eq!(view(&state), Some(View::Loading));
    assert_eq!(effects, vec![refresh("valheim", 1)]);
  }
}
