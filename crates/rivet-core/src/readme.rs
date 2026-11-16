//! The readme shown beside the package lists: which package version it is
//! for, and whether it is loading, shown, missing, or failed.

use crate::install::InstalledPackage;
use crate::{Command, Effect, State};
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

/// Shows `package`'s readme, fetching it unless it is already shown or on its
/// way.
pub(crate) fn selected(state: &mut State, package: Package) -> Vec<Effect> {
  let Some(browse) = state.browse.as_ref() else {
    return Vec::new();
  };
  let community = browse.community.clone();
  let shown = state
    .readme
    .as_ref()
    .is_some_and(|readme| readme.package == package && !matches!(readme.status, Status::Failed(_)));

  if shown {
    return Vec::new();
  }

  state.readmes += 1;
  state.readme = Some(Readme {
    id: state.readmes,
    package: package.clone(),
    status: Status::Loading,
  });

  vec![Effect::Run(Command::FetchReadme(Request {
    id: state.readmes,
    community,
    package,
  }))]
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
    let mut state = opened();

    let effects = update(&mut state, Msg::PackageSelected(package("ModA")));

    assert_eq!(
      state.readme,
      Some(Readme {
        id: 1,
        package: package("ModA"),
        status: Status::Loading,
      })
    );
    assert_eq!(
      effects,
      vec![Effect::Run(Command::FetchReadme(Request {
        id: 1,
        community: "valheim".to_string(),
        package: package("ModA"),
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
    let mut state = opened();

    update(&mut state, Msg::PackageSelected(package("ModA")));

    let effects = update(&mut state, fetched(1, Ok(Some("# ModA"))));

    assert_eq!(status(&state), Some(&Status::Loaded("# ModA".to_string())));
    assert_eq!(effects, vec![]);
  }

  #[test]
  fn a_package_without_a_readme_says_so() {
    let mut state = opened();

    update(&mut state, Msg::PackageSelected(package("ModB")));
    update(&mut state, fetched(1, Ok(None)));

    assert_eq!(status(&state), Some(&Status::Missing));
  }

  #[test]
  fn a_failed_fetch_says_why() {
    let mut state = opened();

    update(&mut state, Msg::PackageSelected(package("ModA")));
    update(&mut state, fetched(1, Err("offline")));

    assert_eq!(status(&state), Some(&Status::Failed("offline".to_string())));
  }

  #[test]
  fn a_readme_arriving_after_another_package_was_selected_is_ignored() {
    let mut state = opened();

    update(&mut state, Msg::PackageSelected(package("ModA")));
    update(&mut state, Msg::PackageSelected(package("ModC")));
    update(&mut state, fetched(1, Ok(Some("# ModA"))));

    assert_eq!(
      state.readme,
      Some(Readme {
        id: 2,
        package: package("ModC"),
        status: Status::Loading,
      })
    );

    update(&mut state, fetched(2, Ok(Some("# ModC"))));

    assert_eq!(status(&state), Some(&Status::Loaded("# ModC".to_string())));
  }

  #[test]
  fn a_readme_arriving_after_the_selection_was_cleared_is_ignored() {
    let mut state = opened();

    update(&mut state, Msg::PackageSelected(package("ModA")));
    update(&mut state, Msg::SwitchGamePressed);
    update(&mut state, fetched(1, Ok(Some("# ModA"))));

    assert_eq!(state.readme, None);
  }

  #[test]
  fn selecting_the_shown_package_again_fetches_nothing() {
    let mut state = opened();

    update(&mut state, Msg::PackageSelected(package("ModA")));

    let loading = update(&mut state, Msg::PackageSelected(package("ModA")));

    update(&mut state, fetched(1, Ok(Some("# ModA"))));

    let loaded = update(&mut state, Msg::PackageSelected(package("ModA")));

    assert_eq!(loading, vec![]);
    assert_eq!(loaded, vec![]);
    assert_eq!(status(&state), Some(&Status::Loaded("# ModA".to_string())));
  }

  #[test]
  fn selecting_a_package_whose_fetch_failed_tries_again() {
    let mut state = opened();

    update(&mut state, Msg::PackageSelected(package("ModA")));
    update(&mut state, fetched(1, Err("offline")));

    let effects = update(&mut state, Msg::PackageSelected(package("ModA")));

    assert_eq!(status(&state), Some(&Status::Loading));
    assert_eq!(effects.len(), 1);
  }

  #[test]
  fn choosing_another_tab_clears_the_selection() {
    let mut state = opened();

    update(&mut state, Msg::PackageSelected(package("ModA")));
    update(&mut state, Msg::TabChosen(Tab::Browse));

    assert!(state.readme.is_some());

    update(&mut state, Msg::TabChosen(Tab::Installed));

    assert_eq!(state.readme, None);
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
