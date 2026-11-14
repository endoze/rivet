//! The operation queue: every change to a profile the user asked for, across
//! every game and profile, run one at a time in the order they were asked
//! for, and closing the window while some are still running or waiting.

use crate::install::{
  Action, FailedPackage, Install, Operation, OperationStatus, Outcome, ProfileStatus, Progress,
};
use crate::manage::{SetEnabled, Uninstall};
use crate::switcher::ProfileChange;
use crate::{Command, Effect, State};

/// Every operation of this run that has not been dismissed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Queue {
  /// In the order they were asked for, which is the order they run.
  pub operations: Vec<Operation>,
  /// How many operations have been asked for, so each gets its own id.
  pub asked: u64,
}

/// The command that carries out an [`Operation`], kept so it can start when
/// its turn comes and start again on retry. Its operation id is always the
/// operation's own.
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

  fn profile_mut(&mut self) -> &mut String {
    match self {
      Work::Install(install) => &mut install.profile,
      Work::Uninstall(uninstall) => &mut uninstall.profile,
      Work::SetEnabled(set) => &mut set.profile,
    }
  }

  fn renumbered(self, id: u64) -> Self {
    match self {
      Work::Install(install) => Work::Install(Install {
        operation: id,
        ..install
      }),
      Work::Uninstall(uninstall) => Work::Uninstall(Uninstall {
        operation: id,
        ..uninstall
      }),
      Work::SetEnabled(set) => Work::SetEnabled(SetEnabled {
        operation: id,
        ..set
      }),
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
  /// Cancel the waiting operations and stop a running install after the
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

  /// Whether the operation has not ended: it is waiting or running.
  pub fn is_active(&self) -> bool {
    self.is_running() || *self == OperationStatus::Queued
  }
}

impl Operation {
  /// Whether it can be cancelled: it is waiting, or it is a running install,
  /// which stops after the package it is on. A running uninstall or toggle
  /// cannot stop part way.
  pub fn can_cancel(&self) -> bool {
    match self.status {
      OperationStatus::Queued => true,
      OperationStatus::Running(_) => matches!(self.action, Action::Install(_)),
      _ => false,
    }
  }
}

impl State {
  /// Whether any operation is running or waiting its turn.
  pub fn is_busy(&self) -> bool {
    self
      .queue
      .operations
      .iter()
      .any(|operation| operation.status.is_active())
  }

  /// Whether the open profile has an operation running or waiting, so it
  /// cannot be launched, renamed, duplicated, or deleted.
  pub fn is_profile_busy(&self) -> bool {
    let Some(profile) = self.profile.as_ref() else {
      return false;
    };

    self.queue.operations.iter().any(|operation| {
      operation.status.is_active()
        && operation.game == profile.game
        && operation.profile == profile.name
    })
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

fn find(state: &mut State, id: u64) -> Option<&mut Operation> {
  state
    .queue
    .operations
    .iter_mut()
    .find(|operation| operation.id == id)
}

/// The running operation with id `id`, if there is one.
fn running(state: &mut State, id: u64) -> Option<&mut Operation> {
  find(state, id).filter(|operation| operation.status.is_running())
}

pub(crate) fn progressed(state: &mut State, id: u64, progress: Progress) -> Vec<Effect> {
  if let Some(operation) = running(state, id) {
    operation.status = match operation.status {
      OperationStatus::Cancelling(_) => OperationStatus::Cancelling(Some(progress)),
      _ => OperationStatus::Running(Some(progress)),
    };
  }

  Vec::new()
}

/// Applies the result of operation `id`: what it did, or why it could not run
/// at all, which leaves the profile as it was. Then starts the next one.
pub(crate) fn finished(
  state: &mut State,
  id: u64,
  outcome: Result<Outcome, String>,
) -> Vec<Effect> {
  let Some(operation) = running(state, id) else {
    return Vec::new();
  };

  let Outcome {
    changed,
    failed,
    cancelled,
    profile,
  } = match outcome {
    Ok(outcome) => outcome,
    Err(reason) => {
      operation.status = OperationStatus::Failed {
        failed: vec![FailedPackage {
          package: operation.action.packages().join(", "),
          reason,
        }],
        changed: Vec::new(),
      };

      return advance(state);
    }
  };

  operation.status = if !failed.is_empty() {
    OperationStatus::Failed { failed, changed }
  } else if !cancelled.is_empty() {
    OperationStatus::Cancelled { changed }
  } else {
    OperationStatus::Finished { changed }
  };

  let (game, name) = (operation.game.clone(), operation.profile.clone());

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

  advance(state)
}

/// Cancels operation `id` when [`Operation::can_cancel`] says it can: one
/// waiting never starts, and a running install stops after the package it is
/// on.
pub(crate) fn cancel_pressed(state: &mut State, id: u64) -> Vec<Effect> {
  let Some(operation) = find(state, id) else {
    return Vec::new();
  };

  cancel(operation)
}

fn cancel(operation: &mut Operation) -> Vec<Effect> {
  if !operation.can_cancel() {
    return Vec::new();
  }

  match &operation.status {
    OperationStatus::Queued => {
      operation.status = OperationStatus::Cancelled {
        changed: Vec::new(),
      };

      Vec::new()
    }
    OperationStatus::Running(progress) => {
      operation.status = OperationStatus::Cancelling(progress.clone());

      vec![Effect::Run(Command::Cancel(operation.id))]
    }
    _ => Vec::new(),
  }
}

/// Asks again for an operation that failed or was cancelled, as a new one at
/// the end of the queue. Ignored while a launch is being prepared.
pub(crate) fn retry_pressed(state: &mut State, id: u64) -> Vec<Effect> {
  if state.is_launching() {
    return Vec::new();
  }

  let Some(position) = state.queue.operations.iter().position(|operation| {
    operation.id == id
      && matches!(
        operation.status,
        OperationStatus::Failed { .. } | OperationStatus::Cancelled { .. }
      )
  }) else {
    return Vec::new();
  };
  let ended = state.queue.operations.remove(position);

  enqueue(state, ended.game, ended.profile, ended.action, |id| {
    ended.work.renumbered(id)
  })
}

/// Keeps ended operations on `game`'s profiles aimed at a profile that exists
/// after `change`, so a retry changes the profile it was asked for: a renamed
/// profile's follow it to its new name, and a deleted profile's are removed.
pub(crate) fn profile_changed(state: &mut State, game: &str, change: &ProfileChange) {
  let ended_on = |operation: &Operation, name: &str| {
    !operation.status.is_active() && operation.game == game && operation.profile == name
  };
  let operations = &mut state.queue.operations;

  match change {
    ProfileChange::Rename { from, to } => {
      for operation in operations
        .iter_mut()
        .filter(|operation| ended_on(operation, from))
      {
        operation.profile = to.clone();
        *operation.work.profile_mut() = to.clone();
      }
    }
    ProfileChange::Delete(name) => operations.retain(|operation| !ended_on(operation, name)),
    ProfileChange::Create(_) | ProfileChange::Duplicate { .. } => {}
  }
}

/// Removes operation `id` from the queue once it has ended.
pub(crate) fn dismissed(state: &mut State, id: u64) -> Vec<Effect> {
  state
    .queue
    .operations
    .retain(|operation| operation.id != id || operation.status.is_active());

  Vec::new()
}

/// Removes every operation that has ended.
pub(crate) fn cleared(state: &mut State) -> Vec<Effect> {
  state
    .queue
    .operations
    .retain(|operation| operation.status.is_active());

  Vec::new()
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
    for operation in &mut state.queue.operations {
      effects.extend(cancel(operation));
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
  use crate::switcher::Naming;
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

    assert_eq!(next, vec![set_enabled(2, "Modded", "Owner-Other", false)]);
    assert_eq!(
      statuses(&state),
      vec![
        (
          1,
          OperationStatus::Finished {
            changed: names(&["Owner-Solo"]),
          }
        ),
        (2, OperationStatus::Running(None)),
      ]
    );
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

    assert_eq!(next, vec![set_enabled(2, "Modded", "Owner-Other", false)]);
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
      Msg::InstallProgress {
        operation: 1,
        progress: progress.clone(),
      },
    );
    update(
      &mut state,
      Msg::InstallProgress {
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
  fn cancelling_a_waiting_operation_marks_it_cancelled_and_it_never_starts() {
    let mut state = two_queued();

    let effects = update(&mut state, Msg::OperationCancelPressed(2));

    assert_eq!(effects, vec![]);

    let next = update(&mut state, finished_with(1, Ok(outcome(&["Owner-Solo"]))));

    assert_eq!(next, vec![]);
    assert_eq!(
      statuses(&state)[1],
      (2, OperationStatus::Cancelled { changed: vec![] })
    );
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
      statuses(&state)[0],
      (
        1,
        OperationStatus::Cancelled {
          changed: names(&["Owner-Lib"]),
        }
      )
    );
    assert_eq!(next, vec![set_enabled(2, "Modded", "Owner-Other", false)]);
  }

  #[test]
  fn a_running_uninstall_or_toggle_cannot_be_cancelled_but_one_waiting_can() {
    let mut state = two_queued();

    assert!(!state.queue.operations[0].can_cancel());
    assert!(state.queue.operations[1].can_cancel());
    assert_eq!(update(&mut state, Msg::OperationCancelPressed(1)), vec![]);
    assert_eq!(statuses(&state)[0], (1, OperationStatus::Running(None)));
  }

  #[test]
  fn a_cancel_that_comes_too_late_to_stop_anything_leaves_the_operation_finished() {
    let mut state = installing_then_queued();

    update(&mut state, Msg::OperationCancelPressed(1));
    update(&mut state, finished_with(1, Ok(outcome(&["Owner-Pack"]))));

    assert_eq!(
      statuses(&state)[0],
      (
        1,
        OperationStatus::Finished {
          changed: names(&["Owner-Pack"]),
        }
      )
    );
  }

  #[test]
  fn a_failed_operation_names_what_failed_and_the_next_one_still_starts() {
    let mut state = two_queued();
    let failure = FailedPackage {
      package: "Owner-Solo".to_string(),
      reason: "permission denied".to_string(),
    };

    let next = update(
      &mut state,
      finished_with(
        1,
        Ok(Outcome {
          failed: vec![failure.clone()],
          ..outcome(&[])
        }),
      ),
    );

    assert_eq!(
      statuses(&state)[0],
      (
        1,
        OperationStatus::Failed {
          failed: vec![failure],
          changed: vec![],
        }
      )
    );
    assert_eq!(next, vec![set_enabled(2, "Modded", "Owner-Other", false)]);
  }

  #[test]
  fn an_operation_that_could_not_run_fails_naming_its_packages() {
    let mut state = two_queued();

    update(&mut state, finished_with(1, Err("disk full")));

    assert_eq!(
      statuses(&state)[0],
      (
        1,
        OperationStatus::Failed {
          failed: vec![FailedPackage {
            package: "Owner-Solo".to_string(),
            reason: "disk full".to_string(),
          }],
          changed: vec![],
        }
      )
    );
  }

  #[test]
  fn retrying_a_failed_operation_asks_for_it_again_at_the_end_of_the_queue() {
    let mut state = two_queued();

    update(&mut state, finished_with(1, Err("offline")));

    let retried = update(&mut state, Msg::RetryPressed(1));

    assert_eq!(retried, vec![]);
    assert_eq!(
      statuses(&state),
      vec![
        (2, OperationStatus::Running(None)),
        (3, OperationStatus::Queued)
      ]
    );

    let next = update(&mut state, finished_with(2, Ok(outcome(&["Owner-Other"]))));

    assert_eq!(
      next,
      vec![set_enabled(3, DEFAULT_PROFILE, "Owner-Solo", false)]
    );
  }

  #[test]
  fn retrying_with_nothing_running_starts_at_once() {
    let mut state = ready(DEFAULT_PROFILE);

    update(&mut state, Msg::DisablePressed("Owner-Solo".to_string()));
    update(&mut state, finished_with(1, Err("offline")));

    let retried = update(&mut state, Msg::RetryPressed(1));

    assert_eq!(
      retried,
      vec![set_enabled(2, DEFAULT_PROFILE, "Owner-Solo", false)]
    );
  }

  #[test]
  fn a_cancelled_operation_can_be_retried_but_a_running_or_finished_one_cannot() {
    let mut state = two_queued();

    update(&mut state, Msg::OperationCancelPressed(2));

    assert_eq!(update(&mut state, Msg::RetryPressed(1)), vec![]);

    update(&mut state, finished_with(1, Ok(outcome(&["Owner-Solo"]))));

    assert_eq!(update(&mut state, Msg::RetryPressed(1)), vec![]);
    assert_eq!(
      update(&mut state, Msg::RetryPressed(2)),
      vec![set_enabled(3, "Modded", "Owner-Other", false)]
    );
  }

  #[test]
  fn an_ended_operation_can_be_dismissed_and_a_running_one_cannot() {
    let mut state = two_queued();

    update(&mut state, Msg::OperationDismissed(1));
    update(&mut state, Msg::OperationDismissed(2));

    assert_eq!(statuses(&state).len(), 2);

    update(&mut state, finished_with(1, Ok(outcome(&["Owner-Solo"]))));
    update(&mut state, Msg::OperationDismissed(1));

    assert_eq!(statuses(&state), vec![(2, OperationStatus::Running(None))]);
  }

  #[test]
  fn clearing_removes_every_ended_operation() {
    let mut state = two_queued();

    update(&mut state, finished_with(1, Err("offline")));
    update(&mut state, Msg::ProfilePicked(DEFAULT_PROFILE.to_string()));
    update(&mut state, opened(DEFAULT_PROFILE));
    update(&mut state, Msg::DisablePressed("Owner-Solo".to_string()));
    update(&mut state, Msg::QueueCleared);

    assert_eq!(
      statuses(&state),
      vec![
        (2, OperationStatus::Running(None)),
        (3, OperationStatus::Queued)
      ]
    );
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

    assert_eq!(next, vec![set_enabled(2, "Modded", "Owner-Other", false)]);
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

  /// A state with valheim open on `Modded`, where disabling `Owner-Solo`
  /// failed as operation 1.
  fn failed_in_modded() -> State {
    let mut state = ready("Modded");

    update(&mut state, Msg::DisablePressed("Owner-Solo".to_string()));
    update(&mut state, finished_with(1, Err("offline")));

    state
  }

  fn profile_changed(change: ProfileChange) -> Msg {
    Msg::ProfileChanged {
      game: "valheim".to_string(),
      change,
      result: Ok(()),
    }
  }

  #[test]
  fn retrying_after_the_profile_was_renamed_changes_it_under_its_new_name() {
    let mut state = failed_in_modded();

    update(&mut state, Msg::ProfileNamePressed(Naming::Rename));
    update(&mut state, Msg::ProfileNameSubmitted("Renamed".to_string()));
    update(
      &mut state,
      profile_changed(ProfileChange::Rename {
        from: "Modded".to_string(),
        to: "Renamed".to_string(),
      }),
    );

    assert_eq!(
      update(&mut state, Msg::RetryPressed(1)),
      vec![set_enabled(2, "Renamed", "Owner-Solo", false)]
    );
  }

  #[test]
  fn deleting_a_profile_drops_its_ended_operations_so_none_can_recreate_it() {
    let mut state = failed_in_modded();

    update(&mut state, Msg::DeleteProfilePressed);
    update(&mut state, Msg::DeleteProfileConfirmed);
    update(
      &mut state,
      profile_changed(ProfileChange::Delete("Modded".to_string())),
    );

    assert_eq!(statuses(&state), vec![]);
    assert_eq!(update(&mut state, Msg::RetryPressed(1)), vec![]);
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
      vec![set_enabled(2, "Modded", "Owner-Other", false)]
    );
    assert_eq!(
      update(&mut state, finished_with(2, Ok(outcome(&["Owner-Other"])))),
      vec![Effect::CloseWindow]
    );
  }

  #[test]
  fn cancelling_to_close_stops_after_the_current_package_then_closes() {
    let mut state = installing_then_queued();

    update(&mut state, Msg::CloseRequested);

    let effects = update(&mut state, Msg::CloseChosen(CloseChoice::Cancel));

    assert_eq!(effects, vec![Effect::Run(Command::Cancel(1))]);
    assert_eq!(
      statuses(&state),
      vec![
        (1, OperationStatus::Cancelling(None)),
        (2, OperationStatus::Cancelled { changed: vec![] })
      ]
    );
    assert_eq!(
      update(&mut state, finished_with(1, Ok(outcome(&["Owner-Pack"])))),
      vec![Effect::CloseWindow]
    );
  }

  #[test]
  fn cancelling_to_close_lets_a_running_toggle_finish_then_closes() {
    let mut state = two_queued();

    update(&mut state, Msg::CloseRequested);

    let effects = update(&mut state, Msg::CloseChosen(CloseChoice::Cancel));

    assert_eq!(effects, vec![]);
    assert_eq!(
      statuses(&state),
      vec![
        (1, OperationStatus::Running(None)),
        (2, OperationStatus::Cancelled { changed: vec![] })
      ]
    );
    assert_eq!(
      update(&mut state, finished_with(1, Ok(outcome(&["Owner-Solo"])))),
      vec![Effect::CloseWindow]
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
      vec![]
    );
  }
}
