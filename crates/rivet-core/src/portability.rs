//! Exporting a profile as a code to share or a file to keep, and importing
//! one as a new profile, in the formats r2modman shares profiles in.

use crate::browse::Index;
use crate::install::Action;
use crate::notice::Notice;
use crate::queue::{self, Work};
use crate::switcher::{NameRefusal, check_name};
use crate::{Command, Effect, State};
use std::path::PathBuf;
use thunderstore_engine::models::PackageIndex;

/// Where an export goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Destination {
  /// Thunderstore, which answers with a code anyone can import it by.
  Code,
  /// A `.r2z` file at this path.
  File(PathBuf),
}

/// Export a game's profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Export {
  /// The game, by its key in the game definitions.
  pub game: String,
  /// The Thunderstore community a code is shared through.
  pub community: String,
  pub profile: String,
  pub to: Destination,
}

/// Where an export went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exported {
  /// The code Thunderstore gave it.
  Code(String),
  /// The `.r2z` file written.
  File(PathBuf),
}

/// Exporting the open profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Exporting {
  /// Waiting for the user to choose where to save the file.
  ChoosingFile,
  Running(Export),
}

/// Where a profile to import comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
  /// A code a profile was shared by on Thunderstore.
  Code(String),
  /// A `.r2z` export, or the bare `export.r2x` list inside one.
  File(PathBuf),
  /// A profile folder in r2modman's data, which is only read.
  R2modman(PathBuf),
}

/// One package a shared profile lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedPackage {
  /// The package, as `Owner-Name`.
  pub full_name: String,
  /// The exact version to install.
  pub version: String,
  pub enabled: bool,
}

/// A shared profile, read and ready to import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shared {
  /// The name it was exported under.
  pub name: String,
  pub packages: Vec<SharedPackage>,
  /// The export as read, which carries the profile's config files.
  pub archive: Vec<u8>,
}

/// Why a shared profile could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadFailure {
  /// Thunderstore has no profile under the code. It may be mistyped or
  /// expired.
  UnknownCode,
  /// What was read is not a profile export, with the reason.
  Damaged(String),
  /// Thunderstore could not be reached, with the reason.
  Unreachable(String),
  /// The file could not be read, with the reason.
  Unreadable(String),
}

/// Importing a shared profile into the open game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Importing {
  /// Asking for a code, with why the last one given could not be read.
  AskingCode(Option<ReadFailure>),
  /// Waiting for the user to choose a file.
  ChoosingFile,
  /// Reading what is shared.
  Reading(Source),
  /// Asking the name of the profile to import it as.
  Naming(ImportForm),
}

/// What an import will bring, and the name being asked for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportForm {
  pub shared: Shared,
  /// The packages, as `Owner-Name-Version`, that the package index does not
  /// have, which the import leaves out.
  pub missing: Vec<String>,
  /// Why the last name given was refused.
  pub refusal: Option<NameRefusal>,
}

/// Import a shared profile into a game as a new profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
  /// The [`crate::install::Operation`] id its progress and result carry.
  pub operation: u64,
  /// The game, by its key in the game definitions.
  pub game: String,
  /// The Thunderstore community the packages come from.
  pub community: String,
  /// The name of the profile to create.
  pub profile: String,
  /// The packages to install, at these versions and enabled states.
  pub packages: Vec<SharedPackage>,
  /// The export as read, whose config files are restored once every package
  /// is installed.
  pub archive: Vec<u8>,
  /// The index the missing packages were found with, so the import brings
  /// what the user saw.
  pub index: Index,
}

impl State {
  /// Whether a profile can be imported into the open game: its package list
  /// is loaded, nothing else is being imported, and no change to its
  /// profiles is being made.
  pub fn can_import_profile(&self) -> bool {
    let listed = self
      .browse
      .as_ref()
      .is_some_and(|browse| browse.index.is_some());

    listed && self.importing.is_none() && self.can_switch_profile()
  }

  /// Whether an export is reading the open profile's folder, so the profile
  /// must not change until it ends.
  pub fn is_profile_exporting(&self) -> bool {
    let Some(profile) = self.profile.as_ref() else {
      return false;
    };

    matches!(
      &self.exporting,
      Some(Exporting::Running(export))
        if export.game == profile.game && export.profile == profile.name
    )
  }

  /// Whether the open profile can be exported: it has been read, nothing
  /// else is exporting, and as for [`State::can_duplicate_profile`].
  pub fn can_export_profile(&self) -> bool {
    let read = self
      .profile
      .as_ref()
      .and_then(crate::install::Profile::installed)
      .is_some();

    read && self.exporting.is_none() && self.can_duplicate_profile()
  }
}

/// Exports the open profile as a code, unless
/// [`State::can_export_profile`] says it cannot.
pub(crate) fn export_code_pressed(state: &mut State) -> Vec<Effect> {
  if !state.can_export_profile() {
    return Vec::new();
  }

  start_export(state, Destination::Code)
}

/// Asks where to save the open profile's export, unless
/// [`State::can_export_profile`] says it cannot be exported.
pub(crate) fn export_file_pressed(state: &mut State) -> Vec<Effect> {
  if !state.can_export_profile() {
    return Vec::new();
  }

  let Some(profile) = state.profile.as_ref() else {
    return Vec::new();
  };
  let suggested = format!("{}.r2z", profile.name);

  state.exporting = Some(Exporting::ChoosingFile);

  vec![Effect::ChooseExportFile(suggested)]
}

/// Exports the open profile to `path`, or nothing when the dialog was
/// cancelled.
pub(crate) fn export_file_chosen(state: &mut State, path: Option<PathBuf>) -> Vec<Effect> {
  if state.exporting != Some(Exporting::ChoosingFile) {
    return Vec::new();
  }

  state.exporting = None;

  match path {
    Some(path) if state.can_export_profile() => start_export(state, Destination::File(path)),
    _ => Vec::new(),
  }
}

/// Asks for a profile code to import, unless
/// [`State::can_import_profile`] says nothing can be imported.
pub(crate) fn import_code_pressed(state: &mut State) -> Vec<Effect> {
  if state.can_import_profile() {
    state.importing = Some(Importing::AskingCode(None));
  }

  Vec::new()
}

/// Reads what `code` shares, ignoring a blank one.
pub(crate) fn import_code_submitted(state: &mut State, code: String) -> Vec<Effect> {
  let code = code.trim();

  if !matches!(state.importing, Some(Importing::AskingCode(_))) || code.is_empty() {
    return Vec::new();
  }

  read(state, Source::Code(code.to_string()))
}

/// Asks for a file to import, unless [`State::can_import_profile`] says
/// nothing can be imported.
pub(crate) fn import_file_pressed(state: &mut State) -> Vec<Effect> {
  if !state.can_import_profile() {
    return Vec::new();
  }

  state.importing = Some(Importing::ChoosingFile);

  vec![Effect::ChooseImportFile]
}

/// Reads the profile `path` shares, or nothing when the dialog was
/// cancelled.
pub(crate) fn import_file_chosen(state: &mut State, path: Option<PathBuf>) -> Vec<Effect> {
  if state.importing != Some(Importing::ChoosingFile) {
    return Vec::new();
  }

  match path {
    Some(path) => read(state, Source::File(path)),
    None => {
      state.importing = None;

      Vec::new()
    }
  }
}

/// Asks the name to import what `source` shares as, offering its own name or
/// the first free one after it. A code that could not be read is asked for
/// again, and a file ends the import, each saying why.
pub(crate) fn shared_read(
  state: &mut State,
  source: Source,
  shared: Result<Shared, ReadFailure>,
) -> Vec<Effect> {
  if state.importing != Some(Importing::Reading(source.clone())) {
    return Vec::new();
  }

  let shared = match shared {
    Ok(shared) => shared,
    Err(failure) => {
      return match source {
        Source::Code(_) => {
          state.importing = Some(Importing::AskingCode(Some(failure)));

          Vec::new()
        }
        Source::File(_) | Source::R2modman(_) => {
          state.importing = None;

          vec![Effect::Notify(Notice::ImportUnread(failure))]
        }
      };
    }
  };
  let missing = state
    .browse
    .as_ref()
    .and_then(|browse| browse.index.as_ref())
    .map(|index| missing(&index.0, &shared.packages))
    .unwrap_or_default();
  let name = free_name(&state.taken_profile_names(), &shared.name);

  state.importing = Some(Importing::Naming(ImportForm {
    shared,
    missing,
    refusal: None,
  }));

  vec![Effect::FillProfileName(name)]
}

/// Queues importing what was read as the profile `name`, or refuses `name`
/// with the reason and imports nothing.
pub(crate) fn import_name_submitted(state: &mut State, name: String) -> Vec<Effect> {
  let Some(Importing::Naming(form)) = state.importing.as_ref() else {
    return Vec::new();
  };

  if let Err(refusal) = check_name(&state.taken_profile_names(), &name, None) {
    if let Some(Importing::Naming(form)) = state.importing.as_mut() {
      form.refusal = Some(refusal);
    }

    return Vec::new();
  }

  let ImportForm {
    shared, missing, ..
  } = form.clone();

  state.importing = None;

  queue_import(state, name, shared, &missing)
}

/// Queues importing `shared` into the open game as the profile `name`,
/// leaving out the `missing` packages, as `Owner-Name-Version`.
pub(crate) fn queue_import(
  state: &mut State,
  name: String,
  shared: Shared,
  missing: &[String],
) -> Vec<Effect> {
  let Some(switcher) = state.switcher.as_ref() else {
    return Vec::new();
  };
  let Some(browse) = state.browse.as_ref() else {
    return Vec::new();
  };
  let Some(index) = browse.index.clone() else {
    return Vec::new();
  };
  let (game, community) = (switcher.game.clone(), browse.community.clone());
  let Shared {
    packages, archive, ..
  } = shared;
  let packages: Vec<SharedPackage> = packages
    .into_iter()
    .filter(|package| {
      let listed = format!("{}-{}", package.full_name, package.version);

      !missing.contains(&listed)
    })
    .collect();
  let action = Action::Import(
    packages
      .iter()
      .map(|package| package.full_name.clone())
      .collect(),
  );

  queue::enqueue(state, game.clone(), name.clone(), action, |operation| {
    Work::Import(Import {
      operation,
      game,
      community,
      profile: name,
      packages,
      archive,
      index,
    })
  })
}

/// The packages of `packages` at a version `index` does not have, as
/// `Owner-Name-Version`.
pub(crate) fn missing(index: &PackageIndex, packages: &[SharedPackage]) -> Vec<String> {
  packages
    .iter()
    .filter(|package| {
      index
        .get_package_by_full_name(&package.full_name)
        .is_none_or(|found| {
          !found
            .versions
            .iter()
            .any(|version| version.version_number.as_deref() == Some(package.version.as_str()))
        })
    })
    .map(|package| format!("{}-{}", package.full_name, package.version))
    .collect()
}

/// `name` when no profile in `taken` has it, and otherwise the first of
/// `name 2`, `name 3`, and on that is free.
pub(crate) fn free_name(taken: &[String], name: &str) -> String {
  let is_free = |candidate: &String| {
    !matches!(
      check_name(taken, candidate, None),
      Err(NameRefusal::Taken(_))
    )
  };

  std::iter::once(name.to_string())
    .chain((2..).map(|number| format!("{name} {number}")))
    .find(is_free)
    .unwrap_or_else(|| name.to_string())
}

/// Reads what `source` shares, through the open game's community.
fn read(state: &mut State, source: Source) -> Vec<Effect> {
  let Some(browse) = state.browse.as_ref() else {
    return Vec::new();
  };
  let community = browse.community.clone();

  state.importing = Some(Importing::Reading(source.clone()));

  vec![Effect::Run(Command::ReadShared { community, source })]
}

/// Exports the open profile to `to`.
fn start_export(state: &mut State, to: Destination) -> Vec<Effect> {
  let Some(profile) = state.profile.as_ref() else {
    return Vec::new();
  };
  let Some(browse) = state.browse.as_ref() else {
    return Vec::new();
  };
  let export = Export {
    game: profile.game.clone(),
    community: browse.community.clone(),
    profile: profile.name.clone(),
    to,
  };

  state.exporting = Some(Exporting::Running(export.clone()));

  vec![Effect::Run(Command::Export(export))]
}

/// Applies the end of `export`, telling the user where the profile went or
/// why it did not.
pub(crate) fn exported(
  state: &mut State,
  export: Export,
  result: Result<Exported, String>,
) -> Vec<Effect> {
  if state.exporting.as_ref() == Some(&Exporting::Running(export.clone())) {
    state.exporting = None;
  }

  let Export { profile, to, .. } = export;

  match result {
    Ok(exported) => {
      let copy = match &exported {
        Exported::Code(code) => Some(Effect::CopyToClipboard(code.clone())),
        Exported::File(_) => None,
      };

      copy
        .into_iter()
        .chain([Effect::Notify(Notice::ProfileExported {
          profile,
          exported,
        })])
        .collect()
    }
    Err(reason) => vec![Effect::Notify(Notice::ExportFailed {
      profile,
      to,
      reason,
    })],
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::install::{DEFAULT_PROFILE, FailedPackage, Outcome};
  use crate::settings::{GameSettings, Settings};
  use crate::{Command, Msg, test_ecosystem, test_installed, update};

  fn index() -> Index {
    crate::test_shared_index()
  }

  /// A state with valheim open on its Default profile, holding `Owner-ModA`,
  /// with the profiles `Default` and `Modded` listed and the package list
  /// loaded.
  fn ready() -> State {
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
      Msg::CachedIndexRead {
        community: "valheim".to_string(),
        index: Ok(Some(index())),
      },
    );
    update(
      &mut state,
      Msg::ProfileOpened {
        game: "valheim".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        installed: Ok(test_installed(&[("Owner-ModA", "1.0.0")])),
      },
    );
    update(
      &mut state,
      Msg::ProfilesListed {
        game: "valheim".to_string(),
        names: Ok(vec![DEFAULT_PROFILE.to_string(), "Modded".to_string()]),
      },
    );

    state
  }

  fn export(to: Destination) -> Export {
    Export {
      game: "valheim".to_string(),
      community: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      to,
    }
  }

  #[test]
  fn exporting_as_a_code_shares_the_open_profile_through_thunderstore() {
    let mut state = ready();

    let effects = update(&mut state, Msg::ExportCodePressed);

    assert_eq!(
      effects,
      vec![Effect::Run(Command::Export(export(Destination::Code)))]
    );
    assert!(!state.can_export_profile());
  }

  #[test]
  fn the_profile_being_exported_cannot_change_until_the_export_ends() {
    let mut state = ready();

    update(&mut state, Msg::ExportCodePressed);

    assert!(!state.can_install());
    assert!(!state.can_manage());
    assert_eq!(
      update(&mut state, Msg::InstallPressed("Owner-ModB".to_string())),
      vec![]
    );
    assert_eq!(
      update(&mut state, Msg::DisablePressed("Owner-ModA".to_string())),
      vec![]
    );
    assert_eq!(state.confirmation, None);
    assert!(state.queue.operations.is_empty());

    update(
      &mut state,
      exported(Destination::Code, Ok(Exported::Code("abc-123".to_string()))),
    );

    assert!(state.can_manage());
  }

  #[test]
  fn another_profile_can_change_while_one_is_exported() {
    let mut state = ready();

    update(&mut state, Msg::ExportCodePressed);
    update(&mut state, Msg::ProfilePicked("Modded".to_string()));
    update(
      &mut state,
      Msg::ProfileOpened {
        game: "valheim".to_string(),
        profile: "Modded".to_string(),
        installed: Ok(test_installed(&[("Owner-ModA", "1.0.0")])),
      },
    );

    assert!(state.can_manage());
  }

  fn exported(to: Destination, result: Result<Exported, &str>) -> Msg {
    Msg::ProfileExported {
      export: export(to),
      result: result.map_err(str::to_string),
    }
  }

  #[test]
  fn an_exported_code_is_copied_and_told() {
    let mut state = ready();

    update(&mut state, Msg::ExportCodePressed);

    let effects = update(
      &mut state,
      exported(Destination::Code, Ok(Exported::Code("abc-123".to_string()))),
    );

    assert_eq!(
      effects,
      vec![
        Effect::CopyToClipboard("abc-123".to_string()),
        Effect::Notify(Notice::ProfileExported {
          profile: DEFAULT_PROFILE.to_string(),
          exported: Exported::Code("abc-123".to_string()),
        }),
      ]
    );
    assert!(state.can_export_profile());
  }

  #[test]
  fn an_export_that_fails_says_why() {
    let mut state = ready();

    update(&mut state, Msg::ExportCodePressed);

    let effects = update(&mut state, exported(Destination::Code, Err("offline")));

    assert_eq!(
      effects,
      vec![Effect::Notify(Notice::ExportFailed {
        profile: DEFAULT_PROFILE.to_string(),
        to: Destination::Code,
        reason: "offline".to_string(),
      })]
    );
    assert_eq!(state.exporting, None);
  }

  #[test]
  fn exporting_as_a_file_asks_where_to_save_it_named_after_the_profile() {
    let mut state = ready();

    let effects = update(&mut state, Msg::ExportFilePressed);

    assert_eq!(
      effects,
      vec![Effect::ChooseExportFile("Default.r2z".to_string())]
    );

    let path = PathBuf::from("/home/me/Default.r2z");
    let effects = update(&mut state, Msg::ExportFileChosen(Some(path.clone())));

    assert_eq!(
      effects,
      vec![Effect::Run(Command::Export(export(Destination::File(
        path.clone()
      ))))]
    );

    let effects = update(
      &mut state,
      exported(
        Destination::File(path.clone()),
        Ok(Exported::File(path.clone())),
      ),
    );

    assert_eq!(
      effects,
      vec![Effect::Notify(Notice::ProfileExported {
        profile: DEFAULT_PROFILE.to_string(),
        exported: Exported::File(path),
      })]
    );
  }

  #[test]
  fn closing_the_save_dialog_exports_nothing() {
    let mut state = ready();

    update(&mut state, Msg::ExportFilePressed);

    assert_eq!(update(&mut state, Msg::ExportFileChosen(None)), vec![]);
    assert!(state.can_export_profile());
  }

  #[test]
  fn a_profile_that_is_changing_cannot_be_exported() {
    let mut state = ready();

    update(&mut state, Msg::DisablePressed("Owner-ModA".to_string()));

    assert!(!state.can_export_profile());
    assert_eq!(update(&mut state, Msg::ExportCodePressed), vec![]);
    assert_eq!(update(&mut state, Msg::ExportFilePressed), vec![]);
  }

  #[test]
  fn the_profile_being_exported_cannot_be_renamed_or_deleted() {
    let mut state = ready();

    update(&mut state, Msg::ProfilePicked("Modded".to_string()));
    update(
      &mut state,
      Msg::ProfileOpened {
        game: "valheim".to_string(),
        profile: "Modded".to_string(),
        installed: Ok(test_installed(&[])),
      },
    );

    assert!(state.can_rename_or_delete_profile());

    update(&mut state, Msg::ExportCodePressed);

    assert!(!state.can_rename_or_delete_profile());
  }

  #[test]
  fn importing_by_code_asks_for_the_code_then_reads_what_it_shares() {
    let mut state = ready();

    assert!(state.can_import_profile());
    assert_eq!(update(&mut state, Msg::ImportCodePressed), vec![]);
    assert_eq!(state.importing, Some(Importing::AskingCode(None)));

    let effects = update(
      &mut state,
      Msg::ImportCodeSubmitted(" abc-123 \n".to_string()),
    );

    assert_eq!(
      effects,
      vec![Effect::Run(Command::ReadShared {
        community: "valheim".to_string(),
        source: Source::Code("abc-123".to_string()),
      })]
    );
    assert_eq!(
      state.importing,
      Some(Importing::Reading(Source::Code("abc-123".to_string())))
    );
  }

  #[test]
  fn an_empty_code_is_not_read() {
    let mut state = ready();

    update(&mut state, Msg::ImportCodePressed);

    assert_eq!(
      update(&mut state, Msg::ImportCodeSubmitted("  ".to_string())),
      vec![]
    );
    assert_eq!(state.importing, Some(Importing::AskingCode(None)));
  }

  #[test]
  fn importing_from_a_file_asks_for_the_file_then_reads_it() {
    let mut state = ready();
    let path = PathBuf::from("/home/me/Modded.r2z");

    assert_eq!(
      update(&mut state, Msg::ImportFilePressed),
      vec![Effect::ChooseImportFile]
    );
    assert_eq!(
      update(&mut state, Msg::ImportFileChosen(Some(path.clone()))),
      vec![Effect::Run(Command::ReadShared {
        community: "valheim".to_string(),
        source: Source::File(path),
      })]
    );
  }

  #[test]
  fn closing_the_file_dialog_imports_nothing() {
    let mut state = ready();

    update(&mut state, Msg::ImportFilePressed);

    assert_eq!(update(&mut state, Msg::ImportFileChosen(None)), vec![]);
    assert_eq!(state.importing, None);
  }

  fn shared_package(full_name: &str, version: &str, enabled: bool) -> SharedPackage {
    SharedPackage {
      full_name: full_name.to_string(),
      version: version.to_string(),
      enabled,
    }
  }

  /// A profile shared as `name`, listing `Owner-ModA` 1.0.0 enabled,
  /// `Owner-ModC` 1.0.0 disabled, and two packages the index lacks.
  fn shared(name: &str) -> Shared {
    Shared {
      name: name.to_string(),
      packages: vec![
        shared_package("Owner-ModA", "1.0.0", true),
        shared_package("Owner-ModC", "1.0.0", false),
        shared_package("Owner-ModC", "3.0.0", true),
        shared_package("Owner-Gone", "1.0.0", true),
      ],
      archive: b"PK archive".to_vec(),
    }
  }

  fn code() -> Source {
    Source::Code("abc-123".to_string())
  }

  /// A state that has read `shared` from a code.
  fn reading_code(shared: Result<Shared, ReadFailure>) -> (State, Vec<Effect>) {
    let mut state = ready();

    update(&mut state, Msg::ImportCodePressed);
    update(&mut state, Msg::ImportCodeSubmitted("abc-123".to_string()));

    let effects = update(
      &mut state,
      Msg::SharedRead {
        source: code(),
        shared,
      },
    );

    (state, effects)
  }

  #[test]
  fn a_read_profile_asks_for_a_free_name_and_lists_what_the_index_lacks() {
    let (state, effects) = reading_code(Ok(shared("Default")));

    assert_eq!(
      effects,
      vec![Effect::FillProfileName("Default 2".to_string())]
    );
    assert_eq!(
      state.importing,
      Some(Importing::Naming(ImportForm {
        shared: shared("Default"),
        missing: vec![
          "Owner-ModC-3.0.0".to_string(),
          "Owner-Gone-1.0.0".to_string()
        ],
        refusal: None,
      }))
    );
  }

  #[test]
  fn a_code_that_cannot_be_read_asks_again_saying_why() {
    let (state, effects) = reading_code(Err(ReadFailure::UnknownCode));

    assert_eq!(effects, vec![]);
    assert_eq!(
      state.importing,
      Some(Importing::AskingCode(Some(ReadFailure::UnknownCode)))
    );
  }

  #[test]
  fn a_file_that_cannot_be_read_ends_the_import_saying_why() {
    let mut state = ready();
    let path = PathBuf::from("/home/me/broken.r2z");
    let damaged = ReadFailure::Damaged("not a zip".to_string());

    update(&mut state, Msg::ImportFilePressed);
    update(&mut state, Msg::ImportFileChosen(Some(path.clone())));

    let effects = update(
      &mut state,
      Msg::SharedRead {
        source: Source::File(path),
        shared: Err(damaged.clone()),
      },
    );

    assert_eq!(effects, vec![Effect::Notify(Notice::ImportUnread(damaged))]);
    assert_eq!(state.importing, None);
  }

  #[test]
  fn a_read_no_longer_waited_for_is_ignored() {
    let mut state = ready();

    update(&mut state, Msg::ImportCodePressed);
    update(&mut state, Msg::ImportCodeSubmitted("abc-123".to_string()));
    update(&mut state, Msg::ImportDismissed);

    let effects = update(
      &mut state,
      Msg::SharedRead {
        source: code(),
        shared: Ok(shared("Modded")),
      },
    );

    assert_eq!(effects, vec![]);
    assert_eq!(state.importing, None);
  }

  fn import_named(state: &mut State, name: &str) -> Vec<Effect> {
    update(state, Msg::ImportNameSubmitted(name.to_string()))
  }

  #[test]
  fn naming_the_import_queues_it_with_the_packages_the_index_has() {
    let (mut state, _) = reading_code(Ok(shared("Shared")));
    let index = state.browse.as_ref().unwrap().index.clone().unwrap();

    let effects = import_named(&mut state, "Shared");

    assert_eq!(
      effects,
      vec![Effect::Run(Command::Import(Import {
        operation: 1,
        game: "valheim".to_string(),
        community: "valheim".to_string(),
        profile: "Shared".to_string(),
        packages: vec![
          shared_package("Owner-ModA", "1.0.0", true),
          shared_package("Owner-ModC", "1.0.0", false),
        ],
        archive: shared("Shared").archive,
        index,
      }))]
    );
    assert_eq!(state.importing, None);
    assert_eq!(
      state
        .queue
        .operations
        .iter()
        .map(|operation| (operation.profile.as_str(), operation.action.clone()))
        .collect::<Vec<_>>(),
      vec![(
        "Shared",
        Action::Import(vec!["Owner-ModA".to_string(), "Owner-ModC".to_string()])
      )]
    );
  }

  fn refusal(state: &State) -> Option<NameRefusal> {
    match &state.importing {
      Some(Importing::Naming(form)) => form.refusal.clone(),
      _ => None,
    }
  }

  #[test]
  fn a_name_a_profile_has_is_refused_and_nothing_is_imported() {
    let (mut state, _) = reading_code(Ok(shared("Shared")));

    assert_eq!(import_named(&mut state, "modded"), vec![]);
    assert_eq!(
      refusal(&state),
      Some(NameRefusal::Taken("Modded".to_string()))
    );
    assert_eq!(import_named(&mut state, "CON"), vec![]);
    assert!(state.queue.operations.is_empty());
  }

  #[test]
  fn a_name_an_import_waiting_or_running_will_take_is_refused_everywhere() {
    let (mut state, _) = reading_code(Ok(shared("Shared")));

    import_named(&mut state, "Shared");

    update(&mut state, Msg::ImportCodePressed);
    update(&mut state, Msg::ImportCodeSubmitted("abc-123".to_string()));
    update(
      &mut state,
      Msg::SharedRead {
        source: code(),
        shared: Ok(shared("Shared")),
      },
    );

    assert_eq!(import_named(&mut state, "shared"), vec![]);
    assert_eq!(
      refusal(&state),
      Some(NameRefusal::Taken("Shared".to_string()))
    );

    update(&mut state, Msg::ImportDismissed);
    update(
      &mut state,
      Msg::ProfileNamePressed(crate::switcher::Naming::Create),
    );

    assert_eq!(
      update(&mut state, Msg::ProfileNameSubmitted("Shared".to_string())),
      vec![]
    );
  }

  /// A state importing `Shared` as operation 1.
  fn importing() -> State {
    let (mut state, _) = reading_code(Ok(shared("Shared")));

    import_named(&mut state, "Shared");

    state
  }

  fn import_finished(outcome: Result<Outcome, &str>) -> Msg {
    Msg::OperationFinished {
      operation: 1,
      outcome: outcome.map_err(str::to_string),
    }
  }

  fn imported(changed: &[&str], cancelled: &[&str]) -> Outcome {
    let names = |names: &[&str]| names.iter().map(|name| name.to_string()).collect();

    Outcome {
      changed: names(changed),
      failed: vec![],
      cancelled: names(cancelled),
      profile: Ok(test_installed(
        &changed
          .iter()
          .map(|name| (*name, "1.0.0"))
          .collect::<Vec<_>>(),
      )),
    }
  }

  fn import_action() -> Action {
    Action::Import(vec!["Owner-ModA".to_string(), "Owner-ModC".to_string()])
  }

  fn selected(state: &State) -> Option<&str> {
    state.profile.as_ref().map(|profile| profile.name.as_str())
  }

  #[test]
  fn a_finished_import_lists_the_new_profile_and_selects_it() {
    let mut state = importing();

    let effects = update(
      &mut state,
      import_finished(Ok(imported(&["Owner-ModA", "Owner-ModC"], &[]))),
    );

    assert_eq!(
      effects,
      vec![
        Effect::Notify(Notice::OperationFinished {
          game: "valheim".to_string(),
          profile: "Shared".to_string(),
          action: import_action(),
          changed: vec!["Owner-ModA".to_string(), "Owner-ModC".to_string()],
        }),
        Effect::Run(Command::SaveSettings(state.settings.clone())),
        Effect::Run(Command::OpenProfile {
          game: "valheim".to_string(),
          profile: "Shared".to_string(),
        }),
      ]
    );
    assert_eq!(state.profile_names(), vec!["Default", "Modded", "Shared"]);
    assert_eq!(state.mod_count("Shared"), Some(2));
    assert_eq!(selected(&state), Some("Shared"));
  }

  #[test]
  fn a_running_import_can_be_cancelled_and_lists_what_it_made_without_selecting_it() {
    let mut state = importing();

    assert_eq!(
      update(&mut state, Msg::OperationCancelPressed(1)),
      vec![Effect::Run(Command::Cancel(1))]
    );

    let effects = update(
      &mut state,
      import_finished(Ok(imported(&["Owner-ModA"], &["Owner-ModC-1.0.0"]))),
    );

    assert_eq!(
      effects,
      vec![Effect::Notify(Notice::OperationCancelled {
        game: "valheim".to_string(),
        profile: "Shared".to_string(),
        action: import_action(),
        changed: vec!["Owner-ModA".to_string()],
      })]
    );
    assert_eq!(state.profile_names(), vec!["Default", "Modded", "Shared"]);
    assert_eq!(selected(&state), Some(DEFAULT_PROFILE));
  }

  #[test]
  fn an_import_that_could_not_run_names_the_profile_and_lists_nothing() {
    let mut state = importing();

    let effects = update(&mut state, import_finished(Err("profile exists")));

    assert_eq!(
      effects,
      vec![Effect::Notify(Notice::OperationFailed {
        game: "valheim".to_string(),
        profile: "Shared".to_string(),
        action: import_action(),
        failed: vec![FailedPackage {
          package: "Shared".to_string(),
          reason: "profile exists".to_string(),
        }],
        changed: vec![],
      })]
    );
    assert_eq!(state.profile_names(), vec!["Default", "Modded"]);
  }

  #[test]
  fn leaving_the_game_drops_an_import_not_yet_queued() {
    let (mut state, _) = reading_code(Ok(shared("Shared")));

    update(&mut state, Msg::SwitchGamePressed);

    assert_eq!(state.importing, None);
  }
}
