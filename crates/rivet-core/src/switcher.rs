//! The profile switcher: the open game's profiles, which one is selected, and
//! creating, renaming, duplicating, and deleting them.

use crate::install::{self, DEFAULT_PROFILE, ProfileStatus};
use crate::notice::Notice;
use crate::{Command, Effect, State};
use std::collections::BTreeMap;
use thunderstore_engine::profile::layout::{BadName, BadProfile, validate_name};

/// The open game's profiles and any change to them in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Switcher {
  /// The game, by its key in the game definitions.
  pub game: String,
  /// The game's profiles by name, sorted. Empty until they are listed.
  pub names: Vec<String>,
  /// A name being asked for.
  pub form: Option<NameForm>,
  /// The profile waiting for the user to confirm deleting it.
  pub deleting: Option<String>,
  /// The change being made on disk. No other starts until it ends.
  pub changing: Option<ProfileChange>,
  /// Why the profiles could not be listed, until they are.
  pub list_failure: Option<String>,
  /// How many packages each profile read since the game opened held when it
  /// was last read, by name.
  pub counts: BTreeMap<String, usize>,
}

impl Switcher {
  fn new(game: &str) -> Self {
    Self {
      game: game.to_string(),
      names: Vec::new(),
      form: None,
      deleting: None,
      changing: None,
      list_failure: None,
      counts: BTreeMap::new(),
    }
  }
}

/// What a name is being asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Naming {
  /// A new, empty profile.
  Create,
  /// A new name for the selected profile.
  Rename,
  /// A copy of the selected profile.
  Duplicate,
}

/// A name being asked for, and why the last one given was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameForm {
  pub naming: Naming,
  /// The profile selected when the form opened, which a rename or duplicate
  /// starts from.
  pub from: String,
  pub refusal: Option<NameRefusal>,
}

/// Why a profile name was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NameRefusal {
  /// The name breaks a rule for folder names, on this system or on Windows.
  Invalid(BadName),
  /// A profile with this name exists, by its name as written. Names that
  /// differ only in case count as the same, as they do on Windows.
  Taken(String),
}

/// A change to a game's profiles on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileChange {
  /// Create an empty profile.
  Create(String),
  Rename {
    from: String,
    to: String,
  },
  /// Copy a profile and everything installed in it under a new name.
  Duplicate {
    from: String,
    to: String,
  },
  Delete(String),
}

/// Whether `name` can be given to a profile of a game that has `names`,
/// ignoring `except`, the profile being renamed.
///
/// ```
/// use rivet_core::switcher::{NameRefusal, check_name};
///
/// let names = ["Default".to_string()];
///
/// assert_eq!(check_name(&names, "Modded", None), Ok(()));
/// assert_eq!(
///   check_name(&names, "default", None),
///   Err(NameRefusal::Taken("Default".to_string()))
/// );
/// ```
pub fn check_name(names: &[String], name: &str, except: Option<&str>) -> Result<(), NameRefusal> {
  if let Err(BadProfile::InvalidName { reason, .. }) = validate_name(name) {
    return Err(NameRefusal::Invalid(reason));
  }

  let name = name.to_lowercase();

  match names
    .iter()
    .filter(|existing| Some(existing.as_str()) != except)
    .find(|existing| existing.to_lowercase() == name)
  {
    Some(existing) => Err(NameRefusal::Taken(existing.clone())),
    None => Ok(()),
  }
}

impl State {
  /// The open game's profiles to show in the switcher: every one once listed,
  /// and until then the selected one alone.
  pub fn profile_names(&self) -> Vec<&str> {
    match &self.switcher {
      Some(switcher) if !switcher.names.is_empty() => {
        switcher.names.iter().map(String::as_str).collect()
      }
      _ => self
        .profile
        .iter()
        .map(|profile| profile.name.as_str())
        .collect(),
    }
  }

  /// How many packages the open game's profile `name` holds, when known: the
  /// open profile once read, or one read earlier while the game stayed open.
  pub fn mod_count(&self, name: &str) -> Option<usize> {
    let open = self
      .profile
      .as_ref()
      .filter(|profile| profile.name == name)
      .and_then(install::Profile::installed);

    match open {
      Some(installed) => Some(installed.len()),
      None => self.switcher.as_ref()?.counts.get(name).copied(),
    }
  }

  /// Whether another profile can be selected or a new one created: a game is
  /// open and no change to its profiles is being made.
  pub fn can_switch_profile(&self) -> bool {
    self
      .switcher
      .as_ref()
      .is_some_and(|switcher| switcher.changing.is_none())
  }

  /// Whether a change to the open game's profiles is being made on disk, so
  /// nothing may install into or launch from the open profile.
  pub fn is_changing_profile(&self) -> bool {
    self
      .switcher
      .as_ref()
      .is_some_and(|switcher| switcher.changing.is_some())
  }

  /// Whether the selected profile can be duplicated: it has been opened, no
  /// operation on it is running or waiting, and no launch is being prepared,
  /// any of which could be reading or writing it.
  pub fn can_duplicate_profile(&self) -> bool {
    let opened = self
      .profile
      .as_ref()
      .is_some_and(|profile| profile.status != ProfileStatus::Opening);

    self.can_switch_profile() && opened && !self.is_profile_busy() && !self.is_launching()
  }

  /// Whether the selected profile can be renamed or deleted: as for
  /// [`State::can_duplicate_profile`], and it is not the default profile,
  /// which every game keeps.
  pub fn can_rename_or_delete_profile(&self) -> bool {
    let default = self
      .profile
      .as_ref()
      .is_some_and(|profile| profile.name == DEFAULT_PROFILE);

    self.can_duplicate_profile() && !default
  }

  fn can_start(&self, naming: Naming) -> bool {
    match naming {
      Naming::Create => self.can_switch_profile(),
      Naming::Duplicate => self.can_duplicate_profile(),
      Naming::Rename => self.can_rename_or_delete_profile(),
    }
  }

  fn selected(&self) -> Option<&str> {
    self.profile.as_ref().map(|profile| profile.name.as_str())
  }
}

/// Sets up the switcher for `game`, whose profiles are listed once its
/// selected profile has opened, so a default profile it creates is included.
pub(crate) fn open(state: &mut State, game: &str) {
  state.switcher = Some(Switcher::new(game));
}

/// Records that `game`'s profile `name` was read holding `count` packages,
/// while `game` is open.
pub(crate) fn counted(state: &mut State, game: &str, name: &str, count: usize) {
  if let Some(switcher) = state
    .switcher
    .as_mut()
    .filter(|switcher| switcher.game == game)
  {
    switcher.counts.insert(name.to_string(), count);
  }
}

/// Lists `game`'s profiles after its selected profile opened, unless they
/// are listed already.
pub(crate) fn profile_opened(state: &State, game: &str) -> Vec<Effect> {
  match &state.switcher {
    Some(switcher) if switcher.game == game && switcher.names.is_empty() => {
      vec![Effect::Run(Command::ListProfiles(game.to_string()))]
    }
    _ => Vec::new(),
  }
}

pub(crate) fn listed(
  state: &mut State,
  game: String,
  names: Result<Vec<String>, String>,
) -> Vec<Effect> {
  let Some(switcher) = state
    .switcher
    .as_mut()
    .filter(|switcher| switcher.game == game)
  else {
    return Vec::new();
  };

  match names {
    Ok(mut names) => {
      names.sort();

      switcher.names = names;
      switcher.list_failure = None;
    }
    Err(reason) => switcher.list_failure = Some(reason),
  }

  Vec::new()
}

/// Selects `name`, a listed profile of the open game, and remembers it.
pub(crate) fn picked(state: &mut State, name: String) -> Vec<Effect> {
  let listed = state
    .switcher
    .as_ref()
    .is_some_and(|switcher| switcher.names.contains(&name));

  if !state.can_switch_profile() || !listed || state.selected() == Some(name.as_str()) {
    return Vec::new();
  }

  select(state, name)
}

/// Opens `name` as the open game's selected profile and remembers it.
fn select(state: &mut State, name: String) -> Vec<Effect> {
  let Some(switcher) = state.switcher.as_mut() else {
    return Vec::new();
  };
  let game = switcher.game.clone();

  switcher.form = None;
  switcher.deleting = None;
  state.confirmation = None;

  let mut effects = remember(state, &game, Some(&name));

  effects.extend(install::open_profile(state, &game, &name));

  effects
}

/// Records `name` as the profile to open `game` with, `None` meaning the
/// default profile, and saves the settings when that changed.
fn remember(state: &mut State, game: &str, name: Option<&str>) -> Vec<Effect> {
  let Some(settings) = state.settings.games.get_mut(game) else {
    return Vec::new();
  };
  let name = name.filter(|name| *name != DEFAULT_PROFILE);

  if settings.profile.as_deref() == name {
    return Vec::new();
  }

  settings.profile = name.map(str::to_string);

  vec![Effect::Run(Command::SaveSettings(state.settings.clone()))]
}

/// Asks for a name for `naming`, unless it cannot start now.
pub(crate) fn name_pressed(state: &mut State, naming: Naming) -> Vec<Effect> {
  if !state.can_start(naming) {
    return Vec::new();
  }

  let Some(from) = state.selected().map(str::to_string) else {
    return Vec::new();
  };
  let Some(switcher) = state.switcher.as_mut() else {
    return Vec::new();
  };
  let filled = match naming {
    Naming::Rename => from.clone(),
    Naming::Create | Naming::Duplicate => String::new(),
  };

  switcher.deleting = None;
  switcher.form = Some(NameForm {
    naming,
    from,
    refusal: None,
  });

  vec![Effect::FillProfileName(filled)]
}

/// Starts the change the form asked a name for, or refuses `name` with the
/// reason and changes nothing.
pub(crate) fn name_submitted(state: &mut State, name: String) -> Vec<Effect> {
  let Some(form) = state
    .switcher
    .as_ref()
    .and_then(|switcher| switcher.form.clone())
  else {
    return Vec::new();
  };

  if !state.can_start(form.naming) {
    return Vec::new();
  }

  let Some(switcher) = state.switcher.as_mut() else {
    return Vec::new();
  };

  if form.naming == Naming::Rename && name == form.from {
    switcher.form = None;

    return Vec::new();
  }

  let except = (form.naming == Naming::Rename).then_some(form.from.as_str());

  if let Err(refusal) = check_name(&switcher.names, &name, except) {
    switcher.form = Some(NameForm {
      refusal: Some(refusal),
      ..form
    });

    return Vec::new();
  }

  let change = match form.naming {
    Naming::Create => ProfileChange::Create(name),
    Naming::Rename => ProfileChange::Rename {
      from: form.from,
      to: name,
    },
    Naming::Duplicate => ProfileChange::Duplicate {
      from: form.from,
      to: name,
    },
  };

  switcher.form = None;

  start(switcher, change)
}

pub(crate) fn name_dismissed(state: &mut State) -> Vec<Effect> {
  if let Some(switcher) = state.switcher.as_mut() {
    switcher.form = None;
  }

  Vec::new()
}

/// Asks to confirm deleting the selected profile.
pub(crate) fn delete_pressed(state: &mut State) -> Vec<Effect> {
  if !state.can_rename_or_delete_profile() {
    return Vec::new();
  }

  let selected = state.selected().map(str::to_string);

  if let Some(switcher) = state.switcher.as_mut() {
    switcher.form = None;
    switcher.deleting = selected;
  }

  Vec::new()
}

pub(crate) fn delete_confirmed(state: &mut State) -> Vec<Effect> {
  if !state.can_rename_or_delete_profile() {
    return Vec::new();
  }

  let Some(switcher) = state.switcher.as_mut() else {
    return Vec::new();
  };
  let Some(name) = switcher.deleting.take() else {
    return Vec::new();
  };

  start(switcher, ProfileChange::Delete(name))
}

pub(crate) fn delete_dismissed(state: &mut State) -> Vec<Effect> {
  if let Some(switcher) = state.switcher.as_mut() {
    switcher.deleting = None;
  }

  Vec::new()
}

fn start(switcher: &mut Switcher, change: ProfileChange) -> Vec<Effect> {
  switcher.changing = Some(change.clone());

  vec![Effect::Run(Command::ChangeProfile {
    game: switcher.game.clone(),
    change,
  })]
}

/// Applies the result of `change` to `game`'s profiles, telling the user
/// whether it was made. A created or duplicated profile is selected, a
/// renamed one keeps its place, and deleting the selected profile selects
/// the default one.
///
/// What `game` opens with next, and the profile ended operations retry on,
/// are kept right even when the game was left while the change ran.
pub(crate) fn changed(
  state: &mut State,
  game: String,
  change: ProfileChange,
  result: Result<(), String>,
) -> Vec<Effect> {
  let showing = state
    .switcher
    .as_ref()
    .is_some_and(|switcher| switcher.game == game && switcher.changing.as_ref() == Some(&change));

  if let Some(switcher) = state.switcher.as_mut().filter(|_| showing) {
    switcher.changing = None;
  }

  if let Err(reason) = result {
    return vec![Effect::Notify(Notice::ProfileChangeFailed {
      change,
      reason,
    })];
  }

  let remembered = state
    .settings
    .games
    .get(&game)
    .and_then(|settings| settings.profile.clone());
  let mut effects = vec![Effect::Notify(Notice::ProfileChanged(change.clone()))];

  effects.extend(match &change {
    ProfileChange::Rename { from, to } if remembered.as_ref() == Some(from) => {
      remember(state, &game, Some(to))
    }
    ProfileChange::Delete(name) if remembered.as_ref() == Some(name) => {
      remember(state, &game, None)
    }
    _ => Vec::new(),
  });

  if !showing {
    return effects;
  }

  let Some(switcher) = state.switcher.as_mut() else {
    return effects;
  };
  let selected = state.profile.as_ref().map(|profile| profile.name.clone());
  let selected_count = state
    .profile
    .as_ref()
    .and_then(install::Profile::installed)
    .map(|installed| installed.len());

  match change {
    ProfileChange::Create(name) => {
      insert(&mut switcher.names, name.clone());
      effects.extend(select(state, name));
    }
    ProfileChange::Duplicate { from, to } => {
      let count = selected_count
        .filter(|_| selected.as_ref() == Some(&from))
        .or_else(|| switcher.counts.get(&from).copied());

      if let Some(count) = count {
        switcher.counts.insert(to.clone(), count);
      }

      insert(&mut switcher.names, to.clone());
      effects.extend(select(state, to));
    }
    ProfileChange::Rename { from, to } => {
      if let Some(count) = switcher.counts.remove(&from) {
        switcher.counts.insert(to.clone(), count);
      }

      switcher.names.retain(|name| *name != from);
      insert(&mut switcher.names, to.clone());

      if let Some(profile) = state.profile.as_mut().filter(|_| selected == Some(from)) {
        profile.name = to;
      }
    }
    ProfileChange::Delete(name) => {
      switcher.counts.remove(&name);
      switcher.names.retain(|existing| *existing != name);

      if selected == Some(name) {
        effects.extend(select(state, DEFAULT_PROFILE.to_string()));
      }
    }
  }

  effects
}

fn insert(names: &mut Vec<String>, name: String) {
  if let Err(position) = names.binary_search(&name) {
    names.insert(position, name);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::install::{Installed, Outcome};
  use crate::settings::{GameSettings, Settings};
  use crate::{Msg, test_ecosystem, update};

  fn settings(profile: Option<&str>) -> Settings {
    Settings {
      last_game: Some("valheim".to_string()),
      games: [(
        "valheim".to_string(),
        GameSettings {
          folder: "/games/valheim".into(),
          profile: profile.map(str::to_string),
        },
      )]
      .into(),
    }
  }

  /// A state that started with valheim open on `profile`, nothing answered
  /// yet, and the effects of starting.
  fn started_on(profile: Option<&str>) -> (State, Vec<Effect>) {
    let mut state = State::new(test_ecosystem());

    update(&mut state, Msg::Started);

    let effects = update(&mut state, Msg::SettingsLoaded(Ok(settings(profile))));

    (state, effects)
  }

  fn opened(name: &str) -> Msg {
    Msg::ProfileOpened {
      game: "valheim".to_string(),
      profile: name.to_string(),
      installed: Ok(Installed::new()),
    }
  }

  fn listed(names: &[&str]) -> Msg {
    Msg::ProfilesListed {
      game: "valheim".to_string(),
      names: Ok(strings(names)),
    }
  }

  fn strings(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
  }

  /// A state with valheim open on `selected`, its profiles `names` listed.
  fn ready(selected: &str, names: &[&str]) -> State {
    let remembered = (selected != DEFAULT_PROFILE).then_some(selected);
    let (mut state, _) = started_on(remembered);

    update(&mut state, opened(selected));
    update(&mut state, listed(names));

    state
  }

  fn open_profile(name: &str) -> Effect {
    Effect::Run(Command::OpenProfile {
      game: "valheim".to_string(),
      profile: name.to_string(),
    })
  }

  fn change(change: ProfileChange) -> Effect {
    Effect::Run(Command::ChangeProfile {
      game: "valheim".to_string(),
      change,
    })
  }

  fn changed(change: ProfileChange, result: Result<(), &str>) -> Msg {
    Msg::ProfileChanged {
      game: "valheim".to_string(),
      change,
      result: result.map_err(str::to_string),
    }
  }

  fn switcher(state: &State) -> &Switcher {
    state.switcher.as_ref().unwrap()
  }

  fn selected(state: &State) -> Option<(&str, &ProfileStatus)> {
    state
      .profile
      .as_ref()
      .map(|profile| (profile.name.as_str(), &profile.status))
  }

  fn remembered(state: &State) -> Option<&str> {
    state.settings.games["valheim"].profile.as_deref()
  }

  fn save(state: &State) -> Effect {
    Effect::Run(Command::SaveSettings(state.settings.clone()))
  }

  fn rename(from: &str, to: &str) -> ProfileChange {
    ProfileChange::Rename {
      from: from.to_string(),
      to: to.to_string(),
    }
  }

  fn duplicate(from: &str, to: &str) -> ProfileChange {
    ProfileChange::Duplicate {
      from: from.to_string(),
      to: to.to_string(),
    }
  }

  fn create(name: &str) -> ProfileChange {
    ProfileChange::Create(name.to_string())
  }

  fn delete(name: &str) -> ProfileChange {
    ProfileChange::Delete(name.to_string())
  }

  /// The notice that `change` was made.
  fn made(change: ProfileChange) -> Effect {
    Effect::Notify(Notice::ProfileChanged(change))
  }

  fn submit(state: &mut State, naming: Naming, name: &str) -> Vec<Effect> {
    update(state, Msg::ProfileNamePressed(naming));
    update(state, Msg::ProfileNameSubmitted(name.to_string()))
  }

  #[test]
  fn a_game_opens_on_the_profile_last_selected_for_it() {
    let (state, effects) = started_on(Some("Modded"));

    assert!(effects.contains(&open_profile("Modded")), "{effects:?}");
    assert_eq!(selected(&state), Some(("Modded", &ProfileStatus::Opening)));
  }

  #[test]
  fn a_game_with_no_profile_remembered_opens_the_default_one() {
    let (_, effects) = started_on(None);

    assert!(effects.contains(&open_profile("Default")), "{effects:?}");
  }

  #[test]
  fn the_profiles_are_listed_once_the_selected_one_opens() {
    let (mut state, _) = started_on(None);

    let effects = update(&mut state, opened("Default"));

    assert_eq!(
      effects,
      vec![Effect::Run(Command::ListProfiles("valheim".to_string()))]
    );
    assert_eq!(state.profile_names(), vec!["Default"]);

    update(&mut state, listed(&["Modded", "Default"]));

    assert_eq!(state.profile_names(), vec!["Default", "Modded"]);
  }

  #[test]
  fn profiles_that_cannot_be_listed_say_why() {
    let (mut state, _) = started_on(None);

    update(&mut state, opened("Default"));
    update(
      &mut state,
      Msg::ProfilesListed {
        game: "valheim".to_string(),
        names: Err("permission denied".to_string()),
      },
    );

    assert_eq!(
      switcher(&state).list_failure.as_deref(),
      Some("permission denied")
    );
    assert_eq!(state.profile_names(), vec!["Default"]);
  }

  #[test]
  fn picking_a_profile_opens_it_and_remembers_it() {
    let mut state = ready("Default", &["Default", "Modded"]);

    let effects = update(&mut state, Msg::ProfilePicked("Modded".to_string()));

    assert_eq!(remembered(&state), Some("Modded"));
    assert_eq!(effects, vec![save(&state), open_profile("Modded")]);
    assert_eq!(selected(&state), Some(("Modded", &ProfileStatus::Opening)));
  }

  #[test]
  fn picking_the_selected_or_an_unlisted_profile_does_nothing() {
    let mut state = ready("Default", &["Default", "Modded"]);

    assert_eq!(
      update(&mut state, Msg::ProfilePicked("Default".to_string())),
      vec![]
    );
    assert_eq!(
      update(&mut state, Msg::ProfilePicked("Gone".to_string())),
      vec![]
    );
    assert_eq!(remembered(&state), None);
  }

  #[test]
  fn creating_a_profile_adds_it_to_the_switcher_and_selects_it() {
    let mut state = ready("Default", &["Default"]);

    let effects = update(&mut state, Msg::ProfileNamePressed(Naming::Create));

    assert_eq!(effects, vec![Effect::FillProfileName(String::new())]);

    let effects = update(&mut state, Msg::ProfileNameSubmitted("Modded".to_string()));

    assert_eq!(effects, vec![change(create("Modded"))]);
    assert_eq!(switcher(&state).form, None);
    assert!(!state.can_switch_profile());

    let effects = update(&mut state, changed(create("Modded"), Ok(())));

    assert_eq!(state.profile_names(), vec!["Default", "Modded"]);
    assert_eq!(selected(&state), Some(("Modded", &ProfileStatus::Opening)));
    assert_eq!(
      effects,
      vec![made(create("Modded")), save(&state), open_profile("Modded")]
    );
    assert!(state.can_switch_profile());
  }

  #[test]
  fn an_invalid_name_is_refused_with_the_reason_and_nothing_is_created() {
    let mut state = ready("Default", &["Default"]);

    let effects = submit(&mut state, Naming::Create, "CON");

    assert_eq!(effects, vec![]);
    assert_eq!(
      switcher(&state)
        .form
        .as_ref()
        .and_then(|form| form.refusal.clone()),
      Some(NameRefusal::Invalid(BadName::Reserved))
    );
    assert_eq!(
      update(&mut state, Msg::ProfileNameSubmitted("a/b".to_string())),
      vec![]
    );
    assert_eq!(
      switcher(&state)
        .form
        .as_ref()
        .and_then(|form| form.refusal.clone()),
      Some(NameRefusal::Invalid(BadName::Escapes))
    );
    assert_eq!(state.profile_names(), vec!["Default"]);
  }

  #[test]
  fn a_name_another_profile_has_in_any_case_is_refused() {
    let mut state = ready("Default", &["Default", "Modded"]);

    let effects = submit(&mut state, Naming::Create, "modded");

    assert_eq!(effects, vec![]);
    assert_eq!(
      switcher(&state)
        .form
        .as_ref()
        .and_then(|form| form.refusal.clone()),
      Some(NameRefusal::Taken("Modded".to_string()))
    );
  }

  #[test]
  fn renaming_the_selected_profile_keeps_it_selected_under_its_new_name() {
    let mut state = ready("Modded", &["Default", "Modded"]);

    let effects = update(&mut state, Msg::ProfileNamePressed(Naming::Rename));

    assert_eq!(effects, vec![Effect::FillProfileName("Modded".to_string())]);

    let effects = update(&mut state, Msg::ProfileNameSubmitted("Heavy".to_string()));

    assert_eq!(effects, vec![change(rename("Modded", "Heavy"))]);

    let effects = update(&mut state, changed(rename("Modded", "Heavy"), Ok(())));

    assert_eq!(state.profile_names(), vec!["Default", "Heavy"]);
    assert_eq!(
      selected(&state),
      Some(("Heavy", &ProfileStatus::Ready(Installed::new())))
    );
    assert_eq!(remembered(&state), Some("Heavy"));
    assert_eq!(effects, vec![made(rename("Modded", "Heavy")), save(&state)]);
  }

  #[test]
  fn a_profile_can_be_renamed_to_a_different_case_of_its_own_name() {
    let mut state = ready("Modded", &["Default", "Modded"]);

    let effects = submit(&mut state, Naming::Rename, "modded");

    assert_eq!(effects, vec![change(rename("Modded", "modded"))]);
  }

  #[test]
  fn renaming_to_the_same_name_closes_the_form_and_changes_nothing() {
    let mut state = ready("Modded", &["Default", "Modded"]);

    let effects = submit(&mut state, Naming::Rename, "Modded");

    assert_eq!(effects, vec![]);
    assert_eq!(switcher(&state).form, None);
  }

  #[test]
  fn duplicating_copies_the_selected_profile_and_selects_the_copy() {
    let mut state = ready("Modded", &["Default", "Modded"]);

    let effects = submit(&mut state, Naming::Duplicate, "Modded copy");

    assert_eq!(effects, vec![change(duplicate("Modded", "Modded copy"))]);

    let effects = update(
      &mut state,
      changed(duplicate("Modded", "Modded copy"), Ok(())),
    );

    assert_eq!(
      state.profile_names(),
      vec!["Default", "Modded", "Modded copy"]
    );
    assert_eq!(remembered(&state), Some("Modded copy"));
    assert_eq!(
      effects,
      vec![
        made(duplicate("Modded", "Modded copy")),
        save(&state),
        open_profile("Modded copy")
      ]
    );
  }

  #[test]
  fn deleting_asks_for_confirmation_first() {
    let mut state = ready("Modded", &["Default", "Modded"]);

    let effects = update(&mut state, Msg::DeleteProfilePressed);

    assert_eq!(effects, vec![]);
    assert_eq!(switcher(&state).deleting.as_deref(), Some("Modded"));

    update(&mut state, Msg::DeleteProfileDismissed);

    assert_eq!(switcher(&state).deleting, None);
    assert_eq!(update(&mut state, Msg::DeleteProfileConfirmed), vec![]);
  }

  #[test]
  fn deleting_the_selected_profile_falls_back_to_the_default_one() {
    let mut state = ready("Modded", &["Default", "Modded"]);

    update(&mut state, Msg::DeleteProfilePressed);

    let effects = update(&mut state, Msg::DeleteProfileConfirmed);

    assert_eq!(effects, vec![change(delete("Modded"))]);

    let effects = update(&mut state, changed(delete("Modded"), Ok(())));

    assert_eq!(state.profile_names(), vec!["Default"]);
    assert_eq!(selected(&state), Some(("Default", &ProfileStatus::Opening)));
    assert_eq!(remembered(&state), None);
    assert_eq!(
      effects,
      vec![
        made(delete("Modded")),
        save(&state),
        open_profile("Default")
      ]
    );
  }

  #[test]
  fn the_default_profile_cannot_be_renamed_or_deleted_but_can_be_duplicated() {
    let mut state = ready("Default", &["Default"]);

    assert!(!state.can_rename_or_delete_profile());
    assert!(state.can_duplicate_profile());
    assert_eq!(
      update(&mut state, Msg::ProfileNamePressed(Naming::Rename)),
      vec![]
    );
    assert_eq!(update(&mut state, Msg::DeleteProfilePressed), vec![]);
    assert_eq!(switcher(&state).deleting, None);
  }

  #[test]
  fn a_failed_change_says_why_and_leaves_the_switcher_as_it_was() {
    let mut state = ready("Modded", &["Default", "Modded"]);

    update(&mut state, Msg::DeleteProfilePressed);
    update(&mut state, Msg::DeleteProfileConfirmed);

    let effects = update(&mut state, changed(delete("Modded"), Err("busy")));

    assert_eq!(
      effects,
      vec![Effect::Notify(Notice::ProfileChangeFailed {
        change: delete("Modded"),
        reason: "busy".to_string(),
      })]
    );
    assert_eq!(switcher(&state).changing, None);
    assert_eq!(state.profile_names(), vec!["Default", "Modded"]);
    assert_eq!(selected(&state).map(|(name, _)| name), Some("Modded"));
    assert!(state.can_switch_profile());
  }

  #[test]
  fn no_profile_can_be_picked_or_changed_while_a_change_is_made() {
    let mut state = ready("Default", &["Default", "Modded"]);

    submit(&mut state, Naming::Create, "New");

    assert_eq!(
      update(&mut state, Msg::ProfilePicked("Modded".to_string())),
      vec![]
    );
    assert_eq!(
      update(&mut state, Msg::ProfileNamePressed(Naming::Create)),
      vec![]
    );
  }

  #[test]
  fn nothing_installs_or_launches_while_the_open_profile_is_renamed() {
    let mut state = ready("Modded", &["Default", "Modded"]);

    state.host = Some(thunderstore_engine::profile::launch::HostOs::Linux);

    assert!(state.can_install());
    assert!(state.can_launch());

    submit(&mut state, Naming::Rename, "Renamed");

    assert!(!state.can_install());
    assert!(!state.can_launch());
    assert_eq!(
      update(
        &mut state,
        Msg::LaunchPressed(thunderstore_engine::profile::launch::LaunchMode::Vanilla)
      ),
      vec![]
    );

    update(&mut state, changed(rename("Modded", "Renamed"), Ok(())));

    assert!(state.can_install());
    assert!(state.can_launch());
  }

  #[test]
  fn the_selected_profile_cannot_be_changed_while_it_is_opening() {
    let (mut state, _) = started_on(Some("Modded"));

    update(&mut state, listed(&["Default", "Modded"]));

    assert!(!state.can_duplicate_profile());
    assert!(!state.can_rename_or_delete_profile());
  }

  /// A state with `Owner-Pack` installing into the selected `Modded`
  /// profile.
  fn installing() -> State {
    let mut state = ready("Modded", &["Default", "Modded"]);
    let index: crate::browse::Index = thunderstore_engine::models::PackageIndex::from(
      serde_json::from_value::<Vec<thunderstore_engine::models::Package>>(serde_json::json!([{
        "name": "Pack",
        "full_name": "Owner-Pack",
        "owner": "Owner",
        "date_created": "2024-01-01T00:00:00Z",
        "date_updated": "2024-01-01T00:00:00Z",
        "categories": [],
        "versions": [{
          "version_number": "1.0.0",
          "dependencies": [],
          "download_url": "https://example.com/Owner-Pack-1.0.0.zip",
          "date_created": "2024-01-01T00:00:00Z"
        }]
      }]))
      .unwrap(),
    )
    .into();

    update(
      &mut state,
      Msg::CachedIndexRead {
        community: "valheim".to_string(),
        index: Ok(Some(index)),
      },
    );
    update(&mut state, Msg::InstallPressed("Owner-Pack".to_string()));
    update(&mut state, Msg::InstallConfirmed);

    assert!(state.is_busy());

    state
  }

  #[test]
  fn while_an_install_runs_profiles_can_be_switched_and_created_but_not_changed() {
    let mut state = installing();

    assert!(state.can_switch_profile());
    assert!(!state.can_duplicate_profile());
    assert!(!state.can_rename_or_delete_profile());
    assert_eq!(update(&mut state, Msg::DeleteProfilePressed), vec![]);

    let effects = update(&mut state, Msg::ProfilePicked("Default".to_string()));

    assert!(effects.contains(&open_profile("Default")), "{effects:?}");
  }

  #[test]
  fn an_install_into_a_profile_switched_away_from_leaves_the_selected_one_alone() {
    let mut state = installing();

    update(&mut state, Msg::ProfilePicked("Default".to_string()));
    update(&mut state, opened("Default"));
    update(
      &mut state,
      Msg::OperationFinished {
        operation: 1,
        outcome: Ok(Outcome {
          changed: strings(&["Owner-Pack"]),
          failed: vec![],
          cancelled: vec![],
          profile: Ok(crate::test_installed(&[("Owner-Pack", "1.0.0")])),
        }),
      },
    );

    assert!(!state.is_installed("Owner-Pack"));
  }

  #[test]
  fn a_rename_that_ends_after_leaving_the_game_still_updates_the_remembered_profile() {
    let mut state = ready("Modded", &["Default", "Modded"]);

    submit(&mut state, Naming::Rename, "Heavy");
    update(&mut state, Msg::SwitchGamePressed);

    let effects = update(&mut state, changed(rename("Modded", "Heavy"), Ok(())));

    assert_eq!(state.switcher, None);
    assert_eq!(remembered(&state), Some("Heavy"));
    assert_eq!(effects, vec![made(rename("Modded", "Heavy")), save(&state)]);
  }

  #[test]
  fn leaving_the_game_closes_the_switcher() {
    let mut state = ready("Default", &["Default"]);

    update(&mut state, Msg::ProfileNamePressed(Naming::Create));
    update(&mut state, Msg::SwitchGamePressed);

    assert_eq!(state.switcher, None);
  }

  fn opened_with(name: &str, packages: &[(&str, &str)]) -> Msg {
    Msg::ProfileOpened {
      game: "valheim".to_string(),
      profile: name.to_string(),
      installed: Ok(crate::test_installed(packages)),
    }
  }

  fn counts(state: &State) -> Vec<(&str, Option<usize>)> {
    state
      .profile_names()
      .into_iter()
      .map(|name| (name, state.mod_count(name)))
      .collect()
  }

  #[test]
  fn a_profile_s_mod_count_is_known_once_it_has_been_read() {
    let (mut state, _) = started_on(None);

    update(
      &mut state,
      opened_with("Default", &[("Owner-A", "1.0.0"), ("Owner-B", "1.0.0")]),
    );
    update(&mut state, listed(&["Default", "Modded"]));

    assert_eq!(counts(&state), vec![("Default", Some(2)), ("Modded", None)]);

    update(&mut state, Msg::ProfilePicked("Modded".to_string()));

    assert_eq!(counts(&state), vec![("Default", Some(2)), ("Modded", None)]);

    update(&mut state, opened_with("Modded", &[("Owner-A", "1.0.0")]));

    assert_eq!(
      counts(&state),
      vec![("Default", Some(2)), ("Modded", Some(1))]
    );
  }

  #[test]
  fn an_operation_on_a_profile_not_open_updates_its_mod_count() {
    let (mut state, _) = started_on(None);

    update(&mut state, opened_with("Default", &[("Owner-A", "1.0.0")]));
    update(&mut state, listed(&["Default", "Modded"]));
    update(&mut state, Msg::DisableAllPressed);
    update(&mut state, Msg::ProfilePicked("Modded".to_string()));
    update(&mut state, opened("Modded"));
    update(
      &mut state,
      Msg::OperationFinished {
        operation: 1,
        outcome: Ok(Outcome {
          changed: strings(&["Owner-A"]),
          failed: vec![],
          cancelled: vec![],
          profile: Ok(crate::test_installed(&[
            ("Owner-A", "1.0.0"),
            ("Owner-B", "1.0.0"),
          ])),
        }),
      },
    );

    assert_eq!(
      counts(&state),
      vec![("Default", Some(2)), ("Modded", Some(0))]
    );
  }

  #[test]
  fn a_mod_count_follows_its_profile_through_a_rename_a_copy_and_a_delete() {
    let (mut state, _) = started_on(Some("Modded"));

    update(&mut state, opened_with("Modded", &[("Owner-A", "1.0.0")]));
    update(&mut state, listed(&["Default", "Modded"]));
    submit(&mut state, Naming::Rename, "Heavy");
    update(&mut state, changed(rename("Modded", "Heavy"), Ok(())));

    assert_eq!(counts(&state), vec![("Default", None), ("Heavy", Some(1))]);

    submit(&mut state, Naming::Duplicate, "Copy");
    update(&mut state, changed(duplicate("Heavy", "Copy"), Ok(())));

    assert_eq!(
      counts(&state),
      vec![("Copy", Some(1)), ("Default", None), ("Heavy", Some(1))]
    );

    update(&mut state, opened_with("Copy", &[("Owner-A", "1.0.0")]));
    update(&mut state, Msg::DeleteProfilePressed);
    update(&mut state, Msg::DeleteProfileConfirmed);
    update(&mut state, changed(delete("Copy"), Ok(())));

    assert_eq!(state.mod_count("Copy"), None);
  }
}
