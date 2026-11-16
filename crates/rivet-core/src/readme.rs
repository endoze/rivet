//! The readme shown beside the package lists: which package version it is
//! for, and whether it is loading, shown, missing, or failed.

use crate::install::InstalledPackage;
use crate::manage::Tab;
use crate::{Command, Effect, Msg, Screen, State};
use std::time::Duration;
use thunderstore_engine::query::PackageRow;

/// One version of a package, whose readme can be shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
  pub owner: String,
  pub name: String,
  pub version: String,
}

impl Package {
  /// A row of the package list, at the latest version it shows. `None` when
  /// the index lacks the owner, name, or version.
  pub fn listed(row: PackageRow<'_>) -> Option<Self> {
    Some(Self {
      owner: row.owner()?.to_string(),
      name: row.name()?.to_string(),
      version: row.version_number()?.to_string(),
    })
  }

  /// A package of the open profile, by `Owner-Name`, at its installed
  /// version.
  pub fn installed(full_name: &str, package: &InstalledPackage) -> Self {
    Self {
      owner: package.author.clone(),
      name: package.name(full_name).to_string(),
      version: package.version.clone(),
    }
  }

  /// The `Owner-Name` identifier.
  pub fn full_name(&self) -> String {
    format!("{}-{}", self.owner, self.name)
  }
}

/// The readme shown beside the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Readme {
  /// The id of the fetch whose answer is awaited or shown.
  pub id: u64,
  pub package: Package,
  pub status: Status,
  /// Whether its fetch has been sent. It waits while the detail pane is
  /// closed, and while a search is being typed.
  pub requested: bool,
}

/// Where a readme is in being fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
  Loading,
  /// The readme's markdown.
  Loaded(String),
  /// Thunderstore has no readme for the package.
  Missing,
  /// The fetch failed, with the reason.
  Failed(String),
}

/// One fetch of a package's readme. `id` is unique per app run, so an answer
/// can be matched to the fetch that is still awaited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
  pub id: u64,
  /// The Thunderstore community the package is listed in.
  pub community: String,
  pub package: Package,
}

/// How long typing in a search must pause before the readme of the package
/// it brings up is fetched.
pub const SEARCH_PAUSE: Duration = Duration::from_millis(300);

/// When the readme of a package the detail pane comes to show is fetched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fetch {
  /// Straight away.
  Now,
  /// Once typing in a search pauses for [`SEARCH_PAUSE`].
  AfterPause,
}

/// Shows `package`'s readme, fetching it unless it is already shown or on its
/// way, or the detail pane is closed.
pub(crate) fn selected(state: &mut State, package: Package) -> Vec<Effect> {
  if state.browse.is_none() {
    return Vec::new();
  }

  let shown = state
    .readme
    .as_ref()
    .is_some_and(|readme| readme.package == package && !matches!(readme.status, Status::Failed(_)));

  if !shown {
    replace(state, package);
  }

  request(state, Fetch::Now)
}

/// The messages after which the shown tab may list other packages, or the
/// detail pane opened, with when a readme they bring up is fetched. Others
/// leave the shown package as it is.
pub(crate) fn follows(msg: &Msg) -> Option<Fetch> {
  match msg {
    Msg::PackageSearchChanged(_) | Msg::InstalledSearchChanged(_) => Some(Fetch::AfterPause),
    Msg::CachedIndexRead { .. }
    | Msg::IndexRefreshed { .. }
    | Msg::SortPressed(_)
    | Msg::CategoryToggled(_)
    | Msg::ShowNsfw(_)
    | Msg::ShowDeprecated(_)
    | Msg::FiltersCleared
    | Msg::TabChosen(_)
    | Msg::InstalledSortChosen(_)
    | Msg::ProfileOpened { .. }
    | Msg::ProfileChanged { .. }
    | Msg::OperationFinished { .. }
    | Msg::SettingsClosed
    | Msg::ShowDetails(true) => Some(Fetch::Now),
    _ => None,
  }
}

/// Keeps the readme on a package the shown tab lists: the one shown, at the
/// version the tab lists it at, or else the list's first, or none when the
/// list is empty. A readme is fetched only when the package shown changes,
/// so typing a search that keeps it fetches nothing.
pub(crate) fn follow_list(state: &mut State, when: Fetch) -> Vec<Effect> {
  if !matches!(state.screen, Screen::Game(_)) {
    return Vec::new();
  }

  let listed = state
    .readme
    .as_ref()
    .and_then(|readme| state.listed(&readme.package.full_name()))
    .or_else(|| state.first_listed());
  let Some(package) = listed else {
    state.readme = None;

    return Vec::new();
  };
  let held = state
    .readme
    .as_ref()
    .is_some_and(|readme| readme.package == package);

  if !held {
    replace(state, package);
  }

  request(state, when)
}

/// Fetches the shown readme once typing has paused, if the pause that ended
/// was the last one asked for it.
pub(crate) fn due(state: &mut State, id: u64) -> Vec<Effect> {
  if state.readme.as_ref().is_none_or(|readme| readme.id != id) {
    return Vec::new();
  }

  request(state, Fetch::Now)
}

/// Shows `package`'s readme loading, its fetch not yet sent.
fn replace(state: &mut State, package: Package) {
  state.readmes += 1;
  state.readme = Some(Readme {
    id: state.readmes,
    package,
    status: Status::Loading,
    requested: false,
  });
}

/// Sends the shown readme's fetch now or after a pause, as `when` says,
/// unless it is sent or the detail pane is closed. Waiting for a pause
/// renumbers the readme, so only the last pause asked for sends it.
fn request(state: &mut State, when: Fetch) -> Vec<Effect> {
  let Some(community) = state.browse.as_ref().map(|browse| browse.community.clone()) else {
    return Vec::new();
  };
  let Some(readme) = state
    .readme
    .as_mut()
    .filter(|readme| !readme.requested && state.details_shown)
  else {
    return Vec::new();
  };

  match when {
    Fetch::Now => {
      readme.requested = true;

      vec![Effect::Run(Command::FetchReadme(Request {
        id: readme.id,
        community,
        package: readme.package.clone(),
      }))]
    }
    Fetch::AfterPause => {
      state.readmes += 1;
      readme.id = state.readmes;

      vec![Effect::After {
        delay: SEARCH_PAUSE,
        msg: Box::new(Msg::ReadmeDue(readme.id)),
      }]
    }
  }
}

impl State {
  /// `full_name` (`Owner-Name`) at the version the shown tab lists it, or
  /// `None` when the tab leaves it out.
  fn listed(&self, full_name: &str) -> Option<Package> {
    match self.tab {
      Tab::Installed => self
        .installed_listing(full_name)
        .map(|package| Package::installed(full_name, package)),
      Tab::Browse => {
        let browse = self.browse.as_ref()?;
        let row = browse.index.as_ref()?.package(full_name)?;

        if !browse.lists(row.index()) {
          return None;
        }

        Package::listed(row)
      }
    }
  }

  /// The first package of the shown tab's list, `None` while it is empty.
  fn first_listed(&self) -> Option<Package> {
    match self.tab {
      Tab::Installed => self
        .installed_rows()
        .first()
        .map(|(full_name, package)| Package::installed(full_name, package)),
      Tab::Browse => {
        let browse = self.browse.as_ref()?;
        let index = browse.index.as_ref()?;

        PackageRow::new(&index.0, *browse.rows.first()?).and_then(Package::listed)
      }
    }
  }
}

/// Opens `url` in the browser when it is a web address. Anything else, such
/// as a `file:` or `steam:` link in a readme, is not opened.
pub(crate) fn link_clicked(url: String) -> Vec<Effect> {
  let web = url.split_once("://").is_some_and(|(scheme, _)| {
    scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
  });

  if !web {
    return Vec::new();
  }

  vec![Effect::OpenUrl(url)]
}

/// Applies the answer to the fetch with id `request`, unless the user has
/// since selected another package or none.
pub(crate) fn fetched(
  state: &mut State,
  request: u64,
  readme: Result<Option<String>, String>,
) -> Vec<Effect> {
  if let Some(shown) = state.readme.as_mut().filter(|shown| shown.id == request) {
    shown.status = match readme {
      Ok(Some(markdown)) => Status::Loaded(markdown),
      Ok(None) => Status::Missing,
      Err(reason) => Status::Failed(reason),
    };
  }

  Vec::new()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::manage::Tab;
  use crate::settings::{GameSettings, Settings};
  use crate::{Command, Effect, Msg, State, test_ecosystem, update};

  /// A state with valheim open, its package list not yet read.
  fn opened() -> State {
    let mut state = State::new(test_ecosystem());
    let settings = Settings {
      last_game: Some("valheim".to_string()),
      games: [("valheim".to_string(), GameSettings::new("/games/valheim"))].into(),
      ..Settings::default()
    };

    update(&mut state, Msg::Started);
    update(&mut state, Msg::SettingsLoaded(Ok(settings)));

    state
  }

  fn package(name: &str) -> Package {
    Package {
      owner: "Owner".to_string(),
      name: name.to_string(),
      version: "1.0.0".to_string(),
    }
  }

  #[test]
  fn selecting_a_package_shows_it_loading_and_fetches_its_readme() {
    let (mut state, _) = listing();

    let effects = update(&mut state, Msg::PackageSelected(package("ModB")));

    assert_eq!(
      state.readme,
      Some(Readme {
        id: 2,
        package: package("ModB"),
        status: Status::Loading,
        requested: true,
      })
    );
    assert_eq!(
      effects,
      vec![Effect::Run(Command::FetchReadme(Request {
        id: 2,
        community: "valheim".to_string(),
        package: package("ModB"),
      }))]
    );
  }

  fn fetched(request: u64, readme: Result<Option<&str>, &str>) -> Msg {
    Msg::ReadmeFetched {
      request,
      readme: readme
        .map(|markdown| markdown.map(str::to_string))
        .map_err(str::to_string),
    }
  }

  fn status(state: &State) -> Option<&Status> {
    state.readme.as_ref().map(|readme| &readme.status)
  }

  #[test]
  fn an_arriving_readme_is_shown() {
    let (mut state, _) = listing();

    let effects = update(&mut state, fetched(1, Ok(Some("# ModA"))));

    assert_eq!(status(&state), Some(&Status::Loaded("# ModA".to_string())));
    assert_eq!(effects, vec![]);
  }

  #[test]
  fn a_package_without_a_readme_says_so() {
    let (mut state, _) = listing();

    update(&mut state, Msg::PackageSelected(package("ModB")));
    update(&mut state, fetched(2, Ok(None)));

    assert_eq!(status(&state), Some(&Status::Missing));
  }

  #[test]
  fn a_failed_fetch_says_why() {
    let (mut state, _) = listing();

    update(&mut state, fetched(1, Err("offline")));

    assert_eq!(status(&state), Some(&Status::Failed("offline".to_string())));
  }

  #[test]
  fn a_readme_arriving_after_another_package_was_selected_is_ignored() {
    let (mut state, _) = listing();

    update(&mut state, Msg::PackageSelected(package("ModB")));
    update(&mut state, fetched(1, Ok(Some("# ModA"))));

    assert_eq!(
      state.readme,
      Some(Readme {
        id: 2,
        package: package("ModB"),
        status: Status::Loading,
        requested: true,
      })
    );

    update(&mut state, fetched(2, Ok(Some("# ModB"))));

    assert_eq!(status(&state), Some(&Status::Loaded("# ModB".to_string())));
  }

  #[test]
  fn a_readme_arriving_after_the_selection_was_cleared_is_ignored() {
    let (mut state, _) = listing();

    update(&mut state, Msg::SwitchGamePressed);
    update(&mut state, fetched(1, Ok(Some("# ModA"))));

    assert_eq!(state.readme, None);
  }

  #[test]
  fn selecting_the_shown_package_again_fetches_nothing() {
    let (mut state, _) = listing();

    let loading = update(&mut state, Msg::PackageSelected(package("ModA")));

    update(&mut state, fetched(1, Ok(Some("# ModA"))));

    let loaded = update(&mut state, Msg::PackageSelected(package("ModA")));

    assert_eq!(loading, vec![]);
    assert_eq!(loaded, vec![]);
    assert_eq!(status(&state), Some(&Status::Loaded("# ModA".to_string())));
  }

  #[test]
  fn selecting_a_package_whose_fetch_failed_tries_again() {
    let (mut state, _) = listing();

    update(&mut state, fetched(1, Err("offline")));

    let effects = update(&mut state, Msg::PackageSelected(package("ModA")));

    assert_eq!(status(&state), Some(&Status::Loading));
    assert_eq!(fetches(&effects), ["Owner-ModA"]);
  }

  #[test]
  fn a_failed_fetch_is_not_tried_again_until_the_package_is_selected() {
    let (mut state, _) = listing();

    update(&mut state, fetched(1, Err("offline")));

    let effects = update(&mut state, Msg::PackageSearchChanged("Mod".to_string()));

    assert_eq!(fetches(&effects), Vec::<String>::new());
    assert_eq!(status(&state), Some(&Status::Failed("offline".to_string())));
  }

  #[test]
  fn choosing_another_tab_shows_the_first_package_of_its_list() {
    let (mut state, _) = listing();

    update(
      &mut state,
      Msg::ProfileOpened {
        game: "valheim".to_string(),
        profile: crate::install::DEFAULT_PROFILE.to_string(),
        installed: Ok(crate::test_installed(&[
          ("Owner-Zed", "1.0.0"),
          ("Owner-Abe", "1.0.0"),
        ])),
      },
    );
    update(&mut state, Msg::PackageSelected(package("ModB")));
    update(&mut state, Msg::TabChosen(Tab::Browse));

    assert_eq!(shown(&state), Some("Owner-ModB".to_string()));

    let effects = update(&mut state, Msg::TabChosen(Tab::Installed));

    assert_eq!(shown(&state), Some("Owner-Abe".to_string()));
    assert_eq!(fetches(&effects), ["Owner-Abe"]);
  }

  /// A one-version package last updated on `updated_day` of January 2024.
  fn listed(full_name: &str, updated_day: u32) -> serde_json::Value {
    let mut listed = crate::test_listed(full_name, &["1.0.0"], false);

    listed["date_updated"] = format!("2024-01-{updated_day:02}T00:00:00Z").into();

    listed
  }

  /// [`opened`] with its package list read from the cache: `Owner-ModA`,
  /// then `Owner-ModB`, then `Owner-Other`, most recently updated first.
  fn listing() -> (State, Vec<Effect>) {
    let mut state = opened();
    let index = crate::test_index(vec![
      listed("Owner-Other", 1),
      listed("Owner-ModB", 2),
      listed("Owner-ModA", 3),
    ]);
    let effects = update(
      &mut state,
      Msg::CachedIndexRead {
        community: "valheim".to_string(),
        index: Ok(Some(index)),
      },
    );

    (state, effects)
  }

  /// The packages whose readmes `effects` fetch, by `Owner-Name`.
  fn fetches(effects: &[Effect]) -> Vec<String> {
    effects
      .iter()
      .filter_map(|effect| match effect {
        Effect::Run(Command::FetchReadme(request)) => Some(request.package.full_name()),
        _ => None,
      })
      .collect()
  }

  fn shown(state: &State) -> Option<String> {
    state
      .readme
      .as_ref()
      .map(|readme| readme.package.full_name())
  }

  #[test]
  fn with_nothing_selected_the_first_listed_package_is_shown() {
    let (state, effects) = listing();

    assert_eq!(shown(&state), Some("Owner-ModA".to_string()));
    assert_eq!(fetches(&effects), ["Owner-ModA"]);
  }

  fn search(text: &str) -> Msg {
    Msg::PackageSearchChanged(text.to_string())
  }

  #[test]
  fn typing_a_search_that_keeps_the_shown_package_fetches_nothing() {
    let (mut state, _) = listing();

    update(&mut state, Msg::PackageSelected(package("ModB")));

    let effects: Vec<Effect> = ["M", "Mo", "Mod"]
      .into_iter()
      .flat_map(|text| update(&mut state, search(text)))
      .collect();

    assert_eq!(shown(&state), Some("Owner-ModB".to_string()));
    assert_eq!(fetches(&effects), Vec::<String>::new());
  }

  /// The messages `effects` ask to be sent back later.
  fn due(effects: &[Effect]) -> Vec<Msg> {
    effects
      .iter()
      .filter_map(|effect| match effect {
        Effect::After { msg, .. } => Some(msg.as_ref().clone()),
        _ => None,
      })
      .collect()
  }

  #[test]
  fn a_search_leaving_out_the_shown_package_shows_its_first_match_at_once() {
    let (mut state, _) = listing();

    let effects = update(&mut state, search("Other"));

    assert_eq!(shown(&state), Some("Owner-Other".to_string()));
    assert_eq!(status(&state), Some(&Status::Loading));
    assert_eq!(fetches(&effects), Vec::<String>::new());
    assert_eq!(due(&effects).len(), 1);
  }

  #[test]
  fn a_burst_of_search_keystrokes_fetches_one_readme_once_typing_pauses() {
    let (mut state, _) = listing();

    let typed: Vec<Effect> = ["ModB", "Mod", "Other"]
      .into_iter()
      .flat_map(|text| update(&mut state, search(text)))
      .collect();

    assert_eq!(fetches(&typed), Vec::<String>::new());

    let paused: Vec<Effect> = due(&typed)
      .into_iter()
      .flat_map(|msg| update(&mut state, msg))
      .collect();

    assert_eq!(fetches(&paused), ["Owner-Other"]);
  }

  #[test]
  fn nothing_is_fetched_while_the_pane_is_closed() {
    let (mut state, _) = listing();

    update(&mut state, Msg::ShowDetails(false));

    let closed: Vec<Effect> = [
      Msg::PackageSelected(package("ModB")),
      search("Other"),
      Msg::TabChosen(Tab::Installed),
      Msg::TabChosen(Tab::Browse),
    ]
    .into_iter()
    .flat_map(|msg| update(&mut state, msg))
    .collect();

    assert_eq!(fetches(&closed), Vec::<String>::new());
    assert_eq!(due(&closed), Vec::<Msg>::new());

    let opened = update(&mut state, Msg::ShowDetails(true));

    assert_eq!(fetches(&opened), ["Owner-Other"]);
  }

  #[test]
  fn an_unrelated_message_leaves_the_shown_package_alone() {
    let (mut state, _) = listing();
    let unlisted = Readme {
      id: 1,
      package: package("Gone"),
      status: Status::Loading,
      requested: true,
    };

    state.readme = Some(unlisted.clone());

    let effects = update(
      &mut state,
      Msg::OperationProgress {
        operation: 99,
        progress: crate::install::Progress {
          package: "Owner-ModA".to_string(),
          stage: crate::install::Stage::Downloading,
          done: 1,
          total: Some(2),
        },
      },
    );

    assert_eq!(state.readme, Some(unlisted));
    assert_eq!(fetches(&effects), Vec::<String>::new());
  }

  #[test]
  fn the_pane_reads_the_version_the_shown_tab_lists() {
    let mut state = opened();

    update(
      &mut state,
      Msg::CachedIndexRead {
        community: "valheim".to_string(),
        index: Ok(Some(crate::test_index(vec![crate::test_listed(
          "Owner-ModA",
          &["1.0.0", "2.0.0"],
          false,
        )]))),
      },
    );
    update(
      &mut state,
      Msg::ProfileOpened {
        game: "valheim".to_string(),
        profile: crate::install::DEFAULT_PROFILE.to_string(),
        installed: Ok(crate::test_installed(&[("Owner-ModA", "1.0.0")])),
      },
    );

    let version = |state: &State| {
      state
        .readme
        .as_ref()
        .map(|readme| readme.package.version.clone())
    };

    assert_eq!(version(&state), Some("2.0.0".to_string()));

    let effects = update(&mut state, Msg::TabChosen(Tab::Installed));

    assert_eq!(version(&state), Some("1.0.0".to_string()));
    assert_eq!(fetches(&effects), ["Owner-ModA"]);
  }

  #[test]
  fn a_search_matching_nothing_shows_no_package() {
    let (mut state, _) = listing();

    let effects = update(&mut state, search("nothing like it"));

    assert_eq!(state.readme, None);
    assert_eq!(fetches(&effects), Vec::<String>::new());
  }

  #[test]
  fn a_web_link_opens_in_the_browser() {
    let mut state = opened();

    for url in ["https://thunderstore.io/c/valheim/", "HTTP://example.com"] {
      let effects = update(&mut state, Msg::LinkClicked(url.to_string()));

      assert_eq!(effects, vec![Effect::OpenUrl(url.to_string())]);
    }
  }

  #[test]
  fn a_link_that_is_not_a_web_address_opens_nothing() {
    let mut state = opened();

    for url in [
      "file:///etc/passwd",
      "steam://run/892970",
      "javascript:alert(1)",
      "docs/CONFIG.md",
      "#installation",
    ] {
      assert_eq!(
        update(&mut state, Msg::LinkClicked(url.to_string())),
        vec![]
      );
    }
  }

  #[test]
  fn a_listed_package_is_read_at_its_latest_version() {
    let packages: Vec<thunderstore_engine::models::Package> =
      serde_json::from_value(serde_json::json!([{
        "name": "ModD",
        "full_name": "Owner-ModD",
        "owner": "Owner",
        "date_created": "2024-01-01T00:00:00Z",
        "date_updated": "2024-02-01T00:00:00Z",
        "categories": [],
        "versions": [
          { "version_number": "2.0.0", "dependencies": [], "date_created": "2024-02-01T00:00:00Z" },
          { "version_number": "1.0.0", "dependencies": [], "date_created": "2024-01-01T00:00:00Z" }
        ]
      }]))
      .unwrap();
    let index = thunderstore_engine::models::PackageIndex::from(packages);
    let row = PackageRow::new(&index, 0).unwrap();

    assert_eq!(
      Package::listed(row),
      Some(Package {
        owner: "Owner".to_string(),
        name: "ModD".to_string(),
        version: "2.0.0".to_string(),
      })
    );
  }

  #[test]
  fn an_installed_package_is_read_at_its_installed_version() {
    let installed = crate::test_installed(&[("Owner-ModD", "1.0.0")]);
    let (full_name, package) = installed.first_key_value().unwrap();

    assert_eq!(
      Package::installed(full_name, package),
      Package {
        owner: "Owner".to_string(),
        name: "ModD".to_string(),
        version: "1.0.0".to_string(),
      }
    );
  }

  #[test]
  fn a_package_is_named_by_owner_and_name() {
    assert_eq!(package("ModA").full_name(), "Owner-ModA");
  }
}
