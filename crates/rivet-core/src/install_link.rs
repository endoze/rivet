//! Thunderstore's "Install with Mod Manager" links: finding which of the
//! user's games the linked package is for, asking which profile to install it
//! into, installing it there, and whether Rivet is the app that opens them.

use crate::browse::Status;
use crate::install::{self, ProfileStatus};
use crate::notice::Notice;
use crate::picker;
use crate::{Command, Effect, State};

/// A package version a `ror2mm://` link asks to install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallLink {
  pub owner: String,
  pub name: String,
  pub version: String,
}

impl InstallLink {
  /// Reads `link`, as Thunderstore writes it:
  /// `ror2mm://v1/install/thunderstore.io/{owner}/{name}/{version}/`. The
  /// trailing slash is optional, and the host may be a subdomain of
  /// `thunderstore.io`, as older links used. `None` for anything else.
  ///
  /// ```
  /// use rivet_core::install_link::InstallLink;
  ///
  /// let link =
  ///   InstallLink::parse("ror2mm://v1/install/thunderstore.io/BepInEx/BepInExPack/5.4.2305/")
  ///     .unwrap();
  ///
  /// assert_eq!(link.full_name(), "BepInEx-BepInExPack");
  /// assert_eq!(link.version, "5.4.2305");
  /// ```
  pub fn parse(link: &str) -> Option<Self> {
    let scheme = link.get(..SCHEME.len())?;

    if !scheme.eq_ignore_ascii_case(SCHEME) {
      return None;
    }

    let path = &link[SCHEME.len()..];
    let path = path.strip_suffix('/').unwrap_or(path);
    let ["v1", "install", host, owner, name, version] = path.split('/').collect::<Vec<_>>()[..]
    else {
      return None;
    };
    let host = host.to_ascii_lowercase();
    let thunderstore = host == HOST
      || host
        .strip_suffix(HOST)
        .is_some_and(|subdomain| subdomain.ends_with('.') && subdomain.len() > 1);

    if !thunderstore || !is_owner(owner) || !is_name(name) || !is_version(version) {
      return None;
    }

    Some(Self {
      owner: owner.to_string(),
      name: name.to_string(),
      version: version.to_string(),
    })
  }

  /// The package as `Owner-Name`.
  pub fn full_name(&self) -> String {
    format!("{}-{}", self.owner, self.name)
  }
}

const SCHEME: &str = "ror2mm://";

const HOST: &str = "thunderstore.io";

/// Whether `owner` is a name Thunderstore allows a team: letters, digits and
/// underscores, starting and ending with a letter or digit.
fn is_owner(owner: &str) -> bool {
  let alphanumeric = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());

  is_name(owner) && alphanumeric(owner.chars().next()) && alphanumeric(owner.chars().last())
}

/// Whether `name` is a name Thunderstore allows a package: letters, digits
/// and underscores.
fn is_name(name: &str) -> bool {
  !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Whether `version` is three dot-separated numbers, as Thunderstore requires.
fn is_version(version: &str) -> bool {
  let parts: Vec<&str> = version.split('.').collect();

  parts.len() == 3
    && parts
      .iter()
      .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

/// How far acting on an install link has got.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Linking {
  /// Asking Thunderstore which communities list the package.
  Finding(InstallLink),
  /// Asking the user which game, then which profile, to install into.
  Choosing(Choice),
  /// Confirmed: waiting for the game, profile, and package list to load,
  /// then for the list to hold the version, to queue the install.
  Installing {
    link: InstallLink,
    /// The game, by its key in the game definitions.
    game: String,
    profile: String,
  },
}

/// The games an install link's package can go into, and the user's choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
  pub link: InstallLink,
  /// The set-up games, by key, whose community lists the package and whose
  /// mods Rivet installs, sorted by name. Never empty.
  pub games: Vec<String>,
  /// Where to install, once a game is chosen. Chosen from the start when
  /// only one game lists the package.
  pub target: Option<Target>,
}

/// The game and profile an install link's package goes into.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
  /// The game, by its key in the game definitions.
  pub game: String,
  /// The game's profiles, sorted: every one once listed, until then the
  /// chosen one alone.
  pub profiles: Vec<String>,
  /// The profile chosen, at first the one the game last opened.
  pub profile: String,
}

/// When Rivet makes itself the app that opens install links. r2modman opens
/// them too, and claims them on each of its starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
  /// Only when no other app opens them.
  IfUnclaimed,
  /// Whichever app opens them now.
  Always,
  /// Never, and remove Rivet's own registration if it opens them.
  Release,
}

/// Registers Rivet for install links as the user chose in settings, or
/// else when no other app opens them, to learn whether Rivet opens them.
pub(crate) fn register(state: &State) -> Vec<Effect> {
  let claim = match state.settings.install_links {
    None => Claim::IfUnclaimed,
    Some(true) => Claim::Always,
    Some(false) => Claim::Release,
  };

  vec![Effect::Run(Command::RegisterInstallLinks(claim))]
}

/// Records whether Rivet opens install links now, unknown when registering
/// failed.
pub(crate) fn registered(state: &mut State, opens: Result<bool, String>) -> Vec<Effect> {
  state.install_links = opens.ok();

  Vec::new()
}

/// Records whether the user wants Rivet to open install links, saves it, and
/// claims or releases them.
pub(crate) fn links_chosen(state: &mut State, on: bool) -> Vec<Effect> {
  state.settings.install_links = Some(on);

  let mut effects = vec![Effect::Run(Command::SaveSettings(state.settings.clone()))];

  effects.extend(register(state));

  effects
}

/// Why Thunderstore could not say which communities list a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ListingFailure {
  /// Thunderstore has no such package.
  Unknown,
  /// Thunderstore could not be reached or answered with an error, with the
  /// reason.
  Unreachable(String),
}

/// Applies Thunderstore's answer about the link being found: asks for the
/// game and profile to install into, or tells why there is none.
pub(crate) fn listings_found(
  state: &mut State,
  link: InstallLink,
  communities: Result<Vec<String>, ListingFailure>,
) -> Vec<Effect> {
  if state.install_link != Some(Linking::Finding(link.clone())) {
    return Vec::new();
  }

  state.install_link = None;

  let communities = match communities {
    Ok(communities) => communities,
    Err(ListingFailure::Unknown) => {
      return vec![Effect::Notify(Notice::InstallLinkUnknown(link.full_name()))];
    }
    Err(ListingFailure::Unreachable(reason)) => {
      return vec![Effect::Notify(Notice::InstallLinkUnreachable(reason))];
    }
  };
  let games = games_listing(state, &communities);
  let only = match games.as_slice() {
    [] => {
      return vec![Effect::Notify(Notice::InstallLinkNeedsGame {
        package: link.full_name(),
        communities,
      })];
    }
    [game] => Some(game.clone()),
    _ => None,
  };

  state.install_link = Some(Linking::Choosing(Choice {
    link,
    games,
    target: None,
  }));

  only.map_or_else(Vec::new, |game| game_chosen(state, game))
}

/// The set-up games, by key, whose community is one of `communities` and
/// whose mods Rivet installs, sorted by name.
fn games_listing(state: &State, communities: &[String]) -> Vec<String> {
  let mut games: Vec<(&str, &str)> = state
    .settings
    .games
    .keys()
    .filter_map(|key| {
      let definition = state.ecosystem.game(key)?;
      let community = definition.community()?;
      let listed = communities.iter().any(|listed| listed == community);

      (listed && picker::installs_mods(definition))
        .then_some((definition.display_name(), key.as_str()))
    })
    .collect();

  games.sort();

  games.into_iter().map(|(_, key)| key.to_string()).collect()
}

/// Chooses `game`, one the link's package can go into, starting from the
/// profile it last opened, and lists its profiles unless they are known.
pub(crate) fn game_chosen(state: &mut State, game: String) -> Vec<Effect> {
  let profile = state.settings.profile(&game).to_string();
  let listed = state
    .switcher
    .as_ref()
    .filter(|switcher| switcher.game == game && !switcher.names.is_empty())
    .map(|switcher| switcher.names.clone());
  let Some(Linking::Choosing(choice)) = state.install_link.as_mut() else {
    return Vec::new();
  };

  if !choice.games.contains(&game) {
    return Vec::new();
  }

  let effects = match listed {
    Some(_) => Vec::new(),
    None => vec![Effect::Run(Command::ListProfiles(game.clone()))],
  };

  choice.target = Some(Target {
    profiles: listed.unwrap_or_else(|| vec![profile.clone()]),
    game,
    profile,
  });

  effects
}

/// Takes `game`'s profiles, as listed, as those the link's package can go
/// into, when `game` is the one chosen.
pub(crate) fn profiles_listed(state: &mut State, game: &str, names: &[String]) {
  let Some(Linking::Choosing(Choice {
    target: Some(target),
    ..
  })) = state.install_link.as_mut()
  else {
    return;
  };

  if target.game != game {
    return;
  }

  let mut profiles = names.to_vec();

  if !profiles.contains(&target.profile) {
    profiles.push(target.profile.clone());
  }

  profiles.sort();

  target.profiles = profiles;
}

/// Chooses `profile`, one of the chosen game's listed profiles.
pub(crate) fn profile_chosen(state: &mut State, profile: String) -> Vec<Effect> {
  if let Some(Linking::Choosing(Choice {
    target: Some(target),
    ..
  })) = state.install_link.as_mut()
    && target.profiles.contains(&profile)
  {
    target.profile = profile;
  }

  Vec::new()
}

/// Opens the chosen game and profile, unless they are open, to install the
/// link's package there once they load. Nothing while Rivet restarts to
/// update.
pub(crate) fn confirmed(state: &mut State) -> Vec<Effect> {
  let Some(Linking::Choosing(Choice {
    link,
    target: Some(target),
    ..
  })) = state.install_link.clone()
  else {
    return Vec::new();
  };

  if state.is_restarting() {
    return Vec::new();
  }

  state.install_link = Some(Linking::Installing {
    link,
    game: target.game.clone(),
    profile: target.profile.clone(),
  });

  crate::show_profile(state, &target.game, &target.profile)
}

/// Queues the confirmed install once its game's profile and package list
/// have loaded, or tells why it can't go ahead, as when the user left that
/// profile first. Drops it without a word while Rivet restarts to update.
pub(crate) fn install_when_ready(state: &mut State) -> Vec<Effect> {
  let Some(Linking::Installing {
    link,
    game,
    profile,
  }) = state.install_link.clone()
  else {
    return Vec::new();
  };

  match readiness(state, &link, &game, &profile) {
    Readiness::Waiting => Vec::new(),
    Readiness::Dropped => {
      state.install_link = None;

      Vec::new()
    }
    Readiness::Refused(notice) => {
      state.install_link = None;

      vec![Effect::Notify(notice)]
    }
    Readiness::Ready(chosen) => {
      state.install_link = None;

      install::queue_install(state, link.full_name(), chosen)
    }
  }
}

/// Whether a confirmed install can be queued.
enum Readiness {
  /// Something it needs is still loading or busy.
  Waiting,
  /// Rivet is restarting to update.
  Dropped,
  /// It cannot go ahead, for the reason to tell.
  Refused(Notice),
  /// It can, at this version, pinned, or at the latest when `None`.
  Ready(Option<String>),
}

fn readiness(state: &State, link: &InstallLink, game: &str, profile: &str) -> Readiness {
  if state.is_restarting() {
    return Readiness::Dropped;
  }

  let full_name = link.full_name();
  let left = || {
    Readiness::Refused(Notice::InstallLinkDropped {
      package: full_name.clone(),
      profile: profile.to_string(),
    })
  };
  let Some(open) = state
    .profile
    .as_ref()
    .filter(|open| open.game == game && open.name == profile)
  else {
    return left();
  };
  let installed = match &open.status {
    ProfileStatus::Opening => return Readiness::Waiting,
    ProfileStatus::Failed(_) => return left(),
    ProfileStatus::Ready(installed) => installed,
  };
  let Some(browse) = state.browse.as_ref() else {
    return left();
  };
  let loading = matches!(browse.status, Status::ReadingCache | Status::Refreshing(_));
  let listed = browse
    .index
    .as_ref()
    .map(|index| (index, index.versions(&full_name)))
    .filter(|(_, versions)| versions.contains(&link.version));
  let missing = Notice::InstallLinkVersionMissing {
    package: full_name.clone(),
    version: link.version.clone(),
  };
  let Some((index, versions)) = listed else {
    if loading {
      return Readiness::Waiting;
    }

    return Readiness::Refused(missing);
  };

  if !state.can_add_mods() {
    return Readiness::Waiting;
  }

  let chosen = (versions.first() != Some(&link.version)).then(|| link.version.clone());
  let planned = install::plan(index, installed, &full_name, chosen.as_deref());

  match planned {
    Some(planned) if planned.can_confirm() => Readiness::Ready(chosen),
    Some(planned) => Readiness::Refused(Notice::InstallLinkIncomplete {
      package: full_name,
      missing: planned.missing,
    }),
    None => Readiness::Refused(missing),
  }
}

/// Asks Thunderstore which games list `link`'s package. Nothing while Rivet
/// restarts to update. A link opened while another is under way replaces it.
pub(crate) fn opened(state: &mut State, link: InstallLink) -> Vec<Effect> {
  if state.is_restarting() {
    return Vec::new();
  }

  state.install_link = Some(Linking::Finding(link.clone()));

  vec![Effect::Run(Command::FindListings(link))]
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::Screen;
  use crate::browse::Refresh;
  use crate::handoff::Request;
  use crate::install::{Action, DEFAULT_PROFILE};
  use crate::settings::{GameSettings, Settings};
  use crate::{Msg, test_ecosystem, test_installed, test_shared_index, update};

  fn link(owner: &str, name: &str, version: &str) -> Option<InstallLink> {
    Some(InstallLink {
      owner: owner.to_string(),
      name: name.to_string(),
      version: version.to_string(),
    })
  }

  #[test]
  fn thunderstores_links_name_the_package_version() {
    let cases = [
      (
        "ror2mm://v1/install/thunderstore.io/BepInEx/BepInExPack/5.4.2305/",
        link("BepInEx", "BepInExPack", "5.4.2305"),
      ),
      (
        "ror2mm://v1/install/thunderstore.io/tristanmcpherson/R2API/5.0.5/",
        link("tristanmcpherson", "R2API", "5.0.5"),
      ),
      (
        "ror2mm://v1/install/thunderstore.io/denikson/BepInExPack_Valheim/5.4.2360/",
        link("denikson", "BepInExPack_Valheim", "5.4.2360"),
      ),
      (
        "ror2mm://v1/install/thunderstore.io/denikson/BepInExPack_Valheim/5.4.2360",
        link("denikson", "BepInExPack_Valheim", "5.4.2360"),
      ),
      (
        "ror2mm://v1/install/valheim.thunderstore.io/denikson/BepInExPack_Valheim/5.4.2360/",
        link("denikson", "BepInExPack_Valheim", "5.4.2360"),
      ),
    ];

    for (text, parsed) in cases {
      assert_eq!(InstallLink::parse(text), parsed, "{text}");
    }
  }

  #[test]
  fn anything_else_is_not_an_install_link() {
    let cases = [
      "ror2mm://v1/install/thunderstore.io/BepInEx/BepInExPack/",
      "ror2mm://v1/install/thunderstore.io/BepInEx/BepInExPack/5.4/",
      "ror2mm://v1/install/thunderstore.io/BepInEx/BepInExPack/5.4.2305/extra/",
      "ror2mm://v1/install/example.com/BepInEx/BepInExPack/5.4.2305/",
      "ror2mm://v1/install/evilthunderstore.io/BepInEx/BepInExPack/5.4.2305/",
      "ror2mm://v2/install/thunderstore.io/BepInEx/BepInExPack/5.4.2305/",
      "ror2mm://v1/install/thunderstore.io/Bep-InEx/BepInExPack/5.4.2305/",
      "ror2mm://v1/install/thunderstore.io/_BepInEx/BepInExPack/5.4.2305/",
      "ror2mm://v1/install/thunderstore.io/BepInEx/Bep%20InEx/5.4.2305/",
      "ror2mm://v1/install/thunderstore.io/BepInEx/BepInExPack/5.4.2305//",
      "https://thunderstore.io/c/valheim/p/BepInEx/BepInExPack/",
      "ror2mm://",
    ];

    for text in cases {
      assert_eq!(InstallLink::parse(text), None, "{text}");
    }
  }

  fn package_link() -> InstallLink {
    InstallLink::parse("ror2mm://v1/install/thunderstore.io/Owner/ModC/1.0.0/").unwrap()
  }

  fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
  }

  /// A state with each of `games` set up, the first open with its package
  /// list and Default profile loaded, and `package_link` handed over.
  fn handed_over(games: &[&str]) -> State {
    let mut state = State::new(test_ecosystem());
    let settings = Settings {
      last_game: games.first().map(|game| game.to_string()),
      games: games
        .iter()
        .map(|game| {
          (
            game.to_string(),
            GameSettings::new(format!("/games/{game}")),
          )
        })
        .collect(),
      ..Settings::default()
    };
    let open = games[0];

    update(&mut state, Msg::Started);
    update(&mut state, Msg::SettingsLoaded(Ok(settings)));
    update(
      &mut state,
      Msg::CachedIndexRead {
        community: open.to_string(),
        index: Ok(Some(test_shared_index())),
      },
    );
    update(
      &mut state,
      Msg::ProfileOpened {
        game: open.to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        installed: Ok(test_installed(&[])),
      },
    );
    update(
      &mut state,
      Msg::ProfilesListed {
        game: open.to_string(),
        names: Ok(names(&[DEFAULT_PROFILE, "Modded"])),
      },
    );
    update(
      &mut state,
      Msg::HandedOff(Request {
        open: Some("ror2mm://v1/install/thunderstore.io/Owner/ModC/1.0.0/".to_string()),
        ..Request::default()
      }),
    );

    state
  }

  fn found(state: &mut State, communities: Result<&[&str], ListingFailure>) -> Vec<Effect> {
    update(
      state,
      Msg::ListingsFound {
        link: package_link(),
        communities: communities.map(names),
      },
    )
  }

  fn choice(state: &State) -> Option<Choice> {
    match &state.install_link {
      Some(Linking::Choosing(choice)) => Some(choice.clone()),
      _ => None,
    }
  }

  #[test]
  fn a_package_one_set_up_game_lists_asks_for_a_profile_of_that_game() {
    let mut state = handed_over(&["valheim", "riskofrain2"]);

    let effects = found(&mut state, Ok(&["valheim", "lethal-company"]));

    assert_eq!(effects, vec![]);
    assert_eq!(
      choice(&state),
      Some(Choice {
        link: package_link(),
        games: names(&["valheim"]),
        target: Some(Target {
          game: "valheim".to_string(),
          profiles: names(&[DEFAULT_PROFILE, "Modded"]),
          profile: DEFAULT_PROFILE.to_string(),
        }),
      })
    );
  }

  #[test]
  fn a_game_not_open_has_its_profiles_listed_starting_from_its_last_opened() {
    let mut state = handed_over(&["valheim", "riskofrain2"]);

    state.settings.games.get_mut("riskofrain2").unwrap().profile = Some("Speedrun".to_string());

    let effects = found(&mut state, Ok(&["riskofrain2"]));

    assert_eq!(
      effects,
      vec![Effect::Run(Command::ListProfiles(
        "riskofrain2".to_string()
      ))]
    );
    assert_eq!(
      choice(&state).and_then(|choice| choice.target),
      Some(Target {
        game: "riskofrain2".to_string(),
        profiles: names(&["Speedrun"]),
        profile: "Speedrun".to_string(),
      })
    );

    update(
      &mut state,
      Msg::ProfilesListed {
        game: "riskofrain2".to_string(),
        names: Ok(names(&["Speedrun", DEFAULT_PROFILE])),
      },
    );

    assert_eq!(
      choice(&state).and_then(|choice| choice.target),
      Some(Target {
        game: "riskofrain2".to_string(),
        profiles: names(&[DEFAULT_PROFILE, "Speedrun"]),
        profile: "Speedrun".to_string(),
      })
    );
  }

  #[test]
  fn a_package_several_set_up_games_list_asks_which_game_first() {
    let mut state = handed_over(&["valheim", "riskofrain2", "lethal-company"]);

    let effects = found(
      &mut state,
      Ok(&["valheim", "riskofrain2", "lethal-company"]),
    );

    assert_eq!(effects, vec![]);
    assert_eq!(
      choice(&state),
      Some(Choice {
        link: package_link(),
        games: names(&["lethal-company", "riskofrain2", "valheim"]),
        target: None,
      })
    );
  }

  #[test]
  fn a_game_rivet_does_not_install_mods_for_is_never_offered() {
    let mut state = handed_over(&["valheim", "balatro"]);

    found(&mut state, Ok(&["valheim", "balatro"]));

    assert_eq!(
      choice(&state).map(|choice| choice.games),
      Some(names(&["valheim"]))
    );
  }

  #[test]
  fn a_package_no_set_up_game_lists_names_the_games_it_is_for() {
    let mut state = handed_over(&["valheim"]);

    let effects = found(&mut state, Ok(&["lethal-company"]));

    assert_eq!(
      effects,
      vec![Effect::Notify(Notice::InstallLinkNeedsGame {
        package: "Owner-ModC".to_string(),
        communities: names(&["lethal-company"]),
      })]
    );
    assert_eq!(state.install_link, None);
  }

  #[test]
  fn a_package_thunderstore_does_not_have_is_named_in_a_notice() {
    let mut state = handed_over(&["valheim"]);

    let effects = found(&mut state, Err(ListingFailure::Unknown));

    assert_eq!(
      effects,
      vec![Effect::Notify(Notice::InstallLinkUnknown(
        "Owner-ModC".to_string()
      ))]
    );
    assert_eq!(state.install_link, None);
  }

  #[test]
  fn thunderstore_unreachable_says_so() {
    let mut state = handed_over(&["valheim"]);

    let effects = found(
      &mut state,
      Err(ListingFailure::Unreachable(
        "connection refused".to_string(),
      )),
    );

    assert_eq!(
      effects,
      vec![Effect::Notify(Notice::InstallLinkUnreachable(
        "connection refused".to_string()
      ))]
    );
    assert_eq!(state.install_link, None);
  }

  #[test]
  fn an_answer_for_a_link_no_longer_waited_on_is_ignored() {
    let mut state = handed_over(&["valheim"]);
    let other = InstallLink {
      version: "2.0.0".to_string(),
      ..package_link()
    };

    let effects = update(
      &mut state,
      Msg::ListingsFound {
        link: other,
        communities: Ok(names(&["valheim"])),
      },
    );

    assert_eq!(effects, vec![]);
    assert_eq!(state.install_link, Some(Linking::Finding(package_link())));
  }

  #[test]
  fn choosing_one_of_several_games_lists_its_profiles() {
    let mut state = handed_over(&["valheim", "riskofrain2"]);

    found(&mut state, Ok(&["valheim", "riskofrain2"]));

    assert_eq!(
      update(
        &mut state,
        Msg::LinkGameChosen("lethal-company".to_string())
      ),
      vec![],
      "a game that does not list the package was chosen"
    );
    assert_eq!(choice(&state).and_then(|choice| choice.target), None);
    assert_eq!(
      update(&mut state, Msg::LinkGameChosen("riskofrain2".to_string())),
      vec![Effect::Run(Command::ListProfiles(
        "riskofrain2".to_string()
      ))]
    );
    assert_eq!(
      choice(&state)
        .and_then(|choice| choice.target)
        .map(|target| target.game),
      Some("riskofrain2".to_string())
    );
  }

  #[test]
  fn a_listed_profile_can_be_chosen_and_another_cannot() {
    let mut state = handed_over(&["valheim"]);

    found(&mut state, Ok(&["valheim"]));
    update(&mut state, Msg::LinkProfileChosen("Elsewhere".to_string()));

    let profile =
      |state: &State| choice(state).and_then(|choice| choice.target.map(|target| target.profile));

    assert_eq!(profile(&state), Some(DEFAULT_PROFILE.to_string()));

    update(&mut state, Msg::LinkProfileChosen("Modded".to_string()));

    assert_eq!(profile(&state), Some("Modded".to_string()));
  }

  #[test]
  fn dismissing_the_choice_installs_nothing() {
    let mut state = handed_over(&["valheim"]);

    found(&mut state, Ok(&["valheim"]));

    assert_eq!(update(&mut state, Msg::LinkInstallDismissed), vec![]);
    assert_eq!(state.install_link, None);
    assert!(state.queue.operations.is_empty());
  }

  /// The game, profile, package, and version of each install in `effects`.
  fn installs(effects: &[Effect]) -> Vec<(String, String, String, Option<String>)> {
    effects
      .iter()
      .filter_map(|effect| match effect {
        Effect::Run(Command::Install(install)) => Some((
          install.game.clone(),
          install.profile.clone(),
          install.package.clone(),
          install.version.clone(),
        )),
        _ => None,
      })
      .collect()
  }

  fn install(
    game: &str,
    profile: &str,
    version: Option<&str>,
  ) -> (String, String, String, Option<String>) {
    (
      game.to_string(),
      profile.to_string(),
      "Owner-ModC".to_string(),
      version.map(str::to_string),
    )
  }

  /// [`handed_over`] with `link` handed over instead.
  fn handed_over_link(games: &[&str], link: &str) -> State {
    let mut state = handed_over(games);

    update(
      &mut state,
      Msg::HandedOff(Request {
        open: Some(link.to_string()),
        ..Request::default()
      }),
    );

    state
  }

  #[test]
  fn confirming_the_newest_version_installs_it_unpinned_into_the_open_profile() {
    let link = "ror2mm://v1/install/thunderstore.io/Owner/ModC/2.0.0/";
    let mut state = handed_over_link(&["valheim"], link);

    update(
      &mut state,
      Msg::ListingsFound {
        link: InstallLink::parse(link).unwrap(),
        communities: Ok(names(&["valheim"])),
      },
    );

    let effects = update(&mut state, Msg::LinkInstallConfirmed);

    assert_eq!(
      installs(&effects),
      vec![install("valheim", DEFAULT_PROFILE, None)]
    );
    assert_eq!(
      state
        .queue
        .operations
        .last()
        .map(|operation| operation.action.clone()),
      Some(Action::Install("Owner-ModC".to_string()))
    );
    assert_eq!(state.install_link, None);
  }

  #[test]
  fn confirming_an_older_version_installs_and_pins_exactly_it() {
    let mut state = handed_over(&["valheim"]);

    found(&mut state, Ok(&["valheim"]));

    let effects = update(&mut state, Msg::LinkInstallConfirmed);

    assert_eq!(
      installs(&effects),
      vec![install("valheim", DEFAULT_PROFILE, Some("1.0.0"))]
    );
  }

  #[test]
  fn confirming_another_profile_of_the_open_game_switches_to_it_first() {
    let mut state = handed_over(&["valheim"]);

    found(&mut state, Ok(&["valheim"]));
    update(&mut state, Msg::LinkProfileChosen("Modded".to_string()));

    let effects = update(&mut state, Msg::LinkInstallConfirmed);

    assert!(
      effects.contains(&Effect::Run(Command::OpenProfile {
        game: "valheim".to_string(),
        profile: "Modded".to_string(),
      })),
      "{effects:?}"
    );
    assert_eq!(installs(&effects), vec![]);

    let effects = update(
      &mut state,
      Msg::ProfileOpened {
        game: "valheim".to_string(),
        profile: "Modded".to_string(),
        installed: Ok(test_installed(&[])),
      },
    );

    assert_eq!(
      installs(&effects),
      vec![install("valheim", "Modded", Some("1.0.0"))]
    );
  }

  #[test]
  fn confirming_another_game_opens_it_and_installs_once_its_list_and_profile_load() {
    let mut state = handed_over(&["valheim", "riskofrain2"]);

    found(&mut state, Ok(&["riskofrain2"]));

    let effects = update(&mut state, Msg::LinkInstallConfirmed);

    assert_eq!(state.screen, Screen::Game("riskofrain2".to_string()));
    assert!(
      effects.contains(&Effect::Run(Command::ReadCachedIndex(
        "riskofrain2".to_string()
      ))),
      "{effects:?}"
    );
    assert_eq!(state.settings.last_game.as_deref(), Some("riskofrain2"));

    let mut effects = update(
      &mut state,
      Msg::CachedIndexRead {
        community: "riskofrain2".to_string(),
        index: Ok(Some(test_shared_index())),
      },
    );

    effects.extend(update(
      &mut state,
      Msg::ProfileOpened {
        game: "riskofrain2".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        installed: Ok(test_installed(&[])),
      },
    ));

    assert_eq!(
      installs(&effects),
      vec![install("riskofrain2", DEFAULT_PROFILE, Some("1.0.0"))]
    );
  }

  #[test]
  fn a_version_the_cached_list_lacks_waits_for_the_refresh_then_says_it_is_missing() {
    let link = "ror2mm://v1/install/thunderstore.io/Owner/ModC/3.0.0/";
    let mut state = handed_over(&["valheim", "riskofrain2"]);

    update(
      &mut state,
      Msg::HandedOff(Request {
        open: Some(link.to_string()),
        ..Request::default()
      }),
    );
    update(
      &mut state,
      Msg::ListingsFound {
        link: InstallLink::parse(link).unwrap(),
        communities: Ok(names(&["riskofrain2"])),
      },
    );
    update(&mut state, Msg::LinkInstallConfirmed);

    let mut effects = update(
      &mut state,
      Msg::CachedIndexRead {
        community: "riskofrain2".to_string(),
        index: Ok(Some(test_shared_index())),
      },
    );

    effects.extend(update(
      &mut state,
      Msg::ProfileOpened {
        game: "riskofrain2".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        installed: Ok(test_installed(&[])),
      },
    ));

    assert_eq!(installs(&effects), vec![]);
    assert!(
      !effects
        .iter()
        .any(|effect| matches!(effect, Effect::Notify(_)))
    );

    let Some(Status::Refreshing(id)) = state.browse.as_ref().map(|browse| browse.status.clone())
    else {
      panic!("the list is not being refreshed");
    };
    let effects = update(
      &mut state,
      Msg::IndexRefreshed {
        refresh: Refresh {
          community: "riskofrain2".to_string(),
          id,
        },
        index: Ok(None),
      },
    );

    assert_eq!(
      effects,
      vec![Effect::Notify(Notice::InstallLinkVersionMissing {
        package: "Owner-ModC".to_string(),
        version: "3.0.0".to_string(),
      })]
    );
    assert_eq!(state.install_link, None);
  }

  #[test]
  fn confirming_while_rivet_restarts_to_update_queues_nothing() {
    let mut state = handed_over(&["valheim"]);

    found(&mut state, Ok(&["valheim"]));

    state.self_update.stage = crate::self_update::Stage::Restarting {
      version: "0.3.0".to_string(),
      file: std::path::PathBuf::from("/opt/rivet/rivet.new"),
      replacing: true,
    };

    let effects = update(&mut state, Msg::LinkInstallConfirmed);

    assert_eq!(installs(&effects), vec![]);
    assert!(state.queue.operations.is_empty());
  }

  #[test]
  fn leaving_the_game_before_it_loads_drops_the_install_and_says_so() {
    let mut state = handed_over(&["valheim", "riskofrain2"]);

    found(&mut state, Ok(&["riskofrain2"]));
    update(&mut state, Msg::LinkInstallConfirmed);

    let effects = update(&mut state, Msg::SwitchGamePressed);

    assert!(
      effects.contains(&Effect::Notify(Notice::InstallLinkDropped {
        package: "Owner-ModC".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
      })),
      "{effects:?}"
    );
    assert_eq!(state.install_link, None);
  }

  #[test]
  fn restarting_to_update_before_the_game_loads_drops_the_install_quietly() {
    let mut state = handed_over(&["valheim", "riskofrain2"]);

    found(&mut state, Ok(&["riskofrain2"]));
    update(&mut state, Msg::LinkInstallConfirmed);

    state.self_update.stage = crate::self_update::Stage::Restarting {
      version: "0.3.0".to_string(),
      file: std::path::PathBuf::from("/opt/rivet/rivet.new"),
      replacing: true,
    };

    let effects = update(
      &mut state,
      Msg::CachedIndexRead {
        community: "riskofrain2".to_string(),
        index: Ok(Some(test_shared_index())),
      },
    );

    assert!(
      !effects
        .iter()
        .any(|effect| matches!(effect, Effect::Notify(_))),
      "{effects:?}"
    );
    assert_eq!(state.install_link, None);
  }

  fn loaded(install_links: Option<bool>) -> (State, Vec<Effect>) {
    let mut state = State::new(test_ecosystem());
    let settings = Settings {
      install_links,
      ..Settings::default()
    };

    update(&mut state, Msg::Started);

    let effects = update(&mut state, Msg::SettingsLoaded(Ok(settings)));

    (state, effects)
  }

  fn register(claim: Claim) -> Effect {
    Effect::Run(Command::RegisterInstallLinks(claim))
  }

  #[test]
  fn on_start_rivet_claims_install_links_as_the_user_chose_or_else_when_unclaimed() {
    let cases = [
      (None, Claim::IfUnclaimed),
      (Some(true), Claim::Always),
      (Some(false), Claim::Release),
    ];

    for (chosen, claim) in cases {
      let (_, effects) = loaded(chosen);

      assert!(
        effects.contains(&register(claim)),
        "{chosen:?}: {effects:?}"
      );
    }
  }

  #[test]
  fn whether_rivet_opens_install_links_is_known_once_registered() {
    let (mut state, _) = loaded(None);

    assert_eq!(state.install_links, None);

    update(&mut state, Msg::InstallLinksRegistered(Ok(false)));

    assert_eq!(state.install_links, Some(false));

    update(
      &mut state,
      Msg::InstallLinksRegistered(Err("access denied".to_string())),
    );

    assert_eq!(state.install_links, None);
  }

  #[test]
  fn turning_install_links_on_or_off_saves_the_choice_and_claims_or_releases_them() {
    let (mut state, _) = loaded(None);

    let effects = update(&mut state, Msg::InstallLinksChosen(true));

    assert_eq!(state.settings.install_links, Some(true));
    assert_eq!(
      effects,
      vec![
        Effect::Run(Command::SaveSettings(state.settings.clone())),
        register(Claim::Always),
      ]
    );

    let effects = update(&mut state, Msg::InstallLinksChosen(false));

    assert_eq!(state.settings.install_links, Some(false));
    assert_eq!(
      effects,
      vec![
        Effect::Run(Command::SaveSettings(state.settings.clone())),
        register(Claim::Release),
      ]
    );
  }
}
