//! The detail pane beside the list: whether it is shown, and what it says
//! about the package it shows.

use crate::install::Profile;
use crate::{Effect, Msg, State};

/// What the detail pane says about the package it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Details {
  /// The `Owner-Name` identifier.
  pub full_name: String,
  pub name: String,
  pub owner: String,
  /// The open profile's version of it, or else Thunderstore's latest.
  pub version: String,
  /// What Thunderstore lists about it. `None` when the index lacks it.
  pub listing: Option<Listing>,
  /// What can be done to it, in the order they are offered.
  pub actions: Vec<Action>,
}

/// What Thunderstore's index says about a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listing {
  /// Summed over every version.
  pub downloads: u64,
  /// When it was last updated, in seconds since the Unix epoch.
  pub updated: i64,
  pub categories: Vec<String>,
  /// The URL of its latest version's icon.
  pub icon: Option<String>,
  pub deprecated: bool,
}

/// Something the detail pane offers to do to its package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
  /// Add it to the open profile.
  Install,
  /// Update it to this version.
  Update(String),
  Unpin,
  /// Install a version picked from those it has published.
  ChooseVersion,
  Uninstall,
}

impl Action {
  /// The message pressing it sends for `full_name` (`Owner-Name`).
  pub fn msg(&self, full_name: &str) -> Msg {
    let package = full_name.to_string();

    match self {
      Self::Install | Self::ChooseVersion => Msg::InstallPressed(package),
      Self::Update(_) => Msg::UpdatePressed(package),
      Self::Unpin => Msg::UnpinPressed(package),
      Self::Uninstall => Msg::UninstallPressed(package),
    }
  }
}

impl State {
  /// What the detail pane says about the package it shows, or `None` when
  /// it shows none.
  pub fn details(&self) -> Option<Details> {
    let full_name = self.readme.as_ref()?.package.full_name();
    let row = self
      .browse
      .as_ref()
      .and_then(|browse| browse.index.as_ref())
      .and_then(|index| index.package(&full_name));
    let listing = row.map(|row| Listing {
      downloads: row.downloads(),
      updated: row.date_updated().unix_timestamp(),
      categories: row.categories().map(str::to_string).collect(),
      icon: row.icon().map(str::to_string),
      deprecated: row.is_deprecated(),
    });
    let installed = self
      .profile
      .as_ref()
      .and_then(Profile::installed)
      .and_then(|installed| installed.get(&full_name));
    let (name, owner, version, actions) = match installed {
      Some(package) => (
        package.name(&full_name).to_string(),
        package.author.clone(),
        package.version.clone(),
        self.profile_actions(&full_name, row.is_some()),
      ),
      None => {
        let row = row?;

        (
          row.name()?.to_string(),
          row.owner()?.to_string(),
          row.version_number()?.to_string(),
          vec![Action::Install],
        )
      }
    };

    Some(Details {
      full_name,
      name,
      owner,
      version,
      listing,
      actions,
    })
  }

  /// Whether `action` can be taken now. Installing and choosing a version
  /// queue behind a running change to the profile, so they wait only for the
  /// profile to take installs. The rest are planned from the packages shown,
  /// which a running change would make stale, so they wait for it to end.
  /// In a game whose mods Rivet cannot [install](State::installs_mods), only
  /// unpinning and uninstalling can be taken.
  pub fn can_take(&self, action: &Action) -> bool {
    match action {
      Action::Install | Action::ChooseVersion => self.can_add_mods(),
      Action::Update(_) => self.can_update(),
      Action::Unpin | Action::Uninstall => self.can_manage(),
    }
  }

  /// What can be done to `full_name` in the open profile: unpinning it when
  /// pinned, or else updating it when Thunderstore has a newer version, then
  /// choosing its version when Thunderstore `lists` it, and uninstalling it.
  fn profile_actions(&self, full_name: &str, lists: bool) -> Vec<Action> {
    let badges = self.badges().remove(full_name).unwrap_or_default();
    let main = match (badges.pinned, badges.update) {
      (true, _) => Some(Action::Unpin),
      (false, Some(version)) => Some(Action::Update(version)),
      (false, None) => None,
    };

    main
      .into_iter()
      .chain(lists.then_some(Action::ChooseVersion))
      .chain([Action::Uninstall])
      .collect()
  }
}

/// Shows the detail pane, or hides it.
pub(crate) fn show(state: &mut State, shown: bool) -> Vec<Effect> {
  state.details_shown = shown;

  Vec::new()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::install::{DEFAULT_PROFILE, Installed};
  use crate::readme::Package;
  use crate::settings::{GameSettings, Settings};
  use crate::{Msg, State, test_ecosystem, test_installed, update};

  /// 1 March 2024, when `Owner-Listed` was last updated.
  const UPDATED: i64 = 1_709_251_200;

  /// Valheim open on an index of `Owner-Listed`, at 1.0.0 then 2.0.0 with
  /// 1,000 and 234 downloads, in Tools and Maps, and of `Owner-Pinned` at
  /// 1.0.0 then 2.0.0. Its profile holds `installed`.
  fn opened(installed: Installed) -> State {
    opened_in("valheim", installed)
  }

  /// As [`opened`], with `game` open instead.
  fn opened_in(game: &str, installed: Installed) -> State {
    let mut state = State::new(test_ecosystem());
    let settings = Settings {
      last_game: Some(game.to_string()),
      games: [(
        game.to_string(),
        GameSettings::new(format!("/games/{game}")),
      )]
      .into(),
      ..Settings::default()
    };
    let mut listed = crate::test_listed("Owner-Listed", &["1.0.0", "2.0.0"], false);

    listed["date_updated"] = "2024-03-01T00:00:00Z".into();
    listed["categories"] = serde_json::json!(["Tools", "Maps"]);
    listed["versions"][0]["downloads"] = 1_000.into();
    listed["versions"][1]["downloads"] = 234.into();
    listed["versions"][1]["icon"] = "https://example.com/icon.png".into();

    let index = crate::test_index(vec![
      listed,
      crate::test_listed("Owner-Pinned", &["1.0.0", "2.0.0"], false),
    ]);

    for msg in [
      Msg::Started,
      Msg::SettingsLoaded(Ok(settings)),
      Msg::CachedIndexRead {
        community: game.to_string(),
        index: Ok(Some(index)),
      },
      Msg::ProfileOpened {
        game: game.to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        installed: Ok(installed),
      },
    ] {
      update(&mut state, msg);
    }

    state
  }

  fn select(state: &mut State, full_name: &str, version: &str) {
    let (owner, name) = full_name.split_once('-').unwrap();

    update(
      state,
      Msg::PackageSelected(Package {
        owner: owner.to_string(),
        name: name.to_string(),
        version: version.to_string(),
      }),
    );
  }

  #[test]
  fn a_game_whose_mods_cannot_be_installed_only_lets_its_mods_be_unpinned_or_uninstalled() {
    let state = opened_in("balatro", test_installed(&[("Owner-Listed", "1.0.0")]));

    assert!(!state.can_take(&Action::Install));
    assert!(!state.can_take(&Action::ChooseVersion));
    assert!(!state.can_take(&Action::Update("2.0.0".to_string())));
    assert!(state.can_take(&Action::Unpin));
    assert!(state.can_take(&Action::Uninstall));
  }

  #[test]
  fn a_thunderstore_package_shows_its_latest_version_and_can_only_be_installed() {
    let mut state = opened(Installed::new());

    select(&mut state, "Owner-Listed", "2.0.0");

    assert_eq!(
      state.details(),
      Some(Details {
        full_name: "Owner-Listed".to_string(),
        name: "Listed".to_string(),
        owner: "Owner".to_string(),
        version: "2.0.0".to_string(),
        listing: Some(Listing {
          downloads: 1_234,
          updated: UPDATED,
          categories: vec!["Tools".to_string(), "Maps".to_string()],
          icon: Some("https://example.com/icon.png".to_string()),
          deprecated: false,
        }),
        actions: vec![Action::Install],
      })
    );
  }

  #[test]
  fn the_pane_is_shown_until_closed_and_comes_back_when_opened() {
    let mut state = State::new(test_ecosystem());

    assert!(state.details_shown);

    update(&mut state, Msg::ShowDetails(false));

    assert!(!state.details_shown);

    update(&mut state, Msg::ShowDetails(true));

    assert!(state.details_shown);
  }

  #[test]
  fn a_profile_package_shows_its_version_and_offers_its_update() {
    let mut state = opened(test_installed(&[("Owner-Listed", "1.0.0")]));

    select(&mut state, "Owner-Listed", "1.0.0");

    let details = state.details().unwrap();

    assert_eq!(details.version, "1.0.0");
    assert_eq!(
      details.listing.map(|listing| listing.downloads),
      Some(1_234)
    );
    assert_eq!(
      details.actions,
      [
        Action::Update("2.0.0".to_string()),
        Action::ChooseVersion,
        Action::Uninstall,
      ]
    );
  }

  #[test]
  fn a_pinned_package_offers_unpinning_instead_of_its_update() {
    let mut installed = test_installed(&[("Owner-Pinned", "1.0.0")]);

    installed.get_mut("Owner-Pinned").unwrap().pinned = true;

    let mut state = opened(installed);

    select(&mut state, "Owner-Pinned", "1.0.0");

    assert_eq!(
      state.details().unwrap().actions,
      [Action::Unpin, Action::ChooseVersion, Action::Uninstall]
    );
  }

  #[test]
  fn a_profile_package_thunderstore_lacks_shows_what_the_profile_knows_and_can_be_uninstalled() {
    let mut state = opened(test_installed(&[("Gone-Away", "3.1.0")]));

    update(&mut state, Msg::TabChosen(crate::manage::Tab::Installed));

    assert_eq!(
      state.details(),
      Some(Details {
        full_name: "Gone-Away".to_string(),
        name: "Away".to_string(),
        owner: "Gone".to_string(),
        version: "3.1.0".to_string(),
        listing: None,
        actions: vec![Action::Uninstall],
      })
    );
  }

  #[test]
  fn a_version_can_be_chosen_while_a_change_to_the_profile_runs() {
    let mut state = opened(test_installed(&[
      ("Owner-Listed", "1.0.0"),
      ("Owner-Pinned", "1.0.0"),
    ]));

    update(&mut state, Msg::DisablePressed("Owner-Pinned".to_string()));

    assert!(state.can_take(&Action::ChooseVersion));
    assert!(state.can_take(&Action::Install));
    assert!(!state.can_take(&Action::Uninstall));
    assert!(!state.can_take(&Action::Unpin));
    assert!(!state.can_take(&Action::Update("2.0.0".to_string())));
  }

  #[test]
  fn each_action_sends_what_its_button_did_before() {
    let package = || "Owner-Listed".to_string();

    assert_eq!(
      [
        Action::Install,
        Action::ChooseVersion,
        Action::Update("2.0.0".to_string()),
        Action::Unpin,
        Action::Uninstall,
      ]
      .map(|action| action.msg("Owner-Listed")),
      [
        Msg::InstallPressed(package()),
        Msg::InstallPressed(package()),
        Msg::UpdatePressed(package()),
        Msg::UnpinPressed(package()),
        Msg::UninstallPressed(package()),
      ]
    );
  }
}
