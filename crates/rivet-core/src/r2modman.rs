//! Offering to copy the open game's r2modman profiles in as new profiles.
//! r2modman's own files are only read.

use crate::notice::Notice;
use crate::portability::{self, ReadFailure, Shared, Source};
use crate::{Command, Effect, State};
use std::path::PathBuf;

/// The open game's r2modman profiles and the offer to import them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct R2modman {
  /// The game, by its key in the game definitions.
  pub game: String,
  /// The profiles r2modman holds for the game. Empty until they are found.
  pub found: Vec<Found>,
  /// The offer, while it is shown: the folders of the profiles chosen to
  /// import.
  pub offer: Option<Vec<PathBuf>>,
  /// The folders of the accepted profiles still being read.
  pub reading: Vec<PathBuf>,
}

/// A profile r2modman holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
  pub name: String,
  /// The profile's folder in r2modman's data.
  pub folder: PathBuf,
}

impl State {
  /// Whether the offer to import r2modman's profiles can be made from the
  /// menu: r2modman holds profiles for the open game.
  pub fn can_offer_r2modman(&self) -> bool {
    self
      .r2modman
      .as_ref()
      .is_some_and(|r2modman| !r2modman.found.is_empty())
  }

  /// Whether the offer can be accepted: a profile is chosen, the package
  /// list is loaded, and no change to the game's profiles is being made.
  pub fn can_import_r2modman(&self) -> bool {
    let chosen = self
      .r2modman
      .as_ref()
      .and_then(|r2modman| r2modman.offer.as_ref())
      .is_some_and(|chosen| !chosen.is_empty());
    let listed = self
      .browse
      .as_ref()
      .is_some_and(|browse| browse.index.is_some());

    chosen && listed && self.can_switch_profile()
  }
}

impl R2modman {
  /// Shows the offer with every profile found chosen.
  fn offer_all(&mut self) {
    self.offer = Some(
      self
        .found
        .iter()
        .map(|found| found.folder.clone())
        .collect(),
    );
  }
}

/// Makes the offer again, with every profile chosen, unless
/// [`State::can_offer_r2modman`] says there is nothing to offer.
pub(crate) fn pressed(state: &mut State) -> Vec<Effect> {
  if let Some(r2modman) = state
    .r2modman
    .as_mut()
    .filter(|r2modman| !r2modman.found.is_empty())
  {
    r2modman.offer_all();
  }

  Vec::new()
}

/// Chooses the profile in `folder`, or unchooses it if it was, keeping the
/// chosen ones in the order they were found.
pub(crate) fn toggled(state: &mut State, folder: PathBuf) -> Vec<Effect> {
  let Some(r2modman) = state.r2modman.as_mut() else {
    return Vec::new();
  };
  let Some(chosen) = r2modman.offer.as_ref() else {
    return Vec::new();
  };
  let toggled = r2modman
    .found
    .iter()
    .map(|found| &found.folder)
    .filter(|found| (**found == folder) != chosen.contains(found))
    .cloned()
    .collect();

  r2modman.offer = Some(toggled);

  Vec::new()
}

/// Reads each chosen profile to import it, closes the offer, and remembers
/// it was answered, unless [`State::can_import_r2modman`] says nothing can be
/// imported.
pub(crate) fn accepted(state: &mut State) -> Vec<Effect> {
  if !state.can_import_r2modman() {
    return Vec::new();
  }

  let Some(community) = state.browse.as_ref().map(|browse| browse.community.clone()) else {
    return Vec::new();
  };
  let Some(r2modman) = state.r2modman.as_mut() else {
    return Vec::new();
  };
  let chosen = r2modman.offer.take().unwrap_or_default();
  let game = r2modman.game.clone();
  let mut effects: Vec<Effect> = chosen
    .iter()
    .map(|folder| {
      Effect::Run(Command::ReadShared {
        community: community.clone(),
        source: Source::R2modman(folder.clone()),
      })
    })
    .collect();

  r2modman.reading.extend(chosen);
  effects.extend(answer(state, &game));

  effects
}

/// Queues importing the accepted profile read from `folder` under its own
/// name or the first free one after it, leaving out and telling the packages
/// the package list lacks, or tells why it could not be read.
pub(crate) fn read(
  state: &mut State,
  folder: PathBuf,
  shared: Result<Shared, ReadFailure>,
) -> Vec<Effect> {
  let Some(r2modman) = state.r2modman.as_mut() else {
    return Vec::new();
  };
  let Some(at) = r2modman
    .reading
    .iter()
    .position(|reading| *reading == folder)
  else {
    return Vec::new();
  };

  r2modman.reading.remove(at);

  let shared = match shared {
    Ok(shared) => shared,
    Err(failure) => {
      let profile = r2modman
        .found
        .iter()
        .find(|found| found.folder == folder)
        .map_or_else(|| folder.display().to_string(), |found| found.name.clone());

      return vec![Effect::Notify(Notice::R2modmanUnread { profile, failure })];
    }
  };
  let missing = state
    .browse
    .as_ref()
    .and_then(|browse| browse.index.as_ref())
    .map(|index| portability::missing(&index.0, &shared.packages))
    .unwrap_or_default();
  let name = portability::free_name(&state.taken_profile_names(), &shared.name);
  let told = (!missing.is_empty()).then(|| {
    Effect::Notify(Notice::ImportIncomplete {
      profile: name.clone(),
      missing: missing.clone(),
    })
  });

  told
    .into_iter()
    .chain(portability::queue_import(state, name, shared, &missing))
    .collect()
}

/// Closes the offer and remembers it was answered.
pub(crate) fn declined(state: &mut State) -> Vec<Effect> {
  let Some(r2modman) = state.r2modman.as_mut() else {
    return Vec::new();
  };

  r2modman.offer = None;

  let game = r2modman.game.clone();

  answer(state, &game)
}

/// Remembers the offer for `game` was answered, saving the settings when it
/// had not been.
fn answer(state: &mut State, game: &str) -> Vec<Effect> {
  let Some(settings) = state.settings.games.get_mut(game) else {
    return Vec::new();
  };

  if settings.r2modman_answered {
    return Vec::new();
  }

  settings.r2modman_answered = true;

  vec![Effect::Run(Command::SaveSettings(state.settings.clone()))]
}

/// Looks for r2modman's profiles of `game` once its own profiles are first
/// listed after it opened.
pub(crate) fn listed(state: &mut State, game: &str) -> Vec<Effect> {
  if state
    .r2modman
    .as_ref()
    .is_some_and(|r2modman| r2modman.game == game)
  {
    return Vec::new();
  }

  let folder = state
    .ecosystem
    .game(game)
    .and_then(|definition| definition.profile())
    .and_then(|profile| profile.settings_identifier.clone());
  let Some(folder) = folder else {
    return Vec::new();
  };

  state.r2modman = Some(R2modman {
    game: game.to_string(),
    found: Vec::new(),
    offer: None,
    reading: Vec::new(),
  });

  vec![Effect::Run(Command::FindR2modmanProfiles {
    game: game.to_string(),
    folder,
  })]
}

/// Records r2modman's `profiles` of `game`, while it is open, and offers
/// them with every one chosen unless the offer was answered before.
pub(crate) fn found(state: &mut State, game: String, profiles: Vec<Found>) -> Vec<Effect> {
  let answered = state
    .settings
    .games
    .get(&game)
    .is_some_and(|settings| settings.r2modman_answered);
  let Some(r2modman) = state
    .r2modman
    .as_mut()
    .filter(|r2modman| r2modman.game == game)
  else {
    return Vec::new();
  };

  r2modman.found = profiles;

  if !answered && !r2modman.found.is_empty() {
    r2modman.offer_all();
  }

  Vec::new()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::install::{Action, DEFAULT_PROFILE};
  use crate::portability::{Import, SharedPackage};
  use crate::settings::{GameSettings, Settings};
  use crate::{Msg, test_ecosystem, test_installed, update};

  /// A state with valheim open on its Default profile and its package list
  /// loaded, before its profiles are listed.
  fn opened() -> State {
    let mut state = State::new(test_ecosystem());
    let settings = Settings {
      last_game: Some("valheim".to_string()),
      games: [("valheim".to_string(), GameSettings::new("/games/valheim"))].into(),
      ..Settings::default()
    };

    update(&mut state, Msg::Started);
    update(&mut state, Msg::SettingsLoaded(Ok(settings)));
    update(
      &mut state,
      Msg::ProfileOpened {
        game: "valheim".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        installed: Ok(test_installed(&[])),
      },
    );

    state
  }

  fn list_profiles(state: &mut State) -> Vec<Effect> {
    update(
      state,
      Msg::ProfilesListed {
        game: "valheim".to_string(),
        names: Ok(vec![DEFAULT_PROFILE.to_string()]),
      },
    )
  }

  fn find() -> Effect {
    Effect::Run(Command::FindR2modmanProfiles {
      game: "valheim".to_string(),
      folder: "Valheim".to_string(),
    })
  }

  #[test]
  fn listing_a_games_profiles_looks_for_its_r2modman_profiles_once() {
    let mut state = opened();

    assert_eq!(list_profiles(&mut state), vec![find()]);
    assert_eq!(list_profiles(&mut state), vec![]);
  }

  fn found(name: &str) -> Found {
    Found {
      name: name.to_string(),
      folder: PathBuf::from(format!("/r2/Valheim/profiles/{name}")),
    }
  }

  fn folders(names: &[&str]) -> Vec<PathBuf> {
    names.iter().map(|name| found(name).folder).collect()
  }

  /// A state that has found `names` among r2modman's valheim profiles.
  fn finding(state: &mut State, names: &[&str]) -> Vec<Effect> {
    list_profiles(state);

    update(
      state,
      Msg::R2modmanFound {
        game: "valheim".to_string(),
        profiles: names.iter().map(|name| found(name)).collect(),
      },
    )
  }

  fn offered(state: &State) -> Option<Vec<PathBuf>> {
    state.r2modman.as_ref()?.offer.clone()
  }

  #[test]
  fn found_profiles_are_offered_with_every_one_chosen() {
    let mut state = opened();

    assert_eq!(finding(&mut state, &["Default", "Modded"]), vec![]);
    assert_eq!(offered(&state), Some(folders(&["Default", "Modded"])));
  }

  #[test]
  fn nothing_is_offered_when_r2modman_has_no_profiles_for_the_game() {
    let mut state = opened();

    finding(&mut state, &[]);

    assert_eq!(offered(&state), None);
  }

  fn save(state: &State) -> Effect {
    Effect::Run(Command::SaveSettings(state.settings.clone()))
  }

  #[test]
  fn declining_the_offer_is_remembered_so_it_is_not_made_again() {
    let mut state = opened();

    finding(&mut state, &["Default"]);

    let effects = update(&mut state, Msg::R2modmanDeclined);

    assert!(state.settings.games["valheim"].r2modman_answered);
    assert_eq!(effects, vec![save(&state)]);
    assert_eq!(offered(&state), None);

    let mut restarted = opened();

    restarted.settings = state.settings.clone();
    finding(&mut restarted, &["Default"]);

    assert_eq!(offered(&restarted), None);
  }

  #[test]
  fn a_declined_offer_can_be_made_again_from_the_menu() {
    let mut state = opened();

    finding(&mut state, &["Default", "Modded"]);
    update(&mut state, Msg::R2modmanDeclined);

    assert!(state.can_offer_r2modman());
    assert_eq!(update(&mut state, Msg::R2modmanPressed), vec![]);
    assert_eq!(offered(&state), Some(folders(&["Default", "Modded"])));
    assert_eq!(update(&mut state, Msg::R2modmanDeclined), vec![]);
  }

  #[test]
  fn the_menu_offers_nothing_when_r2modman_has_no_profiles_for_the_game() {
    let mut state = opened();

    finding(&mut state, &[]);

    assert!(!state.can_offer_r2modman());
    assert_eq!(update(&mut state, Msg::R2modmanPressed), vec![]);
    assert_eq!(offered(&state), None);
  }

  /// [`opened`], with the package list loaded.
  fn ready() -> State {
    let mut state = opened();

    update(
      &mut state,
      Msg::CachedIndexRead {
        community: "valheim".to_string(),
        index: Ok(Some(crate::test_shared_index())),
      },
    );

    state
  }

  fn read(name: &str) -> Effect {
    Effect::Run(Command::ReadShared {
      community: "valheim".to_string(),
      source: Source::R2modman(found(name).folder),
    })
  }

  fn toggle(state: &mut State, name: &str) {
    update(state, Msg::R2modmanToggled(found(name).folder));
  }

  #[test]
  fn accepting_reads_each_chosen_profile_and_remembers_the_answer() {
    let mut state = ready();

    finding(&mut state, &["Default", "Modded", "Testing"]);
    toggle(&mut state, "Modded");

    assert_eq!(offered(&state), Some(folders(&["Default", "Testing"])));

    let effects = update(&mut state, Msg::R2modmanAccepted);

    assert!(state.settings.games["valheim"].r2modman_answered);
    assert_eq!(
      effects,
      vec![read("Default"), read("Testing"), save(&state)]
    );
    assert_eq!(offered(&state), None);
  }

  #[test]
  fn a_profile_unchosen_can_be_chosen_again() {
    let mut state = ready();

    finding(&mut state, &["Default", "Modded"]);
    toggle(&mut state, "Default");
    toggle(&mut state, "Default");

    assert_eq!(offered(&state), Some(folders(&["Default", "Modded"])));
  }

  #[test]
  fn nothing_is_imported_with_no_profile_chosen_or_before_the_package_list_loads() {
    let mut unchosen = ready();

    finding(&mut unchosen, &["Default"]);
    toggle(&mut unchosen, "Default");

    assert!(!unchosen.can_import_r2modman());
    assert_eq!(update(&mut unchosen, Msg::R2modmanAccepted), vec![]);

    let mut unlisted = opened();

    finding(&mut unlisted, &["Default"]);

    assert!(!unlisted.can_import_r2modman());
    assert_eq!(update(&mut unlisted, Msg::R2modmanAccepted), vec![]);
    assert_eq!(offered(&unlisted), Some(folders(&["Default"])));
  }

  fn package(full_name: &str, version: &str, enabled: bool) -> SharedPackage {
    SharedPackage {
      full_name: full_name.to_string(),
      version: version.to_string(),
      enabled,
    }
  }

  /// r2modman's profile `name` as read, holding `packages`.
  fn shared(name: &str, packages: Vec<SharedPackage>) -> Shared {
    Shared {
      name: name.to_string(),
      packages,
      archive: format!("{name} configs").into_bytes(),
    }
  }

  /// A state that accepted importing r2modman's profiles `names`.
  fn accepting(names: &[&str]) -> State {
    let mut state = ready();

    finding(&mut state, names);
    update(&mut state, Msg::R2modmanAccepted);

    state
  }

  fn read_back(state: &mut State, name: &str, shared: Result<Shared, ReadFailure>) -> Vec<Effect> {
    update(
      state,
      Msg::SharedRead {
        source: Source::R2modman(found(name).folder),
        shared,
      },
    )
  }

  fn queued(state: &State) -> Vec<(&str, Action)> {
    state
      .queue
      .operations
      .iter()
      .map(|operation| (operation.profile.as_str(), operation.action.clone()))
      .collect()
  }

  fn import_of(names: &[&str]) -> Action {
    Action::Import(names.iter().map(|name| name.to_string()).collect())
  }

  #[test]
  fn each_read_profile_is_queued_for_import_under_a_free_name() {
    let mut state = accepting(&["Default", "Modded"]);
    let index = state.browse.as_ref().unwrap().index.clone().unwrap();
    let packages = vec![
      package("Owner-ModA", "1.0.0", true),
      package("Owner-ModC", "1.0.0", false),
    ];

    let effects = read_back(
      &mut state,
      "Default",
      Ok(shared("Default", packages.clone())),
    );

    assert_eq!(
      effects,
      vec![Effect::Run(Command::Import(Import {
        operation: 1,
        game: "valheim".to_string(),
        community: "valheim".to_string(),
        profile: "Default 2".to_string(),
        packages,
        archive: b"Default configs".to_vec(),
        index,
      }))]
    );

    read_back(
      &mut state,
      "Modded",
      Ok(shared("Modded", vec![package("Owner-ModB", "1.0.0", true)])),
    );

    assert_eq!(
      queued(&state),
      vec![
        ("Default 2", import_of(&["Owner-ModA", "Owner-ModC"])),
        ("Modded", import_of(&["Owner-ModB"])),
      ]
    );
  }

  #[test]
  fn packages_the_index_lacks_are_left_out_and_told() {
    let mut state = accepting(&["Modded"]);

    let effects = read_back(
      &mut state,
      "Modded",
      Ok(shared(
        "Modded",
        vec![
          package("Owner-ModA", "1.0.0", true),
          package("Owner-Gone", "1.0.0", true),
          package("Owner-ModC", "3.0.0", true),
        ],
      )),
    );

    assert_eq!(
      effects.first(),
      Some(&Effect::Notify(Notice::ImportIncomplete {
        profile: "Modded".to_string(),
        missing: vec![
          "Owner-Gone-1.0.0".to_string(),
          "Owner-ModC-3.0.0".to_string()
        ],
      }))
    );
    assert_eq!(queued(&state), vec![("Modded", import_of(&["Owner-ModA"]))]);
  }

  #[test]
  fn a_profile_that_cannot_be_read_is_told_and_not_imported() {
    let mut state = accepting(&["Modded"]);
    let failure = ReadFailure::Unreadable("permission denied".to_string());

    let effects = read_back(&mut state, "Modded", Err(failure.clone()));

    assert_eq!(
      effects,
      vec![Effect::Notify(Notice::R2modmanUnread {
        profile: "Modded".to_string(),
        failure,
      })]
    );
    assert_eq!(queued(&state), vec![]);
  }

  #[test]
  fn a_profile_read_without_being_accepted_is_ignored() {
    let mut state = ready();

    finding(&mut state, &["Modded"]);

    let effects = read_back(&mut state, "Modded", Ok(shared("Modded", vec![])));

    assert_eq!(effects, vec![]);
    assert_eq!(queued(&state), vec![]);
  }

  #[test]
  fn profiles_found_for_a_game_no_longer_open_are_ignored() {
    let mut state = opened();

    list_profiles(&mut state);
    update(&mut state, Msg::SwitchGamePressed);
    update(
      &mut state,
      Msg::R2modmanFound {
        game: "valheim".to_string(),
        profiles: vec![found("Default")],
      },
    );

    assert_eq!(state.r2modman, None);
  }
}
