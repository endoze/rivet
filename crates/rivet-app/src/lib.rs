//! The gpui shell: draws [`State`], turns input into [`Msg`]s, runs the
//! [`Effect`]s that need the window, and hands the rest to the commands
//! thread.

mod icons;
pub mod text;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  AnyElement, AppContext as _, ClipboardItem, Context, Entity, FontWeight, InteractiveElement as _,
  IntoElement, ParentElement as _, PathPromptOptions, Render, SharedString,
  StatefulInteractiveElement as _, Styled as _, Subscription, Task, TestSupportExt as _, Window,
  div, img, px, rgb, uniform_list,
};
use icons::IconCache;
use rivet_core::browse::{Browse, View};
use rivet_core::install::{Confirmation, Operation, OperationStatus, ProfileStatus};
use rivet_core::launch::{AttemptStatus, OptionStatus, SteamOption};
use rivet_core::picker::Step;
use rivet_core::{Command, Effect, Msg, Screen, State};
use std::ops::Range;
use thunderstore_engine::profile::launch::LaunchMode;
use thunderstore_engine::query::PackageRow;

/// The root view. It owns the state and is the only place `update` is called.
pub struct Shell {
  state: State,
  commands: UnboundedSender<Command>,
  search: Entity<InputState>,
  icons: Entity<IconCache>,
  _subscriptions: Vec<Subscription>,
  _inbox: Task<()>,
}

impl Shell {
  /// Builds the view and starts the app. Commands go out on `commands`, and
  /// every message arriving on `inbox` is applied to the state.
  pub fn new(
    state: State,
    commands: UnboundedSender<Command>,
    mut inbox: UnboundedReceiver<Msg>,
    window: &mut Window,
    cx: &mut Context<Self>,
  ) -> Self {
    let search = cx.new(|cx| InputState::new(window, cx).placeholder(text::SEARCH_PLACEHOLDER));
    let searched = cx.subscribe_in(
      &search,
      window,
      |this: &mut Self, input, event: &InputEvent, _, cx| {
        if let InputEvent::Change = event {
          let query = input.read(cx).value().to_string();

          this.dispatch(Msg::SearchChanged(query), cx);
        }
      },
    );
    let _inbox = cx.spawn(async move |this, cx| {
      while let Some(msg) = inbox.next().await {
        if this.update(cx, |this, cx| this.dispatch(msg, cx)).is_err() {
          break;
        }
      }
    });
    let mut shell = Self {
      state,
      commands,
      search,
      icons: cx.new(|_| IconCache::new()),
      _subscriptions: vec![searched],
      _inbox,
    };

    shell.dispatch(Msg::Started, cx);

    shell
  }

  pub fn state(&self) -> &State {
    &self.state
  }

  fn dispatch(&mut self, msg: Msg, cx: &mut Context<Self>) {
    for effect in rivet_core::update(&mut self.state, msg) {
      match effect {
        Effect::Run(command) => {
          self.commands.unbounded_send(command).ok();
        }
        Effect::ChooseFolder => self.choose_folder(cx),
      }
    }

    cx.notify();
  }

  fn choose_folder(&mut self, cx: &mut Context<Self>) {
    let answer = cx.prompt_for_paths(PathPromptOptions {
      files: false,
      directories: true,
      multiple: false,
      prompt: None,
    });

    cx.spawn(async move |this, cx| {
      let folder = answer
        .await
        .ok()
        .and_then(Result::ok)
        .flatten()
        .and_then(|paths| paths.into_iter().next());

      this
        .update(cx, |this, cx| this.dispatch(Msg::FolderChosen(folder), cx))
        .ok();
    })
    .detach();
  }

  fn game_name(&self, game: &str) -> SharedString {
    self
      .state
      .ecosystem
      .game(game)
      .map_or(game, |definition| definition.display_name())
      .to_string()
      .into()
  }

  fn render_picker(&self, cx: &mut Context<Self>) -> AnyElement {
    let picker = &self.state.picker;
    let games = rivet_core::picker::games(&self.state.ecosystem, &picker.query);
    let refusal = picker
      .refusal
      .as_ref()
      .map(|refusal| text::refusal(&self.game_name(&refusal.game), refusal));
    let checking = match &picker.step {
      Step::Checking { folder, .. } => Some(text::checking(folder)),
      _ => None,
    };
    let rows: Vec<_> = games
      .into_iter()
      .map(|game| {
        let slug = game.slug.clone();

        div()
          .id(SharedString::from(game.slug.clone()))
          .px_3()
          .py_2()
          .hover(|row| row.bg(rgb(0x2a2a2a)))
          .child(SharedString::from(game.display_name().to_string()))
          .on_click(cx.listener(move |this, _, _, cx| {
            this.dispatch(Msg::GamePicked(slug.clone()), cx);
          }))
          .test_support()
      })
      .collect();
    let list = v_flex()
      .id("games")
      .flex_1()
      .overflow_y_scroll()
      .when(rows.is_empty(), |list| list.child(text::NO_GAME_MATCHES))
      .children(rows);

    v_flex()
      .size_full()
      .p_4()
      .gap_3()
      .child(div().text_xl().child(text::PICKER_TITLE))
      .children(
        self
          .state
          .settings_error
          .as_ref()
          .map(|error| failure(text::settings_error(error))),
      )
      .children(refusal.map(failure))
      .children(checking)
      .child(Input::new(&self.search))
      .child(list)
      .into_any_element()
  }

  fn render_game(&self, game: &str, cx: &mut Context<Self>) -> AnyElement {
    let folder = self.state.settings.folder(game).map(text::installed_in);

    v_flex()
      .size_full()
      .p_4()
      .gap_3()
      .child(
        h_flex()
          .justify_between()
          .child(div().text_xl().child(self.game_name(game)))
          .child(
            h_flex()
              .gap_2()
              .child(
                Button::new("refresh")
                  .label(text::REFRESH)
                  .on_click(cx.listener(|this, _, _, cx| this.dispatch(Msg::RefreshPressed, cx))),
              )
              .child(
                Button::new("switch-game")
                  .label(text::SWITCH_GAME)
                  .on_click(
                    cx.listener(|this, _, _, cx| this.dispatch(Msg::SwitchGamePressed, cx)),
                  ),
              ),
          ),
      )
      .children(folder)
      .child(self.render_launch(cx))
      .children(
        self
          .state
          .settings_error
          .as_ref()
          .map(|error| failure(text::settings_error(error))),
      )
      .children(
        self
          .state
          .profile
          .as_ref()
          .and_then(|profile| match &profile.status {
            ProfileStatus::Failed(reason) => {
              Some(failure(text::profile_failed(&profile.name, reason)))
            }
            _ => None,
          }),
      )
      .children(
        self
          .state
          .operation
          .as_ref()
          .map(|operation| render_operation(operation, cx)),
      )
      .children(
        self
          .state
          .confirmation
          .as_ref()
          .map(|confirmation| render_confirmation(confirmation, cx)),
      )
      .children(
        self
          .state
          .browse
          .as_ref()
          .map(|browse| render_browse(browse, &self.icons, cx)),
      )
      .into_any_element()
  }

  /// The launch buttons, which are always shown, with what the user should
  /// know before or after pressing one.
  fn render_launch(&self, cx: &mut Context<Self>) -> AnyElement {
    let can_launch = self.state.can_launch();
    let launcher = self.state.launcher.as_ref();
    let attempt = self.state.launch.as_ref().map(|attempt| {
      let launched = self.game_name(&attempt.game);

      match &attempt.status {
        AttemptStatus::Preparing => div()
          .child(text::launching(&launched, attempt.mode))
          .into_any_element(),
        AttemptStatus::Started => div()
          .child(text::launched(&launched, attempt.mode))
          .into_any_element(),
        AttemptStatus::Failed(launch_failure) => {
          failure(text::launch_failed(&launched, attempt.mode, launch_failure)).into_any_element()
        }
      }
    });
    let missing_loader = self.state.missing_loader().and_then(|loader| {
      let profile = self.state.profile.as_ref()?;

      Some(
        div()
          .id("missing-loader")
          .child(failure(text::missing_loader(&profile.name, loader)))
          .test_support(),
      )
    });
    let steam = launcher
      .and_then(|launcher| launcher.steam.as_ref())
      .map(|steam| render_steam_option(steam, cx));

    v_flex()
      .gap_2()
      .child(
        h_flex()
          .gap_2()
          .child(
            Button::new("launch-modded")
              .label(text::LAUNCH_MODDED)
              .primary()
              .disabled(!can_launch)
              .on_click(cx.listener(|this, _, _, cx| {
                this.dispatch(Msg::LaunchPressed(LaunchMode::Modded), cx);
              })),
          )
          .child(
            Button::new("launch-vanilla")
              .label(text::LAUNCH_VANILLA)
              .disabled(!can_launch)
              .on_click(cx.listener(|this, _, _, cx| {
                this.dispatch(Msg::LaunchPressed(LaunchMode::Vanilla), cx);
              })),
          ),
      )
      .children(attempt)
      .children(missing_loader)
      .children(steam)
      .into_any_element()
  }
}

/// The line to paste into Steam's launch options, with a copy button, and
/// whether Steam holds it.
fn render_steam_option(steam: &SteamOption, cx: &mut Context<Shell>) -> AnyElement {
  let (id, status) = match &steam.status {
    OptionStatus::Checking => (
      "launch-option-checking",
      text::STEAM_OPTION_CHECKING.to_string(),
    ),
    OptionStatus::Set => ("launch-option-set", text::STEAM_OPTION_SET.to_string()),
    OptionStatus::NotSet => (
      "launch-option-not-set",
      text::STEAM_OPTION_NOT_SET.to_string(),
    ),
    OptionStatus::Unknown(reason) => ("launch-option-unknown", text::steam_option_unknown(reason)),
  };
  let line = steam.line.clone().map(|line| {
    let copied = line.clone();

    h_flex()
      .gap_2()
      .child(
        div()
          .flex_1()
          .p_2()
          .font_family("monospace")
          .bg(rgb(0x1e1e1e))
          .child(SharedString::from(line)),
      )
      .child(
        Button::new("copy-launch-option")
          .label(text::COPY)
          .on_click(move |_, _, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(copied.clone()));
          }),
      )
  });

  v_flex()
    .gap_2()
    .p_3()
    .border_1()
    .border_color(rgb(0x606060))
    .child(text::STEAM_OPTION_LEAD)
    .children(line)
    .child(
      h_flex()
        .gap_2()
        .justify_between()
        .child(div().id(id).child(status).test_support())
        .child(
          Button::new("check-launch-option")
            .label(text::CHECK_AGAIN)
            .small()
            .disabled(steam.status == OptionStatus::Checking)
            .on_click(
              cx.listener(|this, _, _, cx| this.dispatch(Msg::SteamOptionCheckPressed, cx)),
            ),
        ),
    )
    .into_any_element()
}

fn render_confirmation(confirmation: &Confirmation, cx: &mut Context<Shell>) -> AnyElement {
  let dependencies: Vec<_> = confirmation
    .dependencies
    .iter()
    .map(|package| div().pl_3().child(text::planned(package)))
    .collect();
  let lead = if dependencies.is_empty() {
    text::NO_DEPENDENCIES
  } else {
    text::WITH_DEPENDENCIES
  };
  let can_confirm = confirmation.can_confirm();

  v_flex()
    .gap_2()
    .p_3()
    .border_1()
    .border_color(rgb(0x606060))
    .child(
      div()
        .font_weight(FontWeight::SEMIBOLD)
        .child(text::confirm_install(&confirmation.package)),
    )
    .child(lead)
    .children(dependencies)
    .when(!can_confirm, |panel| {
      panel.child(failure(text::missing_dependencies(&confirmation.missing)))
    })
    .child(
      h_flex()
        .gap_2()
        .when(can_confirm, |buttons| {
          buttons.child(
            Button::new("confirm-install")
              .label(text::INSTALL)
              .primary()
              .on_click(cx.listener(|this, _, _, cx| this.dispatch(Msg::InstallConfirmed, cx))),
          )
        })
        .child(
          Button::new("dismiss-install")
            .label(text::CANCEL)
            .on_click(cx.listener(|this, _, _, cx| this.dispatch(Msg::InstallDismissed, cx))),
        ),
    )
    .into_any_element()
}

fn render_operation(operation: &Operation, cx: &mut Context<Shell>) -> AnyElement {
  let dismiss = Button::new("dismiss-operation")
    .label(text::DISMISS)
    .on_click(cx.listener(|this, _, _, cx| this.dispatch(Msg::OperationDismissed, cx)));

  match &operation.status {
    OperationStatus::Running(progress) => div()
      .child(text::installing(&operation.package, progress.as_ref()))
      .into_any_element(),
    OperationStatus::Finished { installed } => h_flex()
      .gap_2()
      .justify_between()
      .child(text::installed(&operation.package, installed))
      .child(dismiss)
      .into_any_element(),
    OperationStatus::Failed { failed, installed } => v_flex()
      .gap_2()
      .child(failure(text::install_failed(
        &operation.package,
        failed,
        installed,
      )))
      .child(dismiss)
      .into_any_element(),
  }
}

/// The install button of a package row, or a mark when the open profile
/// already holds the package.
fn install_action(
  full_name: &str,
  installed: bool,
  can_install: bool,
  cx: &mut Context<Shell>,
) -> AnyElement {
  if installed {
    return div()
      .id(SharedString::from(format!("installed-{full_name}")))
      .flex_none()
      .text_sm()
      .child(text::INSTALLED)
      .test_support()
      .into_any_element();
  }

  let package = full_name.to_string();

  Button::new(SharedString::from(format!("install-{full_name}")))
    .label(text::INSTALL)
    .small()
    .disabled(!can_install)
    .on_click(cx.listener(move |this, _, _, cx| {
      this.dispatch(Msg::InstallPressed(package.clone()), cx);
    }))
    .into_any_element()
}

/// Fixed so the list can be virtualized: only the rows in view are built.
const ROW_HEIGHT: f32 = 64.0;
const ICON_SIZE: f32 = 48.0;

fn render_browse(
  browse: &Browse,
  icons: &Entity<IconCache>,
  cx: &mut Context<Shell>,
) -> AnyElement {
  match browse.view() {
    View::Loading => div().child(text::LOADING_PACKAGES).into_any_element(),
    View::Unavailable(reason) => failure(text::unavailable(reason)).into_any_element(),
    View::List { refreshing, stale } => {
      let count = browse.rows.len();
      let rows = uniform_list(
        "packages",
        count,
        cx.processor(|this, range: Range<usize>, _, cx| {
          let state = &this.state;
          let Some(browse) = state.browse.as_ref() else {
            return Vec::new();
          };
          let Some(index) = browse.index.as_ref() else {
            return Vec::new();
          };
          let can_install = state.can_install();

          browse.rows[range]
            .iter()
            .filter_map(|&row| PackageRow::new(&index.0, row))
            .map(|row| {
              let full_name = row.full_name().unwrap_or_default();
              let action =
                install_action(full_name, state.is_installed(full_name), can_install, cx);

              package_row(row, action)
            })
            .collect()
        }),
      )
      .flex_1();

      v_flex()
        .flex_1()
        .gap_2()
        .overflow_hidden()
        .image_cache(icons.clone())
        .children(stale.map(|reason| {
          div()
            .id("stale")
            .child(failure(text::stale(reason)))
            .test_support()
        }))
        .when(refreshing, |list| list.child(text::REFRESHING))
        .when(count == 0, |list| list.child(text::NO_PACKAGES))
        .child(rows)
        .into_any_element()
    }
  }
}

fn package_row(row: PackageRow<'_>, action: AnyElement) -> AnyElement {
  let muted = rgb(0x999999);
  let icon = match row.icon() {
    Some(url) => img(SharedString::from(url.to_string()))
      .size(px(ICON_SIZE))
      .flex_none()
      .into_any_element(),
    None => div().size(px(ICON_SIZE)).flex_none().into_any_element(),
  };
  let text = |field: Option<&str>| SharedString::from(field.unwrap_or_default().to_string());

  h_flex()
    .id(text(row.full_name()))
    .h(px(ROW_HEIGHT))
    .w_full()
    .px_3()
    .gap_3()
    .child(icon)
    .child(
      v_flex()
        .flex_1()
        .overflow_hidden()
        .child(
          h_flex()
            .gap_2()
            .child(
              div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(text(row.name())),
            )
            .child(div().text_sm().text_color(muted).child(text::byline(
              row.owner().unwrap_or_default(),
              row.version_number().unwrap_or_default(),
            ))),
        )
        .child(
          div()
            .truncate()
            .text_sm()
            .text_color(muted)
            .child(text(row.description())),
        ),
    )
    .child(action)
    .test_support()
    .into_any_element()
}

fn failure(failure: text::Failure) -> impl IntoElement {
  v_flex()
    .gap_1()
    .p_3()
    .border_1()
    .border_color(rgb(0xb04040))
    .child(
      div()
        .font_weight(FontWeight::SEMIBOLD)
        .child(failure.headline),
    )
    .child(failure.detail)
    .child(failure.next)
}

impl Render for Shell {
  fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    match self.state.screen.clone() {
      Screen::Starting => v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .child(text::LOADING)
        .into_any_element(),
      Screen::Picker => self.render_picker(cx),
      Screen::Game(game) => self.render_game(&game, cx),
    }
  }
}
