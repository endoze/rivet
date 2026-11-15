//! The operation queue: every change to a profile the user asked for, across
//! every game and profile, run one at a time in the order they were asked
//! for, and closing the window while some are still running or waiting.

use crate::install::{
  Action, FailedPackage, Install, Operation, OperationStatus, Outcome, ProfileStatus, Progress,
};
use crate::manage::{SetEnabled, Uninstall};
use crate::notice::Notice;
use crate::{Command, Effect, State};

/// Every operation running or waiting its turn. One leaves the queue as soon
/// as it ends, and a notice tells the user how it went.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Queue {
  /// In the order they were asked for, which is the order they run.
  pub operations: Vec<Operation>,
  /// How many operations have been asked for, so each gets its own id.
  pub asked: u64,
}

impl Queue {
  /// The operation running now, including one stopping after a cancel.
  pub fn running(&self) -> Option<&Operation> {
    self
      .operations
      .iter()
      .find(|operation| operation.status.is_running())
  }

  /// How many operations wait for their turn.
  pub fn waiting(&self) -> usize {
    self
      .operations
      .iter()
      .filter(|operation| operation.status == OperationStatus::Queued)
      .count()
  }
}

/// The command that carries out an [`Operation`], kept so it can start when
/// its turn comes. Its operation id is always the operation's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Work {
  Install(Install),
  Uninstall(Uninstall),
  SetEnabled(SetEnabled),
}

impl Work {
  fn command(&self) -> Command {
    match self.clone() {
      Work::Install(install) => Command::Install(install),
      Work::Uninstall(uninstall) => Command::Uninstall(uninstall),
      Work::SetEnabled(set) => Command::SetEnabled(set),
    }
  }
}

/// Closing the window while operations are running or waiting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closing {
  /// Asking whether to wait for them or cancel them.
  Asking,
  /// The window closes once no operation is running or waiting.
  Waiting,
}

/// The user's answer to closing the window while work is in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseChoice {
  /// Let every operation run, then close.
  Wait,
  /// Cancel the waiting operations and stop the running one after the
  /// package it is on, then close.
  Cancel,
}

impl OperationStatus {
  /// Whether the operation is running, including stopping after a cancel.
  pub fn is_running(&self) -> bool {
    matches!(
      self,
      OperationStatus::Running(_) | OperationStatus::Cancelling(_)
    )
  }
}

impl Operation {
  /// Whether it can be cancelled: it is waiting, or it is running and has not
  /// been asked to stop yet. A running one stops after the package it is on.
  pub fn can_cancel(&self) -> bool {
    matches!(
      self.status,
      OperationStatus::Queued | OperationStatus::Running(_)
    )
  }
}

impl State {
  /// Whether any operation is running or waiting its turn.
  pub fn is_busy(&self) -> bool {
    !self.queue.operations.is_empty()
  }

  /// Whether the open profile has an operation running or waiting, so it
  /// cannot be launched, renamed, duplicated, or deleted.
  pub fn is_profile_busy(&self) -> bool {
    let Some(profile) = self.profile.as_ref() else {
      return false;
    };

    self
      .queue
      .operations
      .iter()
      .any(|operation| operation.game == profile.game && operation.profile == profile.name)
  }
}

/// Adds `action` on `game`'s `profile` to the end of the queue, carried out by
/// the work `work` builds for its id, and starts it if nothing else runs.
pub(crate) fn enqueue(
  state: &mut State,
  game: String,
  profile: String,
  action: Action,
  work: impl FnOnce(u64) -> Work,
) -> Vec<Effect> {
  state.queue.asked += 1;

  let id = state.queue.asked;

  state.queue.operations.push(Operation {
    id,
    game,
    profile,
    action,
    work: work(id),
    status: OperationStatus::Queued,
  });

  advance(state)
}

/// Starts the first waiting operation when none is running. Closes the window
/// when it was waiting for the queue to empty and nothing is left.
fn advance(state: &mut State) -> Vec<Effect> {
  let operations = &mut state.queue.operations;

  if operations
    .iter()
    .any(|operation| operation.status.is_running())
  {
    return Vec::new();
  }

  if let Some(next) = operations
    .iter_mut()
    .find(|operation| operation.status == OperationStatus::Queued)
  {
    next.status = OperationStatus::Running(None);

    return vec![Effect::Run(next.work.command())];
  }

  if state.closing == Some(Closing::Waiting) {
    state.closing = None;

    return vec![Effect::CloseWindow];
  }

  Vec::new()
}

/// Where operation `id` is in the queue, when it is running.
fn running(state: &State, id: u64) -> Option<usize> {
  state
    .queue
    .operations
    .iter()
    .position(|operation| operation.id == id && operation.status.is_running())
}

pub(crate) fn progressed(state: &mut State, id: u64, progress: Progress) -> Vec<Effect> {
  if let Some(position) = running(state, id) {
    let operation = &mut state.queue.operations[position];

    operation.status = match operation.status {
      OperationStatus::Cancelling(_) => OperationStatus::Cancelling(Some(progress)),
      _ => OperationStatus::Running(Some(progress)),
    };
  }

  Vec::new()
}

/// Applies the result of operation `id`: what it did, or why it could not run
/// at all, which leaves the profile as it was. Then starts the next one.
///
/// It leaves the queue at once, whatever its outcome, with a notice saying
/// what it changed, what failed and why, or what changed before it was
/// cancelled.
pub(crate) fn finished(
  state: &mut State,
  id: u64,
  outcome: Result<Outcome, String>,
) -> Vec<Effect> {
  let Some(position) = running(state, id) else {
    return Vec::new();
  };
  let Operation {
    game,
    profile: name,
    action,
    ..
  } = state.queue.operations.remove(position);
  let Outcome {
    changed,
    failed,
    cancelled,
    profile,
  } = match outcome {
    Ok(outcome) => outcome,
    Err(reason) => {
      let failed = vec![FailedPackage {
        package: action.packages().join(", "),
        reason,
      }];
      let mut effects = vec![Effect::Notify(Notice::OperationFailed {
        game,
        profile: name,
        action,
        failed,
        changed: Vec::new(),
      })];

      effects.extend(advance(state));

      return effects;
    }
  };

  if let Ok(installed) = &profile {
    crate::switcher::counted(state, &game, &name, installed.len());
  }

  if let Some(open) = state
    .profile
    .as_mut()
    .filter(|open| open.game == game && open.name == name)
  {
    open.status = match profile {
      Ok(installed) => ProfileStatus::Ready(installed),
      Err(reason) => ProfileStatus::Failed(reason),
    };
  }

  let notice = if !failed.is_empty() {
    Notice::OperationFailed {
      game,
      profile: name,
      action,
      failed,
      changed,
    }
  } else if !cancelled.is_empty() {
    Notice::OperationCancelled {
      game,
      profile: name,
      action,
      changed,
    }
  } else {
    Notice::OperationFinished {
      game,
      profile: name,
      action,
      changed,
    }
  };
  let mut effects = vec![Effect::Notify(notice)];

  effects.extend(advance(state));

  effects
}

/// Cancels operation `id` when [`Operation::can_cancel`] says it can: one
/// waiting leaves the queue without starting, and a running one stops after
/// the package it is on.
pub(crate) fn cancel_pressed(state: &mut State, id: u64) -> Vec<Effect> {
  let operations = &mut state.queue.operations;
  let Some(position) = operations.iter().position(|operation| operation.id == id) else {
    return Vec::new();
  };

  match operations[position].status.clone() {
    OperationStatus::Queued => {
      let Operation {
        game,
        profile,
        action,
        ..
      } = operations.remove(position);

      vec![Effect::Notify(Notice::OperationCancelled {
        game,
        profile,
        action,
        changed: Vec::new(),
      })]
    }
    OperationStatus::Running(progress) => {
      operations[position].status = OperationStatus::Cancelling(progress);

      vec![Effect::Run(Command::Cancel(id))]
    }
    OperationStatus::Cancelling(_) => Vec::new(),
  }
}

/// Closes the window at once when no operation is running or waiting, and
/// otherwise asks whether to wait for them or cancel them.
pub(crate) fn close_requested(state: &mut State) -> Vec<Effect> {
  if !state.is_busy() {
    return vec![Effect::CloseWindow];
  }

  if state.closing.is_none() {
    state.closing = Some(Closing::Asking);
  }

  Vec::new()
}

/// Closes the window once the queue empties, cancelling what it holds first
/// when `choice` says so.
pub(crate) fn close_chosen(state: &mut State, choice: CloseChoice) -> Vec<Effect> {
  if state.closing != Some(Closing::Asking) {
    return Vec::new();
  }

  state.closing = Some(Closing::Waiting);

  let mut effects = Vec::new();

  if choice == CloseChoice::Cancel {
    let ids: Vec<u64> = state
      .queue
      .operations
      .iter()
      .map(|operation| operation.id)
      .collect();

    for id in ids {
      effects.extend(cancel_pressed(state, id));
    }
  }

  effects.extend(advance(state));

  effects
}

/// Keeps the window open, whether it was asking or waiting to close.
pub(crate) fn close_dismissed(state: &mut State) -> Vec<Effect> {
  state.closing = None;

  Vec::new()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::install::{DEFAULT_PROFILE, Installed, Stage};
  use crate::settings::{GameSettings, Settings};
  use crate::{Msg, test_ecosystem, test_installed, update};
  use std::path::PathBuf;

  /// A state with valheim open on `selected`, holding `Owner-Solo` and
  /// `Owner-Other`, both enabled, with its profiles `Default` and `Modded`
  /// listed.
  fn ready(selected: &str) -> State {
    let mut state = State::new(test_ecosystem());
    let settings = Settings {
      last_game: Some("valheim".to_string()),
      games: [(
        "valheim".to_string(),
        GameSettings {
          folder: PathBuf::from("/games/valheim"),
          profile: (selected != DEFAULT_PROFILE).then(|| selected.to_string()),
        },
      )]
      .into(),
    };

    update(&mut state, Msg::Started);
    update(&mut state, Msg::SettingsLoaded(Ok(settings)));
    update(&mut state, opened(selected));
    update(
      &mut state,
      Msg::ProfilesListed {
        game: "valheim".to_string(),
        names: Ok(vec![DEFAULT_PROFILE.to_string(), "Modded".to_string()]),
      },
    );

    state
  }

  fn holding() -> Installed {
    test_installed(&[("Owner-Solo", "1.0.0"), ("Owner-Other", "1.0.0")])
  }

  fn opened(name: &str) -> Msg {
    Msg::ProfileOpened {
      game: "valheim".to_string(),
      profile: name.to_string(),
      installed: Ok(holding()),
    }
  }

  fn names(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| name.to_string()).collect()
  }

  fn set_enabled(operation: u64, profile: &str, package: &str, enabled: bool) -> Effect {
    Effect::Run(Command::SetEnabled(SetEnabled {
      operation,
      game: "valheim".to_string(),
      profile: profile.to_string(),
      packages: names(&[package]),
      enabled,
    }))
  }

  /// The notice of a finished operation on valheim's `profile`.
  fn notified(profile: &str, action: Action, changed: &[&str]) -> Effect {
    Effect::Notify(Notice::OperationFinished {
      game: "valheim".to_string(),
      profile: profile.to_string(),
      action,
      changed: names(changed),
    })
  }

  /// The notice of a cancelled operation on valheim's `profile`.
  fn cancelled(profile: &str, action: Action, changed: &[&str]) -> Effect {
    Effect::Notify(Notice::OperationCancelled {
      game: "valheim".to_string(),
      profile: profile.to_string(),
      action,
      changed: names(changed),
    })
  }

  /// The notice of cancelling the disable of `Owner-Other` in `Modded`
  /// before it started.
  fn other_cancelled_in_modded() -> Effect {
    cancelled(
      "Modded",
      Action::SetEnabled {
        packages: names(&["Owner-Other"]),
        enabled: false,
      },
      &[],
    )
  }

  /// The notice of disabling `package` in valheim's `profile`.
  fn disabled(profile: &str, package: &str) -> Effect {
    notified(
      profile,
      Action::SetEnabled {
        packages: names(&[package]),
        enabled: false,
      },
      &[package],
    )
  }

  fn statuses(state: &State) -> Vec<(u64, OperationStatus)> {
    state
      .queue
      .operations
      .iter()
      .map(|operation| (operation.id, operation.status.clone()))
      .collect()
  }

  fn outcome(changed: &[&str]) -> Outcome {
    Outcome {
      changed: names(changed),
      failed: vec![],
      cancelled: vec![],
      profile: Ok(holding()),
    }
  }

  fn finished_with(operation: u64, outcome: Result<Outcome, &str>) -> Msg {
    Msg::OperationFinished {
      operation,
      outcome: outcome.map_err(str::to_string),
    }
  }

  /// Opens the `Modded` profile and asks to disable `Owner-Other` in it.
  fn disable_other_in_modded(state: &mut State) -> Vec<Effect> {
    update(state, Msg::ProfilePicked("Modded".to_string()));
    update(state, opened("Modded"));
    update(state, Msg::DisablePressed("Owner-Other".to_string()))
  }

  /// A state where disabling `Owner-Solo` in the Default profile runs as
  /// operation 1 and disabling `Owner-Other` in `Modded`, now open, waits as
  /// operation 2.
  fn two_queued() -> State {
    let mut state = ready(DEFAULT_PROFILE);

    update(&mut state, Msg::DisablePressed("Owner-Solo".to_string()));
    disable_other_in_modded(&mut state);

    state
  }

  /// As [`two_queued`], with operation 1 installing `Owner-Pack` instead.
  fn installing_then_queued() -> State {
    let mut state = ready(DEFAULT_PROFILE);
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
    disable_other_in_modded(&mut state);

    assert_eq!(
      statuses(&state),
      vec![
        (1, OperationStatus::Running(None)),
        (2, OperationStatus::Queued)
      ]
    );

    state
  }

  #[test]
  fn a_change_asked_for_while_one_runs_waits_and_starts_when_the_first_ends() {
    let mut state = ready(DEFAULT_PROFILE);

    let first = update(&mut state, Msg::DisablePressed("Owner-Solo".to_string()));
    let second = disable_other_in_modded(&mut state);

    assert_eq!(
      first,
      vec![set_enabled(1, DEFAULT_PROFILE, "Owner-Solo", false)]
    );
    assert_eq!(second, vec![]);
    assert_eq!(
      statuses(&state),
      vec![
        (1, OperationStatus::Running(None)),
        (2, OperationStatus::Queued)
      ]
    );

    let next = update(&mut state, finished_with(1, Ok(outcome(&["Owner-Solo"]))));

    assert_eq!(
      next,
      vec![
        disabled(DEFAULT_PROFILE, "Owner-Solo"),
        set_enabled(2, "Modded", "Owner-Other", false)
      ]
    );
    assert_eq!(statuses(&state), vec![(2, OperationStatus::Running(None))]);
  }

  #[test]
  fn operations_on_every_profile_share_one_queue_in_the_order_asked_for() {
    let mut state = ready(DEFAULT_PROFILE);

    update(&mut state, Msg::DisablePressed("Owner-Solo".to_string()));
    update(&mut state, Msg::ProfilePicked("Modded".to_string()));
    update(&mut state, opened("Modded"));
    update(&mut state, Msg::DisablePressed("Owner-Other".to_string()));

    let places: Vec<(&str, &str)> = state
      .queue
      .operations
      .iter()
      .map(|operation| (operation.game.as_str(), operation.profile.as_str()))
      .collect();

    assert_eq!(
      places,
      vec![("valheim", DEFAULT_PROFILE), ("valheim", "Modded")]
    );

    let next = update(&mut state, finished_with(1, Ok(outcome(&["Owner-Solo"]))));

    assert_eq!(
      next,
      vec![
        disabled(DEFAULT_PROFILE, "Owner-Solo"),
        set_enabled(2, "Modded", "Owner-Other", false)
      ]
    );
  }

  #[test]
  fn progress_names_the_package_and_stage_of_the_running_operation_only() {
    let mut state = two_queued();
    let progress = Progress {
      package: "Owner-Lib".to_string(),
      stage: Stage::Extracting,
      done: 4,
      total: Some(10),
    };

    update(
      &mut state,
      Msg::OperationProgress {
        operation: 1,
        progress: progress.clone(),
      },
    );
    update(
      &mut state,
      Msg::OperationProgress {
        operation: 2,
        progress: progress.clone(),
      },
    );

    assert_eq!(
      statuses(&state),
      vec![
        (1, OperationStatus::Running(Some(progress))),
        (2, OperationStatus::Queued)
      ]
    );
  }

  #[test]
  fn the_queue_tells_the_running_operation_and_how_many_wait_behind_it() {
    let mut state = ready(DEFAULT_PROFILE);

    assert_eq!(state.queue.running(), None);
    assert_eq!(state.queue.waiting(), 0);

    update(&mut state, Msg::DisablePressed("Owner-Solo".to_string()));
    disable_other_in_modded(&mut state);

    assert_eq!(state.queue.running().map(|operation| operation.id), Some(1));
    assert_eq!(state.queue.waiting(), 1);

    update(&mut state, Msg::OperationCancelPressed(1));

    assert_eq!(state.queue.running().map(|operation| operation.id), Some(1));

    update(&mut state, finished_with(1, Err("offline")));

    assert_eq!(state.queue.running().map(|operation| operation.id), Some(2));
    assert_eq!(state.queue.waiting(), 0);
  }

  #[test]
  fn cancelling_a_waiting_operation_drops_it_saying_nothing_changed() {
    let mut state = two_queued();

    let effects = update(&mut state, Msg::OperationCancelPressed(2));

    assert_eq!(effects, vec![other_cancelled_in_modded()]);
    assert_eq!(statuses(&state), vec![(1, OperationStatus::Running(None))]);

    let next = update(&mut state, finished_with(1, Ok(outcome(&["Owner-Solo"]))));

    assert_eq!(next, vec![disabled(DEFAULT_PROFILE, "Owner-Solo")]);
    assert_eq!(statuses(&state), vec![]);
  }

  #[test]
  fn cancelling_a_running_install_stops_it_after_the_current_package() {
    let mut state = installing_then_queued();

    let effects = update(&mut state, Msg::OperationCancelPressed(1));

    assert_eq!(effects, vec![Effect::Run(Command::Cancel(1))]);
    assert_eq!(statuses(&state)[0], (1, OperationStatus::Cancelling(None)));
    assert_eq!(update(&mut state, Msg::OperationCancelPressed(1)), vec![]);

    let next = update(
      &mut state,
      finished_with(
        1,
        Ok(Outcome {
          cancelled: names(&["Owner-Pack"]),
          ..outcome(&["Owner-Lib"])
        }),
      ),
    );

    assert_eq!(
      next,
      vec![
        cancelled(
          DEFAULT_PROFILE,
          Action::Install("Owner-Pack".to_string()),
          &["Owner-Lib"]
        ),
        set_enabled(2, "Modded", "Owner-Other", false)
      ]
    );
    assert_eq!(statuses(&state), vec![(2, OperationStatus::Running(None))]);
  }

  #[test]
  fn cancelling_a_running_toggle_stops_it_after_the_current_package() {
    let mut state = ready(DEFAULT_PROFILE);

    update(&mut state, Msg::DisableAllPressed);

    let disabling = Progress {
      package: "Owner-Other".to_string(),
      stage: Stage::Disabling,
      done: 0,
      total: None,
    };

    update(
      &mut state,
      Msg::OperationProgress {
        operation: 1,
        progress: disabling.clone(),
      },
    );

    assert!(state.queue.operations[0].can_cancel());

    let effects = update(&mut state, Msg::OperationCancelPressed(1));

    assert_eq!(effects, vec![Effect::Run(Command::Cancel(1))]);
    assert_eq!(
      statuses(&state),
      vec![(1, OperationStatus::Cancelling(Some(disabling)))]
    );

    let ended = update(
      &mut state,
      finished_with(
        1,
        Ok(Outcome {
          cancelled: names(&["Owner-Solo"]),
          ..outcome(&["Owner-Other"])
        }),
      ),
    );

    assert_eq!(
      ended,
      vec![cancelled(
        DEFAULT_PROFILE,
        Action::SetEnabled {
          packages: names(&["Owner-Other", "Owner-Solo"]),
          enabled: false,
        },
        &["Owner-Other"]
      )]
    );
    assert_eq!(statuses(&state), vec![]);
  }

  #[test]
  fn a_running_uninstall_can_be_cancelled() {
    let mut state = ready(DEFAULT_PROFILE);

    update(&mut state, Msg::UninstallPressed("Owner-Solo".to_string()));
    update(
      &mut state,
      Msg::UninstallChecked {
        game: "valheim".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        packages: names(&["Owner-Solo"]),
        untracked: Ok(vec![]),
      },
    );

    let uninstall = state
      .queue
      .operations
      .iter()
      .find(|operation| matches!(operation.action, Action::Uninstall(_)));

    assert_eq!(
      uninstall.map(|operation| (&operation.status, operation.can_cancel())),
      Some((&OperationStatus::Running(None), true))
    );
  }

  #[test]
  fn a_cancel_that_comes_too_late_to_stop_anything_leaves_the_operation_finished() {
    let mut state = installing_then_queued();

    update(&mut state, Msg::OperationCancelPressed(1));

    let effects = update(&mut state, finished_with(1, Ok(outcome(&["Owner-Pack"]))));

    assert_eq!(
      effects,
      vec![
        notified(
          DEFAULT_PROFILE,
          Action::Install("Owner-Pack".to_string()),
          &["Owner-Pack"]
        ),
        set_enabled(2, "Modded", "Owner-Other", false)
      ]
    );
    assert_eq!(statuses(&state), vec![(2, OperationStatus::Running(None))]);
  }

  #[test]
  fn a_failed_operation_leaves_the_queue_naming_what_failed_and_the_next_one_starts() {
    let mut state = installing_then_queued();
    let failure = FailedPackage {
      package: "Owner-Pack".to_string(),
      reason: "permission denied".to_string(),
    };

    let next = update(
      &mut state,
      finished_with(
        1,
        Ok(Outcome {
          failed: vec![failure.clone()],
          ..outcome(&["Owner-Lib"])
        }),
      ),
    );

    assert_eq!(
      next,
      vec![
        Effect::Notify(Notice::OperationFailed {
          game: "valheim".to_string(),
          profile: DEFAULT_PROFILE.to_string(),
          action: Action::Install("Owner-Pack".to_string()),
          failed: vec![failure],
          changed: names(&["Owner-Lib"]),
        }),
        set_enabled(2, "Modded", "Owner-Other", false)
      ]
    );
    assert_eq!(statuses(&state), vec![(2, OperationStatus::Running(None))]);
  }

  #[test]
  fn an_operation_that_could_not_run_fails_naming_its_packages() {
    let mut state = two_queued();

    let next = update(&mut state, finished_with(1, Err("disk full")));

    assert_eq!(
      next,
      vec![
        Effect::Notify(Notice::OperationFailed {
          game: "valheim".to_string(),
          profile: DEFAULT_PROFILE.to_string(),
          action: Action::SetEnabled {
            packages: names(&["Owner-Solo"]),
            enabled: false,
          },
          failed: vec![FailedPackage {
            package: "Owner-Solo".to_string(),
            reason: "disk full".to_string(),
          }],
          changed: vec![],
        }),
        set_enabled(2, "Modded", "Owner-Other", false)
      ]
    );
    assert_eq!(statuses(&state), vec![(2, OperationStatus::Running(None))]);
  }

  #[test]
  fn switching_profile_or_game_does_not_interrupt_a_running_operation() {
    let mut state = two_queued();

    update(&mut state, Msg::ProfilePicked(DEFAULT_PROFILE.to_string()));
    update(&mut state, Msg::SwitchGamePressed);

    assert_eq!(
      statuses(&state),
      vec![
        (1, OperationStatus::Running(None)),
        (2, OperationStatus::Queued)
      ]
    );

    let next = update(&mut state, finished_with(1, Ok(outcome(&["Owner-Solo"]))));

    assert_eq!(
      next,
      vec![
        disabled(DEFAULT_PROFILE, "Owner-Solo"),
        set_enabled(2, "Modded", "Owner-Other", false)
      ]
    );
  }

  #[test]
  fn a_finished_operation_updates_the_open_profile_only_when_it_is_the_one_changed() {
    let mut state = two_queued();
    let mut after = holding();

    after.get_mut("Owner-Solo").unwrap().enabled = false;

    update(
      &mut state,
      finished_with(
        1,
        Ok(Outcome {
          profile: Ok(after),
          ..outcome(&["Owner-Solo"])
        }),
      ),
    );

    assert_eq!(
      state.profile.as_ref().map(|profile| &profile.status),
      Some(&ProfileStatus::Ready(holding()))
    );
  }

  #[test]
  fn only_the_profile_with_work_cannot_be_launched_renamed_or_deleted() {
    let mut state = ready("Modded");

    update(&mut state, Msg::DisablePressed("Owner-Solo".to_string()));

    assert!(state.is_profile_busy());
    assert!(!state.can_launch());
    assert!(!state.can_rename_or_delete_profile());
    assert!(!state.can_duplicate_profile());
    assert!(!state.can_manage());
    assert!(state.can_install());

    update(&mut state, Msg::ProfilePicked(DEFAULT_PROFILE.to_string()));
    update(&mut state, opened(DEFAULT_PROFILE));

    assert!(state.is_busy());
    assert!(!state.is_profile_busy());
    assert!(state.can_duplicate_profile());
    assert!(state.can_manage());
  }

  #[test]
  fn enabling_waits_until_a_disable_queued_on_the_same_profile_ends() {
    let mut state = ready("Modded");
    let mut solo_off = holding();

    solo_off.get_mut("Owner-Solo").unwrap().enabled = false;

    update(&mut state, Msg::DisablePressed("Owner-Solo".to_string()));
    update(&mut state, Msg::ProfilePicked(DEFAULT_PROFILE.to_string()));
    update(
      &mut state,
      Msg::ProfileOpened {
        game: "valheim".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        installed: Ok(solo_off.clone()),
      },
    );
    update(&mut state, Msg::DisablePressed("Owner-Other".to_string()));

    assert_eq!(statuses(&state)[1], (2, OperationStatus::Queued));
    assert!(!state.can_manage());
    assert_eq!(
      update(&mut state, Msg::EnablePressed("Owner-Solo".to_string())),
      vec![]
    );

    update(&mut state, finished_with(1, Ok(outcome(&["Owner-Solo"]))));
    update(
      &mut state,
      finished_with(
        2,
        Ok(Outcome {
          profile: Ok(solo_off),
          ..outcome(&["Owner-Other"])
        }),
      ),
    );

    assert!(state.can_manage());
    assert_eq!(
      update(&mut state, Msg::EnablePressed("Owner-Solo".to_string())),
      vec![set_enabled(3, DEFAULT_PROFILE, "Owner-Solo", true)]
    );
  }

  #[test]
  fn closing_with_nothing_running_closes_at_once() {
    let mut state = ready(DEFAULT_PROFILE);

    assert_eq!(
      update(&mut state, Msg::CloseRequested),
      vec![Effect::CloseWindow]
    );
  }

  #[test]
  fn closing_with_work_in_progress_asks_first() {
    let mut state = two_queued();

    let effects = update(&mut state, Msg::CloseRequested);

    assert_eq!(effects, vec![]);
    assert_eq!(state.closing, Some(Closing::Asking));

    update(&mut state, Msg::CloseDismissed);

    assert_eq!(state.closing, None);
    assert_eq!(statuses(&state)[0], (1, OperationStatus::Running(None)));
  }

  #[test]
  fn waiting_to_close_lets_every_operation_run_then_closes() {
    let mut state = two_queued();

    update(&mut state, Msg::CloseRequested);

    assert_eq!(
      update(&mut state, Msg::CloseChosen(CloseChoice::Wait)),
      vec![]
    );
    assert_eq!(state.closing, Some(Closing::Waiting));
    assert_eq!(
      update(&mut state, finished_with(1, Ok(outcome(&["Owner-Solo"])))),
      vec![
        disabled(DEFAULT_PROFILE, "Owner-Solo"),
        set_enabled(2, "Modded", "Owner-Other", false)
      ]
    );
    assert_eq!(
      update(&mut state, finished_with(2, Ok(outcome(&["Owner-Other"])))),
      vec![disabled("Modded", "Owner-Other"), Effect::CloseWindow]
    );
  }

  #[test]
  fn cancelling_to_close_stops_after_the_current_package_then_closes() {
    let mut state = installing_then_queued();

    update(&mut state, Msg::CloseRequested);

    let effects = update(&mut state, Msg::CloseChosen(CloseChoice::Cancel));

    assert_eq!(
      effects,
      vec![Effect::Run(Command::Cancel(1)), other_cancelled_in_modded()]
    );
    assert_eq!(
      statuses(&state),
      vec![(1, OperationStatus::Cancelling(None))]
    );
    assert_eq!(
      update(&mut state, finished_with(1, Ok(outcome(&["Owner-Pack"])))),
      vec![
        notified(
          DEFAULT_PROFILE,
          Action::Install("Owner-Pack".to_string()),
          &["Owner-Pack"]
        ),
        Effect::CloseWindow
      ]
    );
  }

  #[test]
  fn cancelling_to_close_stops_a_running_toggle_after_the_current_package_then_closes() {
    let mut state = two_queued();

    update(&mut state, Msg::CloseRequested);

    let effects = update(&mut state, Msg::CloseChosen(CloseChoice::Cancel));

    assert_eq!(
      effects,
      vec![Effect::Run(Command::Cancel(1)), other_cancelled_in_modded()]
    );
    assert_eq!(
      statuses(&state),
      vec![(1, OperationStatus::Cancelling(None))]
    );
    assert_eq!(
      update(&mut state, finished_with(1, Ok(outcome(&["Owner-Solo"])))),
      vec![disabled(DEFAULT_PROFILE, "Owner-Solo"), Effect::CloseWindow]
    );
  }

  #[test]
  fn keeping_the_window_open_while_waiting_to_close_stops_it_closing() {
    let mut state = two_queued();

    update(&mut state, Msg::CloseRequested);
    update(&mut state, Msg::CloseChosen(CloseChoice::Wait));
    update(&mut state, Msg::CloseDismissed);
    update(&mut state, finished_with(1, Ok(outcome(&["Owner-Solo"]))));

    assert_eq!(
      update(&mut state, finished_with(2, Ok(outcome(&["Owner-Other"])))),
      vec![disabled("Modded", "Owner-Other")]
    );
  }
}
