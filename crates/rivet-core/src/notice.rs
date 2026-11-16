//! What the user is told in passing: news that floats over the window for a
//! moment and takes no space in it. Anything the user must act on stays in
//! the window instead.

use crate::install::{Action, FailedPackage};
use crate::launch::LaunchFailure;
use crate::portability::{Destination, Exported, ReadFailure};
use crate::switcher::ProfileChange;
use std::path::PathBuf;
use thunderstore_engine::profile::launch::LaunchMode;

/// How a [`Notice`] reads at a glance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
  Info,
  Success,
  Warning,
  Error,
}

/// Something to tell the user in passing, shown with
/// [`crate::Effect::Notify`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
  /// An operation on a game's profile went through. `changed` lists the
  /// packages that changed, and is empty when nothing needed to.
  OperationFinished {
    /// The game, by its key in the game definitions.
    game: String,
    profile: String,
    action: Action,
    changed: Vec<String>,
  },
  /// An operation on a game's profile failed. `failed` names each package
  /// that failed and why, and `changed` lists the packages that changed
  /// before or despite the failure.
  OperationFailed {
    /// The game, by its key in the game definitions.
    game: String,
    profile: String,
    action: Action,
    failed: Vec<FailedPackage>,
    changed: Vec<String>,
  },
  /// An operation on a game's profile was cancelled. `changed` lists the
  /// packages that changed before it stopped, and is empty for one
  /// cancelled before it started.
  OperationCancelled {
    /// The game, by its key in the game definitions.
    game: String,
    profile: String,
    action: Action,
    changed: Vec<String>,
  },
  /// A change to a game's profiles was made.
  ProfileChanged(ProfileChange),
  /// A change to a game's profiles could not be made, with the reason.
  ProfileChangeFailed {
    change: ProfileChange,
    reason: String,
  },
  /// The package list could not be refreshed, with the reason. The cached
  /// list is still shown.
  IndexStale(String),
  /// A launch started the game.
  Launched {
    /// The game, by its key in the game definitions.
    game: String,
    mode: LaunchMode,
  },
  /// A launch did not start the game, with why.
  LaunchFailed {
    /// The game, by its key in the game definitions.
    game: String,
    mode: LaunchMode,
    failure: LaunchFailure,
  },
  /// The Steam launch option line was put on the clipboard.
  LaunchLineCopied,
  /// Steam's launch options for a game, by its key in the game definitions,
  /// were found to hold the line after being found without it.
  SteamOptionSet(String),
  /// A profile was exported. A code is on the clipboard too.
  ProfileExported { profile: String, exported: Exported },
  /// A profile could not be exported, with the reason.
  ExportFailed {
    profile: String,
    to: Destination,
    reason: String,
  },
  /// A file to import could not be read, so nothing was imported.
  ImportUnread(ReadFailure),
  /// The files launches copied into a game's folder were removed, except
  /// those listed: `changed` were changed since and left in place, and
  /// `failed` could not be removed, each with the reason.
  LoaderFilesRemoved {
    /// The game, by its key in the game definitions.
    game: String,
    changed: Vec<PathBuf>,
    failed: Vec<(PathBuf, String)>,
  },
  /// Update all left this many outdated packages alone for being pinned.
  PinnedSkipped(usize),
  /// A package, by `Owner-Name`, could not be unpinned, with the reason.
  UnpinFailed { package: String, reason: String },
  /// A profile's pin file could not be read, with the reason naming the
  /// file, so the profile shows no package pinned.
  PinsUnread(String),
}

impl Notice {
  /// How the notice reads at a glance.
  ///
  /// ```
  /// use rivet_core::notice::{Level, Notice};
  ///
  /// assert_eq!(Notice::LaunchLineCopied.level(), Level::Info);
  /// assert_eq!(Notice::IndexStale("offline".to_string()).level(), Level::Warning);
  /// ```
  pub fn level(&self) -> Level {
    match self {
      Notice::OperationFinished { changed, .. } if changed.is_empty() => Level::Info,
      Notice::OperationFinished { .. }
      | Notice::ProfileChanged(_)
      | Notice::Launched { .. }
      | Notice::SteamOptionSet(_)
      | Notice::ProfileExported { .. } => Level::Success,
      Notice::OperationFailed { .. }
      | Notice::ProfileChangeFailed { .. }
      | Notice::LaunchFailed { .. }
      | Notice::ExportFailed { .. }
      | Notice::ImportUnread(_)
      | Notice::UnpinFailed { .. } => Level::Error,
      Notice::LoaderFilesRemoved { failed, .. } if !failed.is_empty() => Level::Error,
      Notice::LoaderFilesRemoved { changed, .. } if !changed.is_empty() => Level::Warning,
      Notice::LoaderFilesRemoved { .. } => Level::Success,
      Notice::IndexStale(_) | Notice::PinsUnread(_) => Level::Warning,
      Notice::OperationCancelled { .. } | Notice::LaunchLineCopied | Notice::PinnedSkipped(_) => {
        Level::Info
      }
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn finished(changed: &[&str]) -> Notice {
    Notice::OperationFinished {
      game: "valheim".to_string(),
      profile: "Default".to_string(),
      action: Action::Install("Owner-Mod".to_string()),
      changed: changed.iter().map(|name| name.to_string()).collect(),
    }
  }

  /// A removal of loader files that left `changed` and failed to remove
  /// `failed`.
  fn removed(changed: &[&str], failed: &[&str]) -> Notice {
    Notice::LoaderFilesRemoved {
      game: "valheim".to_string(),
      changed: changed.iter().map(PathBuf::from).collect(),
      failed: failed
        .iter()
        .map(|name| (PathBuf::from(name), "in use".to_string()))
        .collect(),
    }
  }

  #[test]
  fn each_notice_reads_at_its_level() {
    let change = ProfileChange::Create("Modded".to_string());
    let levels = [
      (finished(&["Owner-Mod"]), Level::Success),
      (finished(&[]), Level::Info),
      (
        Notice::OperationFailed {
          game: "valheim".to_string(),
          profile: "Default".to_string(),
          action: Action::Install("Owner-Mod".to_string()),
          failed: vec![FailedPackage {
            package: "Owner-Mod".to_string(),
            reason: "offline".to_string(),
          }],
          changed: vec![],
        },
        Level::Error,
      ),
      (
        Notice::OperationCancelled {
          game: "valheim".to_string(),
          profile: "Default".to_string(),
          action: Action::Install("Owner-Mod".to_string()),
          changed: vec![],
        },
        Level::Info,
      ),
      (Notice::ProfileChanged(change.clone()), Level::Success),
      (
        Notice::ProfileChangeFailed {
          change,
          reason: "busy".to_string(),
        },
        Level::Error,
      ),
      (Notice::IndexStale("offline".to_string()), Level::Warning),
      (
        Notice::PinsUnread("pins.toml: bad".to_string()),
        Level::Warning,
      ),
      (
        Notice::Launched {
          game: "valheim".to_string(),
          mode: LaunchMode::Modded,
        },
        Level::Success,
      ),
      (Notice::LaunchLineCopied, Level::Info),
      (
        Notice::SteamOptionSet("valheim".to_string()),
        Level::Success,
      ),
      (removed(&[], &[]), Level::Success),
      (removed(&["winhttp.dll"], &[]), Level::Warning),
      (
        removed(&["winhttp.dll"], &["doorstop_config.ini"]),
        Level::Error,
      ),
    ];

    for (notice, level) in levels {
      assert_eq!(notice.level(), level, "{notice:?}");
    }
  }
}
