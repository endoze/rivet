//! The one modal dialog over the window: which content the state asks for,
//! and what each holds.

use crate::palette::Palette;
use crate::{Shell, failure, send, text};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::Dialog;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::{
  AnyElement, App, ClickEvent, Context, Entity, InteractiveElement as _, IntoElement,
  ParentElement as _, SharedString, Styled as _, TestSupportExt as _, WeakEntity, Window, div,
};
use rivet_core::manage::{Folders, Pending, Scope};
use rivet_core::queue::{CloseChoice, Closing};
use rivet_core::{Msg, State};

/// What the window's one modal is showing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Modal {
  /// Asks what to do with the running changes before the window closes, or
  /// says it will close once they end.
  Closing,
  /// Asks for a name for a new, renamed, or copied profile.
  Naming,
  /// Asks to confirm deleting the selected profile.
  Deleting,
  /// Asks to confirm an install, listing what comes with it.
  Installing,
  /// Asks about an uninstall or disable in the Installed tab: whether to take
  /// the packages that need it too, whether to delete folders no package
  /// records, or why it cannot go ahead.
  Pending,
}

/// The modal `state` asks for, given the one `shown` now. When several are
/// pending, closing comes first, then naming, deleting, installing, and the
/// Installed tab's question.
///
/// Checking an uninstall opens no modal, since the check usually ends before
/// anyone could read it, but it keeps the Installed tab's question open while
/// it runs after one.
pub(crate) fn wanted_modal(state: &State, shown: Option<Modal>) -> Option<Modal> {
  let switcher = state.switcher.as_ref();
  let pending = match &state.manage.pending {
    Some(Pending::Checking { .. }) => shown == Some(Modal::Pending),
    Some(_) => true,
    None => false,
  };

  if state.closing.is_some() {
    Some(Modal::Closing)
  } else if switcher.is_some_and(|switcher| switcher.form.is_some()) {
    Some(Modal::Naming)
  } else if switcher.is_some_and(|switcher| switcher.deleting.is_some()) {
    Some(Modal::Deleting)
  } else if state.confirmation.is_some() {
    Some(Modal::Installing)
  } else if pending {
    Some(Modal::Pending)
  } else {
    None
  }
}

impl Shell {
  /// Opens, swaps, or closes the window's modal to match the state. Deferred
  /// past the render, since dialogs belong to the window's root view.
  pub(crate) fn sync_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let wanted = wanted_modal(&self.state, self.modal);

    if wanted == self.modal {
      return;
    }

    let shown = std::mem::replace(&mut self.modal, wanted);
    let shell = cx.weak_entity();
    let input = self.profile_name.clone();

    window.defer(cx, move |window, cx| {
      if shown.is_some() && window.has_active_dialog(cx) {
        window.close_dialog(cx);
      }

      match wanted {
        Some(Modal::Closing) => window.open_dialog(cx, closing(shell)),
        Some(Modal::Naming) => window.open_dialog(cx, naming(shell, input)),
        Some(Modal::Deleting) => window.open_dialog(cx, deleting(shell)),
        Some(Modal::Installing) => window.open_dialog(cx, installing(shell)),
        Some(Modal::Pending) => window.open_dialog(cx, pending(shell)),
        None => {}
      }
    });
  }
}

/// The frame every modal shares: a title, a body, and a dismiss button beside
/// the buttons that go ahead at the bottom right. Escape and the close button
/// send what the dismiss button sends.
struct Frame {
  title: String,
  /// Finds the body in tests.
  id: &'static str,
  body: Vec<AnyElement>,
  /// The dismiss button's id.
  cancel: &'static str,
  /// The dismiss button's label.
  cancel_label: &'static str,
  /// Sent by the dismiss button, Escape, and the close button alike.
  dismissed: Msg,
  /// The buttons that go ahead, none when nothing can.
  actions: Vec<Button>,
}

impl Frame {
  fn fill(self, dialog: Dialog, shell: &WeakEntity<Shell>) -> Dialog {
    dialog
      .title(SharedString::from(self.title))
      .on_ok(|_, _, _| false)
      .on_close(send(shell, self.dismissed.clone()))
      .child(
        v_flex()
          .id(self.id)
          .gap_3()
          .children(self.body)
          .child(
            h_flex()
              .gap_2()
              .justify_end()
              .child(
                Button::new(self.cancel)
                  .label(self.cancel_label)
                  .on_click(send(shell, self.dismissed)),
              )
              .children(self.actions),
          )
          .test_support(),
      )
  }
}

/// The modal asking for a profile name, with why the last one given was
/// refused. It focuses the name field when it opens.
fn naming(
  shell: WeakEntity<Shell>,
  input: Entity<InputState>,
) -> impl Fn(Dialog, &mut Window, &mut App) -> Dialog {
  let focus = std::cell::Cell::new(true);

  move |dialog, window, cx| {
    if focus.replace(false) {
      input.update(cx, |input, cx| input.focus(window, cx));
    }

    let form = shell.upgrade().and_then(|shell| {
      let switcher = shell.read(cx).state.switcher.as_ref()?;

      switcher.form.clone()
    });
    let Some(form) = form else {
      return dialog;
    };
    let palette = Palette::new(cx);
    let submit = {
      let shell = shell.clone();

      move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
        shell
          .update(cx, |this, cx| this.submit_profile_name(cx))
          .ok();
      }
    };
    let refusal = form.refusal.as_ref().map(|refusal| {
      div()
        .id("profile-name-refused")
        .child(failure(palette, text::name_refused(refusal)))
        .test_support()
        .into_any_element()
    });
    let frame = Frame {
      title: text::name_prompt(form.naming, &form.from),
      id: "profile-name-dialog",
      body: std::iter::once(Input::new(&input).into_any_element())
        .chain(refusal)
        .collect(),
      cancel: "cancel-profile-name",
      cancel_label: text::CANCEL,
      dismissed: Msg::ProfileNameDismissed,
      actions: vec![
        Button::new("save-profile-name")
          .label(text::SAVE)
          .primary()
          .on_click(submit),
      ],
    };

    frame.fill(dialog, &shell)
  }
}

/// The modal asking to confirm deleting the selected profile.
fn deleting(shell: WeakEntity<Shell>) -> impl Fn(Dialog, &mut Window, &mut App) -> Dialog {
  move |dialog, _, cx| {
    let deleting = shell.upgrade().and_then(|shell| {
      let switcher = shell.read(cx).state.switcher.as_ref()?;

      switcher.deleting.clone()
    });
    let Some(name) = deleting else {
      return dialog;
    };
    let frame = Frame {
      title: text::confirm_delete(&name),
      id: "delete-profile-dialog",
      body: vec![text::DELETE_WARNING.into_any_element()],
      cancel: "cancel-delete-profile",
      cancel_label: text::CANCEL,
      dismissed: Msg::DeleteProfileDismissed,
      actions: vec![
        Button::new("confirm-delete-profile")
          .label(text::DELETE)
          .danger()
          .on_click(send(&shell, Msg::DeleteProfileConfirmed)),
      ],
    };

    frame.fill(dialog, &shell)
  }
}

/// The modal asking to confirm an install: the package, what comes with it,
/// and any dependencies the index lacks, which keep it from going ahead.
fn installing(shell: WeakEntity<Shell>) -> impl Fn(Dialog, &mut Window, &mut App) -> Dialog {
  move |dialog, _, cx| {
    let confirmation = shell
      .upgrade()
      .and_then(|shell| shell.read(cx).state.confirmation.clone());
    let Some(confirmation) = confirmation else {
      return dialog;
    };
    let palette = Palette::new(cx);
    let dependencies = confirmation.dependencies.iter().map(|package| {
      div()
        .pl_3()
        .child(text::planned(package))
        .into_any_element()
    });
    let lead = if confirmation.dependencies.is_empty() {
      text::NO_DEPENDENCIES
    } else {
      text::WITH_DEPENDENCIES
    };
    let can_confirm = confirmation.can_confirm();
    let missing = (!can_confirm).then(|| {
      failure(palette, text::missing_dependencies(&confirmation.missing)).into_any_element()
    });
    let frame = Frame {
      title: text::confirm_install(&confirmation.package),
      id: "install-dialog",
      body: std::iter::once(lead.into_any_element())
        .chain(dependencies)
        .chain(missing)
        .collect(),
      cancel: "dismiss-install",
      cancel_label: text::CANCEL,
      dismissed: Msg::InstallDismissed,
      actions: can_confirm
        .then(|| {
          Button::new("confirm-install")
            .label(text::INSTALL)
            .primary()
            .on_click(send(&shell, Msg::InstallConfirmed))
        })
        .into_iter()
        .collect(),
    };

    frame.fill(dialog, &shell)
  }
}

/// The modal asking about an uninstall or disable in the Installed tab. Its
/// content follows the question as it changes, so choosing to take the
/// dependants, checking the uninstall, and asking about folders all happen in
/// one modal.
fn pending(shell: WeakEntity<Shell>) -> impl Fn(Dialog, &mut Window, &mut App) -> Dialog {
  move |dialog, _, cx| {
    let pending = shell
      .upgrade()
      .and_then(|shell| shell.read(cx).state.manage.pending.clone());
    let Some(pending) = pending else {
      return dialog;
    };
    let list = |items: Vec<String>| {
      items
        .into_iter()
        .map(|item| div().pl_3().child(item).into_any_element())
    };
    let (title, id, body, cancel_label, actions): (_, _, Vec<AnyElement>, _, _) = match pending {
      Pending::Dependants {
        removal,
        package,
        dependants,
      } => (
        text::dependants_question(removal, &package),
        "dependants-question",
        list(dependants).collect(),
        text::CANCEL,
        vec![
          Button::new("package-only")
            .label(text::PACKAGE_ONLY)
            .on_click(send(&shell, Msg::DependantsChosen(Scope::PackageOnly))),
          Button::new("with-dependants")
            .label(text::WITH_DEPENDANTS)
            .on_click(send(&shell, Msg::DependantsChosen(Scope::WithDependants))),
        ],
      ),
      Pending::Checking { .. } => (
        text::CHECKING_UNINSTALL.to_string(),
        "checking-uninstall",
        Vec::new(),
        text::CANCEL,
        Vec::new(),
      ),
      Pending::Untracked { packages, folders } => (
        text::untracked_question(&packages),
        "untracked-question",
        std::iter::once(text::UNTRACKED_LEAD.into_any_element())
          .chain(list(
            folders
              .iter()
              .map(|folder| folder.display().to_string())
              .collect(),
          ))
          .collect(),
        text::CANCEL,
        vec![
          Button::new("delete-folders")
            .label(text::DELETE_FOLDERS)
            .danger()
            .on_click(send(&shell, Msg::FoldersChosen(Folders::Delete))),
          Button::new("keep-folders")
            .label(text::KEEP_FOLDERS)
            .on_click(send(&shell, Msg::FoldersChosen(Folders::Keep))),
        ],
      ),
      Pending::Refused { packages, reason } => {
        let refusal = text::uninstall_refused(&packages, &reason);

        (
          refusal.headline,
          "uninstall-refused",
          vec![
            refusal.detail.into_any_element(),
            refusal.next.into_any_element(),
          ],
          text::DISMISS,
          Vec::new(),
        )
      }
    };
    let frame = Frame {
      title,
      id,
      body,
      cancel: "dismiss-pending",
      cancel_label,
      dismissed: Msg::PendingDismissed,
      actions,
    };

    frame.fill(dialog, &shell)
  }
}

/// The modal shown when the window is asked to close while changes run:
/// whether to wait for them or cancel them first, then, once chosen, that the
/// window closes when they end.
fn closing(shell: WeakEntity<Shell>) -> impl Fn(Dialog, &mut Window, &mut App) -> Dialog {
  move |dialog, _, cx| {
    let closing = shell
      .upgrade()
      .and_then(|shell| shell.read(cx).state.closing);
    let Some(closing) = closing else {
      return dialog;
    };
    let (body, actions) = match closing {
      Closing::Asking => (
        text::CLOSE_DETAIL,
        vec![
          Button::new("close-cancel")
            .label(text::CLOSE_CANCEL)
            .on_click(send(&shell, Msg::CloseChosen(CloseChoice::Cancel))),
          Button::new("close-wait")
            .label(text::CLOSE_WAIT)
            .primary()
            .on_click(send(&shell, Msg::CloseChosen(CloseChoice::Wait))),
        ],
      ),
      Closing::Waiting => (text::CLOSING_WHEN_DONE, Vec::new()),
    };
    let frame = Frame {
      title: text::CLOSE_HEADLINE.to_string(),
      id: "closing",
      body: vec![body.into_any_element()],
      cancel: "close-dismiss",
      cancel_label: text::KEEP_OPEN,
      dismissed: Msg::CloseDismissed,
      actions,
    };

    frame.fill(dialog, &shell)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use rivet_core::install::{Confirmation, Planned};
  use rivet_core::switcher::{NameForm, Naming, Switcher};
  use thunderstore_engine::ecosystem::Ecosystem;

  /// A state with a profile name asked for, a deletion waiting, and an
  /// install waiting, each when asked.
  fn pending(naming: bool, deleting: bool, installing: bool) -> State {
    let mut state = State::new(Ecosystem::from_json(r#"{ "games": {} }"#).unwrap());

    state.switcher = Some(Switcher {
      game: "valheim".to_string(),
      names: vec!["Default".to_string()],
      form: naming.then_some(NameForm {
        naming: Naming::Create,
        from: "Default".to_string(),
        refusal: None,
      }),
      deleting: deleting.then(|| "Default".to_string()),
      changing: None,
      list_failure: None,
      counts: Default::default(),
    });
    state.confirmation = installing.then(|| Confirmation {
      package: Planned {
        full_name: "Owner-ModA".to_string(),
        version: "1.0.0".to_string(),
        installed: false,
      },
      dependencies: vec![],
      missing: vec![],
    });

    state
  }

  #[test]
  fn nothing_pending_wants_no_modal() {
    assert_eq!(wanted_modal(&pending(false, false, false), None), None);
  }

  #[test]
  fn each_pending_alone_wants_its_own_modal() {
    assert_eq!(
      wanted_modal(&pending(true, false, false), None),
      Some(Modal::Naming)
    );
    assert_eq!(
      wanted_modal(&pending(false, true, false), None),
      Some(Modal::Deleting)
    );
    assert_eq!(
      wanted_modal(&pending(false, false, true), None),
      Some(Modal::Installing)
    );
  }

  #[test]
  fn naming_comes_before_deleting_and_installing() {
    assert_eq!(
      wanted_modal(&pending(true, true, true), None),
      Some(Modal::Naming)
    );
    assert_eq!(
      wanted_modal(&pending(true, false, true), None),
      Some(Modal::Naming)
    );
  }

  #[test]
  fn deleting_comes_before_installing() {
    assert_eq!(
      wanted_modal(&pending(false, true, true), None),
      Some(Modal::Deleting)
    );
  }

  #[test]
  fn closing_comes_before_every_other_modal() {
    for closing in [Closing::Asking, Closing::Waiting] {
      let mut state = pending(true, true, true);

      state.closing = Some(closing);

      assert_eq!(wanted_modal(&state, None), Some(Modal::Closing));
    }
  }

  #[test]
  fn an_install_waiting_outside_a_game_still_wants_its_modal() {
    let mut state = pending(false, false, true);

    state.switcher = None;

    assert_eq!(wanted_modal(&state, None), Some(Modal::Installing));
  }

  fn asking(question: Pending) -> State {
    let mut state = pending(false, false, false);

    state.manage.pending = Some(question);

    state
  }

  fn checking() -> Pending {
    Pending::Checking {
      packages: vec!["Owner-ModA".to_string()],
    }
  }

  #[test]
  fn each_installed_tab_question_wants_its_modal() {
    let questions = [
      Pending::Dependants {
        removal: rivet_core::manage::Removal::Uninstall,
        package: "Owner-ModA".to_string(),
        dependants: vec!["Owner-ModC".to_string()],
      },
      Pending::Untracked {
        packages: vec!["Owner-ModA".to_string()],
        folders: vec!["Stray".into()],
      },
      Pending::Refused {
        packages: vec!["Owner-ModA".to_string()],
        reason: "read-only".to_string(),
      },
    ];

    for question in questions {
      assert_eq!(wanted_modal(&asking(question), None), Some(Modal::Pending));
    }
  }

  #[test]
  fn checking_an_uninstall_opens_no_modal_of_its_own() {
    assert_eq!(wanted_modal(&asking(checking()), None), None);
  }

  #[test]
  fn checking_an_uninstall_keeps_the_question_open_after_one() {
    assert_eq!(
      wanted_modal(&asking(checking()), Some(Modal::Pending)),
      Some(Modal::Pending)
    );
  }

  #[test]
  fn an_install_comes_before_the_installed_tab_question() {
    let mut state = pending(false, false, true);

    state.manage.pending = Some(Pending::Refused {
      packages: vec!["Owner-ModA".to_string()],
      reason: "read-only".to_string(),
    });

    assert_eq!(wanted_modal(&state, None), Some(Modal::Installing));
  }
}
