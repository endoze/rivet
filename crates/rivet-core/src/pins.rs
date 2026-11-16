//! Pins: the versions the user chose for a profile's packages, which updates
//! leave alone until they are unpinned.
//!
//! A profile's pins are kept in a file of the app's own inside the profile,
//! never in the installed-packages file, so r2modman can still read the
//! profile. They do not travel in exports.

use crate::browse::Index;
use crate::install::{Installed, ProfileStatus};
use crate::notice::Notice;
use crate::{Command, Effect, State};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One profile's pin file: the version each pinned package is held at, by
/// `Owner-Name`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pins {
  pub pins: BTreeMap<String, String>,
}

impl Pins {
  /// Marks the packages of `installed` pinned that are at the version their
  /// pin holds, and every other one unpinned. A pin at another version is
  /// stale: something other than the user's choice moved the package.
  pub fn mark(&self, installed: &mut Installed) {
    for (full_name, package) in installed.iter_mut() {
      package.pinned = self.pins.get(full_name) == Some(&package.version);
    }
  }

  /// Applies an install of `package` that went through, leaving the profile
  /// holding `after`: a `chosen` version the package is now at pins it there,
  /// and installing the latest version (`None`) unpins it. Then drops stale
  /// pins, as [`Pins::keep_installed`] does.
  pub fn installed(&mut self, package: &str, chosen: Option<&str>, after: &Installed) {
    match chosen {
      Some(version) => {
        if after
          .get(package)
          .is_some_and(|installed| installed.version == version)
        {
          self.pins.insert(package.to_string(), version.to_string());
        }
      }
      None => {
        self.pins.remove(package);
      }
    }

    self.keep_installed(after);
  }

  /// Drops the pins of packages `installed` lacks or holds at another
  /// version, so a package installed again later is not pinned by a choice
  /// made before.
  pub fn keep_installed(&mut self, installed: &Installed) {
    self.pins.retain(|full_name, version| {
      installed
        .get(full_name)
        .is_some_and(|package| package.version == *version)
    });
  }
}

/// The pinned packages of `installed` an install or update must hold where
/// they are, as `Owner-Name-Version`, so one that needs a pinned package does
/// not move it. The packages the change is for, by `Owner-Name`, are left
/// out, as is any package held at a version `index` no longer publishes,
/// which could not be installed.
pub fn held(installed: &Installed, index: &Index, changing: &[String]) -> Vec<String> {
  installed
    .iter()
    .filter(|(full_name, package)| package.pinned && !changing.contains(full_name))
    .filter(|(full_name, package)| index.versions(full_name).contains(&package.version))
    .map(|(full_name, package)| format!("{full_name}-{}", package.version))
    .collect()
}

/// Unpin `package` (`Owner-Name`) in a game's profile, so updates move it
/// again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unpin {
  /// The game, by its key in the game definitions.
  pub game: String,
  pub profile: String,
  pub package: String,
}

/// Unpins `full_name` in the open profile, when it is pinned and a change can
/// start.
pub(crate) fn unpin_pressed(state: &mut State, full_name: String) -> Vec<Effect> {
  if !state.can_manage() {
    return Vec::new();
  }

  let Some(profile) = state.profile.as_ref() else {
    return Vec::new();
  };
  let pinned = profile
    .installed()
    .and_then(|installed| installed.get(&full_name))
    .is_some_and(|package| package.pinned);

  if !pinned {
    return Vec::new();
  }

  vec![Effect::Run(Command::Unpin(Unpin {
    game: profile.game.clone(),
    profile: profile.name.clone(),
    package: full_name,
  }))]
}

/// Applies the answer to `unpin`: the package loses its mark when its profile
/// is open, or the user is told why it could not be unpinned.
pub(crate) fn unpinned(state: &mut State, unpin: Unpin, result: Result<(), String>) -> Vec<Effect> {
  if let Err(reason) = result {
    return vec![Effect::Notify(Notice::UnpinFailed {
      package: unpin.package,
      reason,
    })];
  }

  let package = state
    .profile
    .as_mut()
    .filter(|open| open.game == unpin.game && open.name == unpin.profile)
    .and_then(|open| match &mut open.status {
      ProfileStatus::Ready(installed) => installed.get_mut(&unpin.package),
      _ => None,
    });

  if let Some(package) = package {
    package.pinned = false;
  }

  Vec::new()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::install::DEFAULT_PROFILE;
  use crate::settings::{GameSettings, Settings};
  use crate::updates::Badges;
  use crate::{Msg, test_ecosystem, update};

  fn installed(packages: &[(&str, &str)]) -> Installed {
    crate::test_installed(packages)
  }

  fn pins(pins: &[(&str, &str)]) -> Pins {
    Pins {
      pins: pins
        .iter()
        .map(|(package, version)| (package.to_string(), version.to_string()))
        .collect(),
    }
  }

  fn pinned(installed: &Installed) -> Vec<&str> {
    installed
      .iter()
      .filter(|(_, package)| package.pinned)
      .map(|(full_name, _)| full_name.as_str())
      .collect()
  }

  #[test]
  fn a_package_is_pinned_only_at_the_version_its_pin_holds() {
    let mut profile = installed(&[
      ("Owner-Held", "1.0.0"),
      ("Owner-Moved", "2.0.0"),
      ("Owner-Free", "1.0.0"),
    ]);

    pins(&[
      ("Owner-Held", "1.0.0"),
      ("Owner-Moved", "1.0.0"),
      ("Owner-Gone", "1.0.0"),
    ])
    .mark(&mut profile);

    assert_eq!(pinned(&profile), vec!["Owner-Held"]);
  }

  #[test]
  fn installing_a_chosen_version_pins_it() {
    let mut kept = pins(&[("Owner-Other", "1.0.0")]);
    let after = installed(&[("Owner-Mod", "1.1.0"), ("Owner-Other", "1.0.0")]);

    kept.installed("Owner-Mod", Some("1.1.0"), &after);

    assert_eq!(
      kept,
      pins(&[("Owner-Mod", "1.1.0"), ("Owner-Other", "1.0.0")])
    );
  }

  #[test]
  fn choosing_another_version_moves_the_pin() {
    let mut kept = pins(&[("Owner-Mod", "1.1.0")]);

    kept.installed(
      "Owner-Mod",
      Some("1.0.0"),
      &installed(&[("Owner-Mod", "1.0.0")]),
    );

    assert_eq!(kept, pins(&[("Owner-Mod", "1.0.0")]));
  }

  #[test]
  fn installing_the_latest_version_unpins_the_package() {
    let mut kept = pins(&[("Owner-Mod", "1.2.0")]);

    kept.installed("Owner-Mod", None, &installed(&[("Owner-Mod", "1.2.0")]));

    assert_eq!(kept, Pins::default());
  }

  #[test]
  fn an_install_that_left_the_package_at_another_version_pins_nothing() {
    let mut kept = Pins::default();

    kept.installed(
      "Owner-Mod",
      Some("1.1.0"),
      &installed(&[("Owner-Mod", "1.0.0")]),
    );

    assert_eq!(kept, Pins::default());
  }

  #[test]
  fn pins_of_packages_gone_or_moved_are_dropped() {
    let mut kept = pins(&[
      ("Owner-Held", "1.0.0"),
      ("Owner-Moved", "1.0.0"),
      ("Owner-Gone", "1.0.0"),
    ]);

    kept.keep_installed(&installed(&[
      ("Owner-Held", "1.0.0"),
      ("Owner-Moved", "2.0.0"),
    ]));

    assert_eq!(kept, pins(&[("Owner-Held", "1.0.0")]));
  }

  #[test]
  fn changes_hold_every_pinned_package_but_the_one_changed_at_its_version() {
    let mut profile = installed(&[
      ("Owner-Held", "1.0.0"),
      ("Owner-Chosen", "1.0.0"),
      ("Owner-Free", "1.0.0"),
      ("Owner-Unlisted", "1.0.0"),
    ]);
    let index = crate::test_index(vec![
      crate::test_listed("Owner-Held", &["1.0.0", "2.0.0"], false),
      crate::test_listed("Owner-Chosen", &["1.0.0", "2.0.0"], false),
      crate::test_listed("Owner-Free", &["1.0.0", "2.0.0"], false),
      crate::test_listed("Owner-Unlisted", &["2.0.0"], false),
    ]);

    pins(&[
      ("Owner-Held", "1.0.0"),
      ("Owner-Chosen", "1.0.0"),
      ("Owner-Unlisted", "1.0.0"),
    ])
    .mark(&mut profile);

    assert_eq!(
      held(&profile, &index, &["Owner-Chosen".to_string()]),
      vec!["Owner-Held-1.0.0".to_string()]
    );
  }

  /// A state with valheim open, its Default profile holding `Owner-Held`
  /// pinned at 1.0.0 and `Owner-Free` at 1.0.0, and its list holding each
  /// at 1.0.0 and 2.0.0.
  fn ready() -> State {
    let mut state = State::new(test_ecosystem());
    let settings = Settings {
      last_game: Some("valheim".to_string()),
      games: [("valheim".to_string(), GameSettings::new("/games/valheim"))].into(),
      ..Settings::default()
    };
    let index = crate::test_index(vec![
      crate::test_listed("Owner-Held", &["1.0.0", "2.0.0"], false),
      crate::test_listed("Owner-Free", &["1.0.0", "2.0.0"], false),
    ]);
    let mut profile = installed(&[("Owner-Held", "1.0.0"), ("Owner-Free", "1.0.0")]);

    pins(&[("Owner-Held", "1.0.0")]).mark(&mut profile);
    update(&mut state, Msg::Started);
    update(&mut state, Msg::SettingsLoaded(Ok(settings)));
    update(
      &mut state,
      Msg::CachedIndexRead {
        community: "valheim".to_string(),
        index: Ok(Some(index)),
      },
    );
    update(
      &mut state,
      Msg::ProfileOpened {
        game: "valheim".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        installed: Ok(profile),
      },
    );

    state
  }

  fn unpin(package: &str) -> Effect {
    Effect::Run(Command::Unpin(Unpin {
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      package: package.to_string(),
    }))
  }

  fn unpinned(package: &str, result: Result<(), &str>) -> Msg {
    Msg::Unpinned {
      unpin: Unpin {
        game: "valheim".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        package: package.to_string(),
      },
      result: result.map_err(str::to_string),
    }
  }

  fn badge(state: &State, full_name: &str) -> Option<Badges> {
    state.badges().get(full_name).cloned()
  }

  #[test]
  fn unpinning_removes_the_mark_and_makes_the_package_updatable() {
    let mut state = ready();

    assert_eq!(
      update(&mut state, Msg::UnpinPressed("Owner-Held".to_string())),
      vec![unpin("Owner-Held")]
    );
    assert_eq!(update(&mut state, unpinned("Owner-Held", Ok(()))), vec![]);
    assert_eq!(
      badge(&state, "Owner-Held"),
      Some(Badges {
        update: Some("2.0.0".to_string()),
        ..Badges::default()
      })
    );
  }

  #[test]
  fn unpinning_a_package_that_is_not_pinned_does_nothing() {
    let mut state = ready();

    assert_eq!(
      update(&mut state, Msg::UnpinPressed("Owner-Free".to_string())),
      vec![]
    );
  }

  #[test]
  fn nothing_is_unpinned_while_the_profile_is_changing() {
    let mut state = ready();

    update(&mut state, Msg::UpdatePressed("Owner-Free".to_string()));

    assert_eq!(
      update(&mut state, Msg::UnpinPressed("Owner-Held".to_string())),
      vec![]
    );
  }

  #[test]
  fn a_failed_unpin_keeps_the_mark_and_says_why() {
    let mut state = ready();

    update(&mut state, Msg::UnpinPressed("Owner-Held".to_string()));

    let effects = update(&mut state, unpinned("Owner-Held", Err("read-only")));

    assert_eq!(
      effects,
      vec![Effect::Notify(Notice::UnpinFailed {
        package: "Owner-Held".to_string(),
        reason: "read-only".to_string(),
      })]
    );
    assert!(badge(&state, "Owner-Held").is_some_and(|badge| badge.pinned));
  }

  #[test]
  fn an_unreadable_pin_file_is_told() {
    let mut state = ready();

    let effects = update(&mut state, Msg::PinsUnread("pins.toml: bad".to_string()));

    assert_eq!(
      effects,
      vec![Effect::Notify(Notice::PinsUnread(
        "pins.toml: bad".to_string()
      ))]
    );
  }

  #[test]
  fn an_unpin_of_a_profile_not_open_changes_no_marks() {
    let mut state = ready();

    update(
      &mut state,
      Msg::Unpinned {
        unpin: Unpin {
          game: "valheim".to_string(),
          profile: "Other".to_string(),
          package: "Owner-Held".to_string(),
        },
        result: Ok(()),
      },
    );

    assert!(badge(&state, "Owner-Held").is_some_and(|badge| badge.pinned));
  }
}
