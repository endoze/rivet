//! The open game's package list: which index it shows, and whether the cache
//! is being read, a refresh is running, or the last refresh failed.

use crate::notice::Notice;
use crate::{Command, Effect, State};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;
use thunderstore_engine::models::PackageIndex;
use thunderstore_engine::query::{self, BrowseQuery, PackageRow};
pub use thunderstore_engine::query::{SortDirection, SortKey};

/// A package index shared between the commands thread and the state.
///
/// Two are equal when they are the same index, not when their contents match,
/// so comparing messages never walks tens of thousands of packages.
#[derive(Debug, Clone)]
pub struct Index(pub Arc<PackageIndex>, Arc<HashMap<String, usize>>);

impl Index {
  /// The package named `full_name` (`Owner-Name`), or `None` when the index
  /// lacks it. Finds it without walking the index.
  pub fn package(&self, full_name: &str) -> Option<PackageRow<'_>> {
    let idx = *self.1.get(full_name)?;

    PackageRow::new(&self.0, idx)
  }

  /// Every version `full_name` (`Owner-Name`) has published, the most
  /// recent first. Empty when the index lacks it.
  pub fn versions(&self, full_name: &str) -> Vec<String> {
    let Some(mut versions) = self
      .1
      .get(full_name)
      .and_then(|&idx| self.0.get_package_at(idx))
      .map(|package| package.versions)
    else {
      return Vec::new();
    };

    versions.sort_by_key(|version| std::cmp::Reverse(version.date_created));

    versions
      .into_iter()
      .filter_map(|version| version.version_number)
      .collect()
  }
}

impl PartialEq for Index {
  fn eq(&self, other: &Self) -> bool {
    Arc::ptr_eq(&self.0, &other.0)
  }
}

impl Eq for Index {}

impl From<PackageIndex> for Index {
  fn from(index: PackageIndex) -> Self {
    let positions = index.build_name_index();

    Self(Arc::new(index), Arc::new(positions))
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
  /// The positions `rows` holds, to tell cheaply whether a package is listed.
  listed: HashSet<usize>,
  pub status: Status,
  /// What the user narrowed and ordered the list by.
  pub filters: Filters,
  /// Every category a package in `index` belongs to, in order.
  pub categories: Vec<String>,
}

/// Other mod managers published as packages, by `Owner-Name`, taken from
/// r2modman's exclusion list. They never appear in the list, so none is
/// installed as if it were a mod.
pub const HIDDEN_PACKAGES: &[&str] = &[
  "ebkr-r2modman",
  "ebkr-r2modman_dsp",
  "ethanbrews-RiskOfRainModManager",
  "ethanbrews-Forecast_Mod_Manager",
  "scottbot95-RoR2ModManager",
  "HoodedDeath-RiskOfDeathModManager",
  "Elaviers-GCManager",
  "MythicManiac-MythicModManager",
  "Foldex-r2mod_cli",
  "Kesomannen-GaleModManager",
];

/// The search, sort, and filters the user set on the package list.
///
/// The default is r2modman's browse defaults: no search, most recently
/// updated first, every category, NSFW and deprecated packages hidden.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filters {
  /// Words to find in a package's name, author, or description. Set it with
  /// [`Filters::set_search`] so the sort follows it.
  pub search: String,
  pub sort: SortKey,
  pub direction: SortDirection,
  /// Keeps packages in at least one of these. Empty keeps every package.
  pub categories: BTreeSet<String>,
  pub show_nsfw: bool,
  pub show_deprecated: bool,
  /// The sort that was active when the search went from empty to typed,
  /// brought back when the search empties while still sorted by
  /// [`SortKey::BestMatch`]. `None` while the search is empty.
  pub sort_before_search: Option<(SortKey, SortDirection)>,
}

impl Default for Filters {
  fn default() -> Self {
    Self {
      search: String::new(),
      sort: SortKey::LastUpdated,
      direction: SortDirection::Descending,
      categories: BTreeSet::new(),
      show_nsfw: false,
      show_deprecated: false,
      sort_before_search: None,
    }
  }
}

impl Filters {
  /// Whether the search or any filter differs from the defaults, so there is
  /// something to clear. The sort is not a filter.
  pub fn is_filtering(&self) -> bool {
    !self.search.is_empty() || !self.categories.is_empty() || self.show_nsfw || self.show_deprecated
  }

  /// Resets the search and filters, keeping the sort, or bringing back the one
  /// from before the search as emptying the search does.
  pub fn clear(&mut self) {
    self.set_search(String::new());

    *self = Self {
      sort: self.sort,
      direction: self.direction,
      ..Self::default()
    };
  }

  /// Searches for `search`. Typing into an empty search sorts by
  /// [`SortKey::BestMatch`], and emptying it brings back the sort from before
  /// unless another was picked while searching.
  pub fn set_search(&mut self, search: String) {
    if self.search.is_empty() && !search.is_empty() {
      self.sort_before_search = Some((self.sort, self.direction));
      self.sort = SortKey::BestMatch;
      self.direction = SortDirection::Descending;
    }

    if search.is_empty() {
      let before = self.sort_before_search.take();

      if self.sort == SortKey::BestMatch {
        let defaults = Self::default();

        (self.sort, self.direction) = before.unwrap_or((defaults.sort, defaults.direction));
      }
    }

    self.search = search;
  }

  /// Adds `category` to the chosen ones, or removes it if already chosen.
  pub fn toggle_category(&mut self, category: String) {
    if !self.categories.remove(&category) {
      self.categories.insert(category);
    }
  }

  /// Sorts by `key`: reversed when it is already the sort, otherwise in the
  /// direction that reads first for it, `A` to `Z` for names and largest or
  /// latest first for the rest. [`SortKey::BestMatch`] is never reversed.
  pub fn sort_by(&mut self, key: SortKey) {
    if self.sort == key && key != SortKey::BestMatch {
      self.direction = match self.direction {
        SortDirection::Ascending => SortDirection::Descending,
        SortDirection::Descending => SortDirection::Ascending,
      };

      return;
    }

    self.sort = key;
    self.direction = match key {
      SortKey::Name => SortDirection::Ascending,
      _ => SortDirection::Descending,
    };
  }

  /// The engine query these filters stand for. Community-pinned packages lead
  /// only the default order, as in r2modman.
  fn query(&self) -> BrowseQuery<'_> {
    let defaults = Self::default();
    let default_order = self.sort == defaults.sort && self.direction == defaults.direction;

    BrowseQuery::new()
      .search(&self.search)
      .sort(self.sort, self.direction)
      .pinned_first(default_order)
      .categories(self.categories.iter().map(String::as_str))
      .show_nsfw(self.show_nsfw)
      .show_deprecated(self.show_deprecated)
  }
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
  },
}

impl Browse {
  /// A list for `community` that is about to read its cache.
  pub fn new(community: String) -> Self {
    Self {
      community,
      index: None,
      rows: Vec::new(),
      listed: HashSet::new(),
      status: Status::ReadingCache,
      filters: Filters::default(),
      categories: Vec::new(),
    }
  }

  /// Whether the list shows the package at `position` in `index`.
  pub fn lists(&self, position: usize) -> bool {
    self.listed.contains(&position)
  }

  /// What the package list area shows.
  pub fn view(&self) -> View<'_> {
    let busy = matches!(self.status, Status::ReadingCache | Status::Refreshing(_));
    let failure = match &self.status {
      Status::Failed(reason) => Some(reason.as_str()),
      _ => None,
    };

    match (&self.index, failure) {
      (Some(_), _) => View::List { refreshing: busy },
      (None, Some(reason)) => View::Unavailable(reason),
      (None, None) => View::Loading,
    }
  }

  /// Shows `index`, listing its packages through the current filters.
  fn show(&mut self, index: Index) {
    let categories: BTreeSet<&str> = (0..index.0.len())
      .filter_map(|idx| PackageRow::new(&index.0, idx))
      .flat_map(|row| row.categories())
      .collect();

    self.categories = categories.into_iter().map(str::to_string).collect();
    self.index = Some(index);
    self.list();
  }

  /// Lists the index's packages again through the current filters, leaving
  /// out [`HIDDEN_PACKAGES`].
  fn list(&mut self) {
    let Some(index) = &self.index else {
      return;
    };

    self.rows = query::browse(&index.0, &self.filters.query());
    self.rows.retain(|&row| {
      index
        .0
        .full_name_at(row)
        .is_none_or(|full_name| !HIDDEN_PACKAGES.contains(&full_name))
    });

    self.listed = self.rows.iter().copied().collect();
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
/// is current, or why the fetch failed. A failure with a list still shown is
/// told in passing, as that list stays usable.
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
    Err(reason) => {
      let stale = browse
        .index
        .is_some()
        .then(|| Effect::Notify(Notice::IndexStale(reason.clone())));

      browse.status = Status::Failed(reason);

      return stale.into_iter().collect();
    }
  }

  Vec::new()
}

/// Changes the open list's filters with `change`, then lists it again.
pub(crate) fn filter(state: &mut State, change: impl FnOnce(&mut Filters)) -> Vec<Effect> {
  if let Some(browse) = state.browse.as_mut() {
    change(&mut browse.filters);
    browse.list();
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
        GameSettings::new(PathBuf::from("/games").join(game)),
      )]
      .into(),
      ..Settings::default()
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

  /// `effects` without the readme fetch of the package a list shows first.
  fn index_effects(effects: Vec<Effect>) -> Vec<Effect> {
    effects
      .into_iter()
      .filter(|effect| !matches!(effect, Effect::Run(Command::FetchReadme(_))))
      .collect()
  }

  #[test]
  fn an_unchanged_index_keeps_the_cached_list() {
    let mut state = opened("valheim");

    update(&mut state, cache_read("valheim", Ok(Some(two_packages()))));

    let effects = update(&mut state, unchanged("valheim", 1));

    assert_eq!(effects, vec![]);
    assert_eq!(listed(&state), ["Owner-Newer", "Owner-Older"]);
    assert_eq!(view(&state), Some(View::List { refreshing: false }));
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

    assert_eq!(index_effects(effects), vec![refresh_if_stale("valheim", 2)]);

    let earlier = update(&mut state, refreshed("valheim", 1, Err("offline")));

    assert_eq!(earlier, vec![]);
    assert_eq!(view(&state), Some(View::List { refreshing: true }));

    update(&mut state, refreshed("valheim", 2, Ok(two_packages())));

    assert_eq!(view(&state), Some(View::List { refreshing: false }));
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
    assert_eq!(view(&state), Some(View::List { refreshing: true }));
    assert_eq!(index_effects(effects), vec![refresh_if_stale("valheim", 1)]);
  }

  #[test]
  fn without_a_cache_loading_shows_until_the_first_fetch_arrives() {
    let mut state = opened("valheim");

    let effects = update(&mut state, cache_read("valheim", Ok(None)));

    assert_eq!(view(&state), Some(View::Loading));
    assert_eq!(effects, vec![refresh("valheim", 1)]);

    let effects = update(&mut state, refreshed("valheim", 1, Ok(two_packages())));

    assert_eq!(listed(&state), ["Owner-Newer", "Owner-Older"]);
    assert_eq!(view(&state), Some(View::List { refreshing: false }));
    assert_eq!(index_effects(effects), vec![]);
  }

  #[test]
  fn a_failed_refresh_keeps_the_cached_list_and_says_why() {
    let mut state = opened("valheim");

    update(&mut state, cache_read("valheim", Ok(Some(two_packages()))));

    let effects = update(&mut state, refreshed("valheim", 1, Err("offline")));

    assert_eq!(listed(&state), ["Owner-Newer", "Owner-Older"]);
    assert_eq!(view(&state), Some(View::List { refreshing: false }));
    assert_eq!(
      effects,
      vec![Effect::Notify(Notice::IndexStale("offline".to_string()))]
    );
  }

  #[test]
  fn a_failed_first_fetch_is_not_announced_as_the_list_says_why() {
    let mut state = opened("valheim");

    update(&mut state, cache_read("valheim", Ok(None)));

    assert_eq!(
      update(&mut state, refreshed("valheim", 1, Err("offline"))),
      vec![]
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
    assert_eq!(view(&state), Some(View::List { refreshing: true }));
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

  /// A state showing `packages` from the cache.
  fn showing(packages: Vec<serde_json::Value>) -> State {
    let mut state = opened("valheim");

    update(&mut state, cache_read("valheim", Ok(Some(index(packages)))));

    state
  }

  fn filters(state: &State) -> &Filters {
    &state.browse.as_ref().unwrap().filters
  }

  #[test]
  fn typing_in_the_search_box_narrows_the_list_by_name_author_and_description() {
    let mut described = package("Owner-Described", 1);

    described["versions"][0]["description"] = "Adds a Fast travel map".into();

    let mut state = showing(vec![
      package("Owner-Older", 2),
      package("Owner-FastBoats", 3),
      package("Fastest-Rafts", 4),
      described,
    ]);

    update(&mut state, Msg::PackageSearchChanged("fast".to_string()));

    assert_eq!(
      listed(&state),
      ["Owner-FastBoats", "Fastest-Rafts", "Owner-Described"]
    );

    update(
      &mut state,
      Msg::PackageSearchChanged("fast rafts".to_string()),
    );

    assert_eq!(listed(&state), ["Fastest-Rafts"]);
    assert_eq!(filters(&state).search, "fast rafts");
  }

  /// Three packages whose name, update day, downloads, and rating each order
  /// them differently.
  fn three_to_sort() -> State {
    let mut bravo = package("Owner-bravo", 1);
    let mut alpha = package("Owner-Alpha", 2);
    let mut charlie = package("Owner-Charlie", 3);

    bravo["versions"][0]["downloads"] = 300.into();
    alpha["versions"][0]["downloads"] = 100.into();
    charlie["versions"][0]["downloads"] = 200.into();
    bravo["rating_score"] = 20.into();
    alpha["rating_score"] = 30.into();
    charlie["rating_score"] = 10.into();

    showing(vec![bravo, alpha, charlie])
  }

  #[test]
  fn each_sort_order_works_in_both_directions() {
    let cases = [
      (
        SortKey::Name,
        ["Owner-Alpha", "Owner-bravo", "Owner-Charlie"],
      ),
      (
        SortKey::Downloads,
        ["Owner-bravo", "Owner-Charlie", "Owner-Alpha"],
      ),
      (
        SortKey::Rating,
        ["Owner-Alpha", "Owner-bravo", "Owner-Charlie"],
      ),
    ];

    for (key, first) in cases {
      let mut state = three_to_sort();
      let mut reversed = first;

      reversed.reverse();

      update(&mut state, Msg::SortPressed(key));

      assert_eq!(listed(&state), first, "{key:?} first press");

      update(&mut state, Msg::SortPressed(key));

      assert_eq!(listed(&state), reversed, "{key:?} second press");
    }
  }

  #[test]
  fn pressing_the_current_sort_reverses_it() {
    let mut state = three_to_sort();

    assert_eq!(
      listed(&state),
      ["Owner-Charlie", "Owner-Alpha", "Owner-bravo"]
    );

    update(&mut state, Msg::SortPressed(SortKey::LastUpdated));

    assert_eq!(
      listed(&state),
      ["Owner-bravo", "Owner-Alpha", "Owner-Charlie"]
    );
    assert_eq!(filters(&state).direction, SortDirection::Ascending);
  }

  #[test]
  fn community_pinned_packages_lead_only_the_default_order() {
    let mut pinned = package("Owner-Pinned", 1);

    pinned["is_pinned"] = true.into();

    let mut state = showing(vec![package("Owner-Alpha", 2), pinned]);

    assert_eq!(listed(&state), ["Owner-Pinned", "Owner-Alpha"]);

    update(&mut state, Msg::SortPressed(SortKey::Name));

    assert_eq!(listed(&state), ["Owner-Alpha", "Owner-Pinned"]);
  }

  /// A package last updated on `updated_day` in `categories`.
  fn categorized(full_name: &str, updated_day: u32, categories: &[&str]) -> serde_json::Value {
    let mut package = package(full_name, updated_day);

    package["categories"] = categories.into();

    package
  }

  #[test]
  fn the_categories_on_offer_are_those_in_the_index() {
    let state = showing(vec![
      categorized("Owner-Map", 1, &["Tools", "Maps"]),
      categorized("Owner-Boat", 2, &["Mods", "Tools"]),
    ]);

    assert_eq!(
      state.browse.as_ref().unwrap().categories,
      ["Maps", "Mods", "Tools"]
    );
  }

  #[test]
  fn choosing_categories_narrows_the_list_to_packages_in_any_of_them() {
    let mut state = showing(vec![
      categorized("Owner-Map", 1, &["Maps"]),
      categorized("Owner-Boat", 2, &["Mods", "Tools"]),
      categorized("Owner-Skin", 3, &["Cosmetics"]),
    ]);

    update(&mut state, Msg::CategoryToggled("Tools".to_string()));

    assert_eq!(listed(&state), ["Owner-Boat"]);

    update(&mut state, Msg::CategoryToggled("Maps".to_string()));

    assert_eq!(listed(&state), ["Owner-Boat", "Owner-Map"]);

    update(&mut state, Msg::CategoryToggled("Tools".to_string()));

    assert_eq!(listed(&state), ["Owner-Map"]);
    assert_eq!(
      filters(&state).categories,
      BTreeSet::from(["Maps".to_string()])
    );
  }

  #[test]
  fn nsfw_and_deprecated_packages_are_hidden_until_each_is_turned_on() {
    let mut deprecated = package("Owner-Deprecated", 2);
    let mut nsfw = package("Owner-Nsfw", 3);

    deprecated["is_deprecated"] = true.into();
    nsfw["has_nsfw_content"] = true.into();

    let mut state = showing(vec![package("Owner-Plain", 1), deprecated, nsfw]);

    assert_eq!(listed(&state), ["Owner-Plain"]);

    update(&mut state, Msg::ShowNsfw(true));

    assert_eq!(listed(&state), ["Owner-Nsfw", "Owner-Plain"]);

    update(&mut state, Msg::ShowDeprecated(true));

    assert_eq!(
      listed(&state),
      ["Owner-Nsfw", "Owner-Deprecated", "Owner-Plain"]
    );

    update(&mut state, Msg::ShowNsfw(false));

    assert_eq!(listed(&state), ["Owner-Deprecated", "Owner-Plain"]);
  }

  #[test]
  fn other_mod_managers_never_appear() {
    let mut state = showing(vec![
      package("Owner-Plain", 1),
      package("ebkr-r2modman", 2),
      package("Kesomannen-GaleModManager", 3),
    ]);

    assert_eq!(listed(&state), ["Owner-Plain"]);

    update(
      &mut state,
      Msg::PackageSearchChanged("r2modman".to_string()),
    );

    assert_eq!(listed(&state), Vec::<String>::new());
  }

  #[test]
  fn clearing_resets_search_and_filters_and_keeps_the_sort() {
    let mut nsfw = categorized("Owner-Nsfw", 2, &["Mods"]);

    nsfw["has_nsfw_content"] = true.into();

    let mut state = showing(vec![categorized("Owner-Plain", 1, &["Tools"]), nsfw]);

    update(&mut state, Msg::PackageSearchChanged("nsfw".to_string()));
    update(&mut state, Msg::CategoryToggled("Mods".to_string()));
    update(&mut state, Msg::ShowNsfw(true));
    update(&mut state, Msg::ShowDeprecated(true));
    update(&mut state, Msg::SortPressed(SortKey::Name));

    assert!(filters(&state).is_filtering());

    update(&mut state, Msg::FiltersCleared);

    assert!(!filters(&state).is_filtering());
    assert_eq!(
      filters(&state),
      &Filters {
        sort: SortKey::Name,
        direction: SortDirection::Ascending,
        ..Filters::default()
      }
    );
    assert_eq!(listed(&state), ["Owner-Plain"]);
  }

  #[test]
  fn typing_into_an_empty_search_sorts_by_best_match() {
    let mut state = three_to_sort();

    update(&mut state, Msg::SortPressed(SortKey::Downloads));
    update(&mut state, Msg::PackageSearchChanged("a".to_string()));

    assert_eq!(filters(&state).sort, SortKey::BestMatch);
    assert_eq!(filters(&state).direction, SortDirection::Descending);
  }

  #[test]
  fn a_sort_picked_while_searching_is_honoured() {
    let mut state = three_to_sort();

    update(&mut state, Msg::PackageSearchChanged("owner".to_string()));
    update(&mut state, Msg::SortPressed(SortKey::Name));

    assert_eq!(
      listed(&state),
      ["Owner-Alpha", "Owner-bravo", "Owner-Charlie"]
    );
    assert_eq!(filters(&state).search, "owner");
  }

  #[test]
  fn deleting_the_search_restores_the_sort_from_before_it() {
    let mut state = three_to_sort();

    update(&mut state, Msg::SortPressed(SortKey::Downloads));
    update(&mut state, Msg::PackageSearchChanged("a".to_string()));
    update(&mut state, Msg::PackageSearchChanged("al".to_string()));
    update(&mut state, Msg::PackageSearchChanged(String::new()));

    assert_eq!(filters(&state).sort, SortKey::Downloads);
    assert_eq!(filters(&state).direction, SortDirection::Descending);
    assert_eq!(
      listed(&state),
      ["Owner-bravo", "Owner-Charlie", "Owner-Alpha"]
    );
  }

  #[test]
  fn clearing_a_best_match_search_restores_the_sort_from_before_it() {
    let mut state = three_to_sort();

    update(&mut state, Msg::SortPressed(SortKey::Rating));
    update(&mut state, Msg::SortPressed(SortKey::Rating));
    update(&mut state, Msg::PackageSearchChanged("al".to_string()));
    update(&mut state, Msg::FiltersCleared);

    assert_eq!(
      filters(&state),
      &Filters {
        sort: SortKey::Rating,
        direction: SortDirection::Ascending,
        ..Filters::default()
      }
    );
  }

  #[test]
  fn a_sort_picked_while_searching_outlasts_deleting_the_search() {
    let mut state = three_to_sort();

    update(&mut state, Msg::PackageSearchChanged("al".to_string()));
    update(&mut state, Msg::SortPressed(SortKey::Downloads));
    update(&mut state, Msg::PackageSearchChanged(String::new()));

    assert_eq!(
      filters(&state),
      &Filters {
        sort: SortKey::Downloads,
        ..Filters::default()
      }
    );
    assert_eq!(
      listed(&state),
      ["Owner-bravo", "Owner-Charlie", "Owner-Alpha"]
    );
  }

  #[test]
  fn best_match_puts_an_exact_name_ahead_of_description_matches() {
    let mut newest = package("Owner-Newest", 9);
    let mut popular = package("Owner-Popular", 5);

    newest["versions"][0]["description"] = "Works with Boats".into();
    popular["versions"][0]["description"] = "Better boats".into();
    popular["versions"][0]["downloads"] = 1_000_000.into();

    let mut state = showing(vec![newest, package("Someone-Boats", 1), popular]);

    update(&mut state, Msg::PackageSearchChanged("boats".to_string()));

    assert_eq!(
      listed(&state),
      ["Someone-Boats", "Owner-Popular", "Owner-Newest"]
    );
  }

  #[test]
  fn pressing_best_match_again_keeps_it_best_first() {
    let mut state = three_to_sort();

    update(&mut state, Msg::PackageSearchChanged("al".to_string()));
    update(&mut state, Msg::SortPressed(SortKey::BestMatch));

    assert_eq!(filters(&state).sort, SortKey::BestMatch);
    assert_eq!(filters(&state).direction, SortDirection::Descending);
  }

  #[test]
  fn the_filters_outlast_a_refresh() {
    let mut state = showing(vec![package("Owner-Alpha", 1)]);

    update(&mut state, Msg::PackageSearchChanged("beta".to_string()));
    update(
      &mut state,
      refreshed(
        "valheim",
        1,
        Ok(index(vec![
          package("Owner-Alpha", 1),
          package("Owner-Beta", 2),
        ])),
      ),
    );

    assert_eq!(listed(&state), ["Owner-Beta"]);
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
