//! The gpui shell: draws [`State`], turns input into [`Msg`]s, runs the
//! [`Effect`]s that need the window, and hands the rest to the commands
//! thread.

mod icons;
pub mod palette;
pub mod text;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{Disableable as _, Selectable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  AnyElement, AppContext as _, ClipboardItem, Context, Div, Entity, FontWeight, Hsla,
  InteractiveElement as _, IntoElement, ParentElement as _, PathPromptOptions, Render,
  SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, Subscription, Task,
  TestSupportExt as _, Window, div, img, px, uniform_list,
};
use icons::IconCache;
use palette::Palette;
use rivet_core::browse::{Browse, SortKey, View};
use rivet_core::install::{
  Confirmation, InstalledPackage, Operation, OperationStatus, Profile, ProfileStatus,
};
use rivet_core::launch::{AttemptStatus, OptionStatus, SteamOption};
use rivet_core::manage::{Folders, Pending, Scope, Sort, Tab};
use rivet_core::picker::Step;
use rivet_core::queue::{CloseChoice, Closing};
use rivet_core::readme::{Package, Readme, Status};
use rivet_core::switcher::Naming;
use rivet_core::{Command, Effect, Msg, Screen, State};
use std::ops::Range;
use thunderstore_engine::profile::launch::LaunchMode;
use thunderstore_engine::query::PackageRow;

/// The root view. It owns the state and is the only place `update` is called.
pub struct Shell {
  state: State,
  commands: UnboundedSender<Command>,
  search: Entity<InputState>,
  package_search: Entity<InputState>,
  profile_name: Entity<InputState>,
  /// Text for the profile name field, put there and focused on the next
  /// render, since setting it needs the window.
  fill_profile_name: Option<String>,
  /// Set when the window should close, which it does on the next render or
  /// as the answer to its close button, since closing needs the window.
  close_window: bool,
  installed_search: Entity<InputState>,
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
    let package_search =
      cx.new(|cx| InputState::new(window, cx).placeholder(text::PACKAGE_SEARCH_PLACEHOLDER));
    let package_searched = cx.subscribe_in(
      &package_search,
      window,
      |this: &mut Self, input, event: &InputEvent, _, cx| {
        if let InputEvent::Change = event {
          let search = input.read(cx).value().to_string();

          this.dispatch(Msg::PackageSearchChanged(search), cx);
        }
      },
    );
    let profile_name = cx.new(|cx| InputState::new(window, cx));
    let named = cx.subscribe_in(
      &profile_name,
      window,
      |this: &mut Self, _, event: &InputEvent, _, cx| {
        if let InputEvent::PressEnter { .. } = event {
          this.submit_profile_name(cx);
        }
      },
    );
    let installed_search =
      cx.new(|cx| InputState::new(window, cx).placeholder(text::INSTALLED_SEARCH_PLACEHOLDER));
    let installed_searched = cx.subscribe_in(
      &installed_search,
      window,
      |this: &mut Self, input, event: &InputEvent, _, cx| {
        if let InputEvent::Change = event {
          let query = input.read(cx).value().to_string();

          this.dispatch(Msg::InstalledSearchChanged(query), cx);
        }
      },
    );
    let shell = cx.weak_entity();

    window.on_window_should_close(cx, move |_, cx| {
      shell
        .update(cx, |this, cx| this.close_requested(cx))
        .unwrap_or(true)
    });

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
      package_search,
      profile_name,
      fill_profile_name: None,
      close_window: false,
      installed_search,
      icons: cx.new(|_| IconCache::new()),
      _subscriptions: vec![searched, package_searched, named, installed_searched],
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
        Effect::FillProfileName(text) => self.fill_profile_name = Some(text),
        Effect::OpenUrl(url) => cx.open_url(&url),
        Effect::CloseWindow => self.close_window = true,
      }
    }

    cx.notify();
  }

  /// Empties the package search box to match a list whose search was reset.
  fn empty_package_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self
      .package_search
      .update(cx, |input, cx| input.set_value("", window, cx));
  }

  fn submit_profile_name(&mut self, cx: &mut Context<Self>) {
    let name = self.profile_name.read(cx).value().to_string();

    self.dispatch(Msg::ProfileNameSubmitted(name), cx);
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
    let palette = Palette::new(cx);
    let picker = &self.state.picker;
    let games = rivet_core::picker::games(&self.state.ecosystem, &picker.query);
    let refusal = picker
      .refusal
      .as_ref()
      .map(|refusal| text::refusal(&self.game_name(&refusal.game), refusal));
    let fallback = picker.fallback.as_ref().map(|fallback| {
      div()
        .id("steam-fallback")
        .child(text::fallback(
          &self.game_name(&fallback.game),
          &fallback.reason,
        ))
        .test_support()
    });
    let checking = match &picker.step {
      Step::Locating(game) => Some(text::locating(&self.game_name(game))),
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
          .map(|row| paint_row(row, false, palette))
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
          .map(|error| failure(palette, text::settings_error(error))),
      )
      .children(fallback)
      .children(refusal.map(|refusal| failure(palette, refusal)))
      .children(checking)
      .children(self.render_queue(cx))
      .child(Input::new(&self.search))
      .child(list)
      .into_any_element()
  }

  /// Every operation in the queue, on any game or profile, or `None` when it
  /// is empty.
  fn render_queue(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
    let palette = Palette::new(cx);
    let operations = &self.state.queue.operations;

    if operations.is_empty() {
      return None;
    }

    let ended = operations
      .iter()
      .any(|operation| !operation.status.is_active());
    let rows: Vec<AnyElement> = operations
      .iter()
      .map(|operation| render_operation(operation, &self.game_name(&operation.game), cx))
      .collect();
    let clear = ended.then(|| {
      Button::new("clear-queue")
        .label(text::CLEAR_ENDED)
        .small()
        .on_click(cx.listener(|this, _, _, cx| this.dispatch(Msg::QueueCleared, cx)))
    });

    Some(
      v_flex()
        .gap_2()
        .p_3()
        .border_1()
        .border_color(palette.border)
        .child(
          h_flex()
            .justify_between()
            .child(
              div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(text::QUEUE_TITLE),
            )
            .children(clear),
        )
        .child(
          v_flex()
            .id("queue")
            .gap_2()
            .max_h(px(240.0))
            .overflow_y_scroll()
            .children(rows),
        )
        .into_any_element(),
    )
  }

  /// What closing the window is waiting on: the question asked when work is
  /// in progress, or that the window closes once it ends.
  fn render_closing(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
    let palette = Palette::new(cx);
    let closing = self.state.closing?;
    let keep_open = Button::new("close-dismiss")
      .label(text::KEEP_OPEN)
      .on_click(cx.listener(|this, _, _, cx| this.dispatch(Msg::CloseDismissed, cx)));
    let panel = v_flex()
      .id("closing")
      .gap_2()
      .p_3()
      .border_1()
      .border_color(palette.danger_border);
    let panel = match closing {
      Closing::Asking => panel
        .child(
          div()
            .font_weight(FontWeight::SEMIBOLD)
            .child(text::CLOSE_HEADLINE),
        )
        .child(text::CLOSE_DETAIL)
        .child(
          h_flex()
            .gap_2()
            .child(
              Button::new("close-wait")
                .label(text::CLOSE_WAIT)
                .primary()
                .on_click(cx.listener(|this, _, _, cx| {
                  this.dispatch(Msg::CloseChosen(CloseChoice::Wait), cx);
                })),
            )
            .child(
              Button::new("close-cancel")
                .label(text::CLOSE_CANCEL)
                .on_click(cx.listener(|this, _, _, cx| {
                  this.dispatch(Msg::CloseChosen(CloseChoice::Cancel), cx);
                })),
            )
            .child(keep_open),
        ),
      Closing::Waiting => panel.child(
        h_flex()
          .gap_2()
          .justify_between()
          .child(text::CLOSING_WHEN_DONE)
          .child(keep_open),
      ),
    };

    Some(panel.test_support().into_any_element())
  }

  /// Applies a press of the window's close button, answering whether the
  /// window may close now.
  fn close_requested(&mut self, cx: &mut Context<Self>) -> bool {
    self.dispatch(Msg::CloseRequested, cx);

    std::mem::take(&mut self.close_window)
  }

  fn render_game(&self, game: &str, cx: &mut Context<Self>) -> AnyElement {
    let palette = Palette::new(cx);
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
                  .on_click(cx.listener(|this, _, window, cx| {
                    this.dispatch(Msg::SwitchGamePressed, cx);
                    this.empty_package_search(window, cx);
                  })),
              ),
          ),
      )
      .children(folder)
      .children(self.render_switcher(cx))
      .child(self.render_launch(cx))
      .children(
        self
          .state
          .settings_error
          .as_ref()
          .map(|error| failure(palette, text::settings_error(error))),
      )
      .children(
        self
          .state
          .profile
          .as_ref()
          .and_then(|profile| match &profile.status {
            ProfileStatus::Failed(reason) => Some(failure(
              palette,
              text::profile_failed(&profile.name, reason),
            )),
            _ => None,
          }),
      )
      .children(self.render_queue(cx))
      .children(
        self
          .state
          .confirmation
          .as_ref()
          .map(|confirmation| render_confirmation(confirmation, cx)),
      )
      .child(render_tabs(self.state.tab, cx))
      .child(
        div()
          .flex()
          .flex_1()
          .gap_3()
          .overflow_hidden()
          .children(match self.state.tab {
            Tab::Browse => self
              .state
              .browse
              .as_ref()
              .map(|browse| render_browse(browse, &self.package_search, &self.icons, cx)),
            Tab::Installed => Some(self.render_installed(cx)),
          })
          .child(render_readme(self.state.readme.as_ref(), cx)),
      )
      .into_any_element()
  }

  /// The open profile's packages, with search, sort, the bulk toggles, and
  /// the question a change is waiting on.
  fn render_installed(&self, cx: &mut Context<Self>) -> AnyElement {
    let state = &self.state;
    let can_change = state.can_manage();
    let manage = &state.manage;
    let sort_button = |id: &'static str, label: &'static str, sort: Sort| {
      Button::new(id)
        .label(label)
        .small()
        .when(manage.sort == sort, |button| button.primary())
        .on_click(cx.listener(move |this, _, _, cx| {
          this.dispatch(Msg::InstalledSortChosen(sort), cx);
        }))
    };
    let controls = h_flex()
      .gap_2()
      .child(div().flex_1().child(Input::new(&self.installed_search)))
      .child(sort_button(
        "installed-sort-name",
        text::SORT_NAME,
        Sort::Name,
      ))
      .child(sort_button(
        "installed-sort-author",
        text::SORT_AUTHOR,
        Sort::Author,
      ))
      .child(sort_button(
        "installed-sort-newest",
        text::SORT_NEWEST,
        Sort::Newest,
      ))
      .child(
        Button::new("enable-all")
          .label(text::ENABLE_ALL)
          .small()
          .disabled(!can_change)
          .on_click(cx.listener(|this, _, _, cx| this.dispatch(Msg::EnableAllPressed, cx))),
      )
      .child(
        Button::new("disable-all")
          .label(text::DISABLE_ALL)
          .small()
          .disabled(!can_change)
          .on_click(cx.listener(|this, _, _, cx| this.dispatch(Msg::DisableAllPressed, cx))),
      );
    let pending = manage
      .pending
      .as_ref()
      .map(|pending| render_pending(pending, cx));
    let installed = state.profile.as_ref().and_then(Profile::installed);
    let rows: Vec<AnyElement> = installed
      .map(|installed| rivet_core::manage::rows(installed, &manage.query, manage.sort))
      .unwrap_or_default()
      .into_iter()
      .map(|(full_name, package)| {
        let toggle = state.can_toggle(full_name);
        let selected = is_selected(state.readme.as_ref(), full_name);

        installed_row(full_name, package, toggle, can_change, selected, cx)
      })
      .collect();
    let empty = match installed {
      Some(installed) if installed.is_empty() => Some(text::NO_INSTALLED),
      Some(_) if rows.is_empty() => Some(text::NO_INSTALLED_MATCHES),
      _ => None,
    };

    v_flex()
      .flex_1()
      .gap_2()
      .overflow_hidden()
      .child(controls)
      .children(pending)
      .children(empty)
      .child(
        v_flex()
          .id("installed-packages")
          .flex_1()
          .overflow_y_scroll()
          .children(rows),
      )
      .into_any_element()
  }

  /// The profile switcher, which is always shown in a game: a button per
  /// profile, the actions on the selected one, and any name being asked for,
  /// deletion being confirmed, or failure.
  fn render_switcher(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
    let palette = Palette::new(cx);
    let state = &self.state;
    let switcher = state.switcher.as_ref()?;
    let selected = state.profile.as_ref().map(|profile| profile.name.as_str());
    let can_switch = state.can_switch_profile();
    let can_rename_or_delete = state.can_rename_or_delete_profile();
    let profiles: Vec<_> = state
      .profile_names()
      .into_iter()
      .map(|name| {
        let picked = name.to_string();

        Button::new(SharedString::from(format!("profile-{name}")))
          .label(name.to_string())
          .small()
          .when(Some(name) == selected, |button| button.primary())
          .disabled(!can_switch)
          .on_click(cx.listener(move |this, _, _, cx| {
            this.dispatch(Msg::ProfilePicked(picked.clone()), cx);
          }))
      })
      .collect();
    let naming = |id: &'static str, label: &'static str, naming: Naming, enabled: bool| {
      Button::new(id)
        .label(label)
        .small()
        .disabled(!enabled)
        .on_click(cx.listener(move |this, _, _, cx| {
          this.dispatch(Msg::ProfileNamePressed(naming), cx);
        }))
    };
    let actions = h_flex()
      .gap_2()
      .child(naming(
        "new-profile",
        text::NEW_PROFILE,
        Naming::Create,
        can_switch,
      ))
      .child(naming(
        "rename-profile",
        text::RENAME,
        Naming::Rename,
        can_rename_or_delete,
      ))
      .child(naming(
        "duplicate-profile",
        text::DUPLICATE,
        Naming::Duplicate,
        state.can_duplicate_profile(),
      ))
      .child(
        Button::new("delete-profile")
          .label(text::DELETE)
          .small()
          .disabled(!can_rename_or_delete)
          .on_click(cx.listener(|this, _, _, cx| this.dispatch(Msg::DeleteProfilePressed, cx))),
      );
    let form = switcher.form.as_ref().map(|form| {
      v_flex()
        .gap_2()
        .p_3()
        .border_1()
        .border_color(palette.border)
        .child(text::name_prompt(form.naming, &form.from))
        .child(Input::new(&self.profile_name))
        .children(form.refusal.as_ref().map(|refusal| {
          div()
            .id("profile-name-refused")
            .child(failure(palette, text::name_refused(refusal)))
            .test_support()
        }))
        .child(
          h_flex()
            .gap_2()
            .child(
              Button::new("save-profile-name")
                .label(text::SAVE)
                .primary()
                .on_click(cx.listener(|this, _, _, cx| this.submit_profile_name(cx))),
            )
            .child(
              Button::new("cancel-profile-name")
                .label(text::CANCEL)
                .on_click(
                  cx.listener(|this, _, _, cx| this.dispatch(Msg::ProfileNameDismissed, cx)),
                ),
            ),
        )
    });
    let deleting = switcher.deleting.as_ref().map(|name| {
      v_flex()
        .gap_2()
        .p_3()
        .border_1()
        .border_color(palette.danger_border)
        .child(
          div()
            .font_weight(FontWeight::SEMIBOLD)
            .child(text::confirm_delete(name)),
        )
        .child(text::DELETE_WARNING)
        .child(
          h_flex()
            .gap_2()
            .child(
              Button::new("confirm-delete-profile")
                .label(text::DELETE)
                .primary()
                .on_click(
                  cx.listener(|this, _, _, cx| this.dispatch(Msg::DeleteProfileConfirmed, cx)),
                ),
            )
            .child(
              Button::new("cancel-delete-profile")
                .label(text::CANCEL)
                .on_click(
                  cx.listener(|this, _, _, cx| this.dispatch(Msg::DeleteProfileDismissed, cx)),
                ),
            ),
        )
    });

    Some(
      v_flex()
        .gap_2()
        .child(
          h_flex()
            .gap_2()
            .flex_wrap()
            .child(text::PROFILE)
            .children(profiles),
        )
        .child(actions)
        .children(switcher.changing.as_ref().map(text::changing))
        .children(form)
        .children(deleting)
        .children(
          switcher
            .failure
            .as_ref()
            .map(|switcher_failure| failure(palette, text::switcher_failed(switcher_failure))),
        )
        .into_any_element(),
    )
  }

  /// The launch buttons, which are always shown, with what the user should
  /// know before or after pressing one.
  fn render_launch(&self, cx: &mut Context<Self>) -> AnyElement {
    let palette = Palette::new(cx);
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
        AttemptStatus::Failed(launch_failure) => failure(
          palette,
          text::launch_failed(&launched, attempt.mode, launch_failure),
        )
        .into_any_element(),
      }
    });
    let missing_loader = self.state.missing_loader().and_then(|loader| {
      let profile = self.state.profile.as_ref()?;

      Some(
        div()
          .id("missing-loader")
          .child(failure(
            palette,
            text::missing_loader(&profile.name, loader),
          ))
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
  let palette = Palette::new(cx);
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
          .bg(palette.code)
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
    .border_color(palette.border)
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
  let palette = Palette::new(cx);
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
    .border_color(palette.border)
    .child(
      div()
        .font_weight(FontWeight::SEMIBOLD)
        .child(text::confirm_install(&confirmation.package)),
    )
    .child(lead)
    .children(dependencies)
    .when(!can_confirm, |panel| {
      panel.child(failure(
        palette,
        text::missing_dependencies(&confirmation.missing),
      ))
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

/// One operation of the queue: the game and profile it changes, what it is
/// doing or how it ended, and the buttons that act on it.
fn render_operation(operation: &Operation, game: &str, cx: &mut Context<Shell>) -> AnyElement {
  let palette = Palette::new(cx);
  let id = operation.id;
  let action = &operation.action;
  let button = |name: &str, label: &'static str, msg: Msg| {
    Button::new(SharedString::from(format!("{name}-operation-{id}")))
      .label(label)
      .small()
      .on_click(cx.listener(move |this, _, _, cx| this.dispatch(msg.clone(), cx)))
  };
  let cancel = || button("cancel", text::CANCEL, Msg::OperationCancelPressed(id));
  let retry = || button("retry", text::RETRY, Msg::RetryPressed(id));
  let dismiss = || button("dismiss", text::DISMISS, Msg::OperationDismissed(id));
  let line = |line: String| div().child(line).into_any_element();
  let (body, buttons) = match &operation.status {
    OperationStatus::Queued => (line(text::queued(action)), vec![cancel()]),
    OperationStatus::Running(progress) => (
      line(text::running(action, progress.as_ref())),
      operation.can_cancel().then(cancel).into_iter().collect(),
    ),
    OperationStatus::Cancelling(progress) => {
      (line(text::stopping(action, progress.as_ref())), vec![])
    }
    OperationStatus::Finished { changed } => {
      (line(text::finished(action, changed)), vec![dismiss()])
    }
    OperationStatus::Failed { failed, changed } => (
      failure(palette, text::operation_failed(action, failed, changed)).into_any_element(),
      vec![retry(), dismiss()],
    ),
    OperationStatus::Cancelled { changed } => (
      line(text::cancelled(action, changed)),
      vec![retry(), dismiss()],
    ),
  };

  v_flex()
    .id(SharedString::from(format!("operation-{id}")))
    .gap_1()
    .child(
      div()
        .text_sm()
        .text_color(palette.muted_text)
        .child(text::place(game, &operation.profile)),
    )
    .child(
      h_flex()
        .gap_2()
        .child(div().flex_1().child(body))
        .child(h_flex().gap_2().flex_none().children(buttons)),
    )
    .test_support()
    .into_any_element()
}

fn render_tabs(tab: Tab, cx: &mut Context<Shell>) -> AnyElement {
  let tab_button = |id: &'static str, label: &'static str, this_tab: Tab| {
    Button::new(id)
      .label(label)
      .when(tab == this_tab, |button| button.primary())
      .on_click(cx.listener(move |this, _, _, cx| this.dispatch(Msg::TabChosen(this_tab), cx)))
  };

  h_flex()
    .gap_2()
    .child(tab_button("tab-browse", text::TAB_BROWSE, Tab::Browse))
    .child(tab_button(
      "tab-installed",
      text::TAB_INSTALLED,
      Tab::Installed,
    ))
    .into_any_element()
}

/// The question or refusal a change in the Installed tab is waiting on.
fn render_pending(pending: &Pending, cx: &mut Context<Shell>) -> AnyElement {
  let palette = Palette::new(cx);
  let cancel = Button::new("dismiss-pending")
    .label(text::CANCEL)
    .on_click(cx.listener(|this, _, _, cx| this.dispatch(Msg::PendingDismissed, cx)));
  let panel = || {
    v_flex()
      .gap_2()
      .p_3()
      .border_1()
      .border_color(palette.border)
  };
  let list = |items: Vec<String>| {
    items
      .into_iter()
      .map(|item| div().pl_3().child(item))
      .collect::<Vec<_>>()
  };

  match pending {
    Pending::Dependants {
      removal,
      package,
      dependants,
    } => panel()
      .id("dependants-question")
      .child(
        div()
          .font_weight(FontWeight::SEMIBOLD)
          .child(text::dependants_question(*removal, package)),
      )
      .children(list(dependants.clone()))
      .child(
        h_flex()
          .gap_2()
          .child(
            Button::new("package-only")
              .label(text::PACKAGE_ONLY)
              .on_click(cx.listener(|this, _, _, cx| {
                this.dispatch(Msg::DependantsChosen(Scope::PackageOnly), cx);
              })),
          )
          .child(
            Button::new("with-dependants")
              .label(text::WITH_DEPENDANTS)
              .on_click(cx.listener(|this, _, _, cx| {
                this.dispatch(Msg::DependantsChosen(Scope::WithDependants), cx);
              })),
          )
          .child(cancel),
      )
      .test_support()
      .into_any_element(),
    Pending::Checking { .. } => div().child(text::CHECKING_UNINSTALL).into_any_element(),
    Pending::Untracked { packages, folders } => panel()
      .id("untracked-question")
      .child(
        div()
          .font_weight(FontWeight::SEMIBOLD)
          .child(text::untracked_question(packages)),
      )
      .child(text::UNTRACKED_LEAD)
      .children(list(
        folders
          .iter()
          .map(|folder| folder.display().to_string())
          .collect(),
      ))
      .child(
        h_flex()
          .gap_2()
          .child(
            Button::new("delete-folders")
              .label(text::DELETE_FOLDERS)
              .on_click(cx.listener(|this, _, _, cx| {
                this.dispatch(Msg::FoldersChosen(Folders::Delete), cx);
              })),
          )
          .child(
            Button::new("keep-folders")
              .label(text::KEEP_FOLDERS)
              .on_click(cx.listener(|this, _, _, cx| {
                this.dispatch(Msg::FoldersChosen(Folders::Keep), cx);
              })),
          )
          .child(cancel),
      )
      .test_support()
      .into_any_element(),
    Pending::Refused { packages, reason } => v_flex()
      .gap_2()
      .child(failure(palette, text::uninstall_refused(packages, reason)))
      .child(
        Button::new("dismiss-pending")
          .label(text::DISMISS)
          .on_click(cx.listener(|this, _, _, cx| this.dispatch(Msg::PendingDismissed, cx))),
      )
      .into_any_element(),
  }
}

/// One package of the Installed tab: its name, author, version, whether it
/// is enabled, and the buttons that change it. Clicking it selects it.
fn installed_row(
  full_name: &str,
  package: &InstalledPackage,
  can_toggle: bool,
  can_change: bool,
  selected: bool,
  cx: &mut Context<Shell>,
) -> AnyElement {
  let selection = Package::installed(full_name, package);
  let palette = Palette::new(cx);
  let muted = palette.muted_text;
  let (state_id, state_label) = match (can_toggle, package.enabled) {
    (false, _) => ("loader", text::MOD_LOADER),
    (true, true) => ("enabled", text::ENABLED),
    (true, false) => ("disabled", text::DISABLED),
  };
  let toggle = can_toggle.then(|| {
    let (id, label, msg) = if package.enabled {
      (
        "disable",
        text::DISABLE,
        Msg::DisablePressed(full_name.to_string()),
      )
    } else {
      (
        "enable",
        text::ENABLE,
        Msg::EnablePressed(full_name.to_string()),
      )
    };

    Button::new(SharedString::from(format!("{id}-{full_name}")))
      .label(label)
      .small()
      .disabled(!can_change)
      .on_click(cx.listener(move |this, _, _, cx| this.dispatch(msg.clone(), cx)))
  });
  let uninstall = full_name.to_string();
  let item = h_flex()
    .id(SharedString::from(format!("profile-{full_name}")))
    .size_full()
    .overflow_hidden()
    .px_3()
    .gap_3()
    .map(|row| paint_row(row, selected, palette))
    .when(!package.enabled, |row| row.opacity(0.6))
    .on_click(cx.listener(move |this, _, _, cx| {
      this.dispatch(Msg::PackageSelected(selection.clone()), cx);
    }))
    .child(
      headline(
        package.name(full_name).to_string(),
        text::byline(&package.author, &package.version),
        muted,
      )
      .flex_1(),
    )
    .child(
      div()
        .id(SharedString::from(format!("{state_id}-{full_name}")))
        .flex_none()
        .text_sm()
        .child(state_label)
        .test_support(),
    )
    .children(toggle)
    .child(
      Button::new(SharedString::from(format!("uninstall-{full_name}")))
        .label(text::UNINSTALL)
        .small()
        .disabled(!can_change)
        .on_click(cx.listener(move |this, _, _, cx| {
          this.dispatch(Msg::UninstallPressed(uninstall.clone()), cx);
        })),
    )
    .test_support();

  row_slot(INSTALLED_ROW_HEIGHT, item).into_any_element()
}

/// A row's name and byline on one line, each cut short with an ellipsis
/// when the row is too narrow for it.
fn headline(name: String, byline: String, muted: Hsla) -> Div {
  h_flex()
    .gap_2()
    .overflow_hidden()
    .child(
      div()
        .id("name")
        .truncate()
        .font_weight(FontWeight::SEMIBOLD)
        .child(name)
        .test_support(),
    )
    .child(
      div()
        .id("byline")
        .truncate()
        .text_sm()
        .text_color(muted)
        .child(byline)
        .test_support(),
    )
}

/// Puts a row in a slot `height` tall that leaves a [`ROW_GAP`] to the
/// next, so every row is the same height and the row paints only its own
/// background.
fn row_slot(height: f32, row: impl IntoElement) -> Div {
  div()
    .flex_none()
    .h(px(height))
    .py(px(ROW_GAP / 2.))
    .child(row)
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
/// Tall enough for the name line and two description lines.
const ROW_HEIGHT: f32 = 96.0;
const INSTALLED_ROW_HEIGHT: f32 = 56.0;
/// The space between two rows of a list.
const ROW_GAP: f32 = 6.0;
const ICON_SIZE: f32 = 48.0;

/// Whether the readme shown is `full_name`'s.
fn is_selected(readme: Option<&Readme>, full_name: &str) -> bool {
  readme.is_some_and(|readme| readme.package.full_name() == full_name)
}

/// Paints a list row's text, and its background for whether it is selected
/// and under the pointer. Hovering the selected row keeps it selected.
fn paint_row(row: Stateful<Div>, selected: bool, palette: Palette) -> Stateful<Div> {
  let hovered = if selected {
    palette.row_selected
  } else {
    palette.row_hover
  };

  row
    .text_color(palette.text)
    .when(selected, |row| row.bg(palette.row_selected))
    .hover(move |row| row.bg(hovered))
}

/// The readme of the selected package, or why it is not shown.
fn render_readme(readme: Option<&Readme>, cx: &mut Context<Shell>) -> AnyElement {
  let palette = Palette::new(cx);
  let panel = v_flex()
    .flex_1()
    .overflow_hidden()
    .p_3()
    .border_1()
    .border_color(palette.border);
  let message = |id: &'static str, message: String| {
    div()
      .id(id)
      .text_color(palette.muted_text)
      .child(message)
      .test_support()
      .into_any_element()
  };
  let Some(readme) = readme else {
    return panel
      .child(message("readme-none", text::NO_README_SELECTED.to_string()))
      .into_any_element();
  };
  let content = match &readme.status {
    Status::Loading => message(
      "readme-loading",
      text::readme_loading(&readme.package.full_name()),
    ),
    Status::Missing => message("readme-missing", text::README_MISSING.to_string()),
    Status::Failed(reason) => message("readme-failed", text::readme_failed(reason)),
    Status::Loaded(markdown) => {
      let shell = cx.entity().downgrade();
      let view = TextView::markdown(
        SharedString::from(format!("readme-{}", readme.id)),
        markdown.clone(),
      )
      .scrollable(true)
      .selectable(true)
      .on_link_click(move |url, _, _, cx| {
        shell
          .update(cx, |this, cx| {
            this.dispatch(Msg::LinkClicked(url.to_string()), cx);
          })
          .ok();
      });

      div()
        .id("readme")
        .size_full()
        .child(view)
        .test_support()
        .into_any_element()
    }
  };

  panel.child(content).into_any_element()
}

/// The search box, sort buttons, and filters above the package list.
fn render_filters(
  browse: &Browse,
  search: &Entity<InputState>,
  cx: &mut Context<Shell>,
) -> AnyElement {
  let filters = &browse.filters;
  let best_match = (!filters.search.is_empty()).then_some(SortKey::BestMatch);
  let sorts: Vec<_> = best_match
    .into_iter()
    .chain([
      SortKey::LastUpdated,
      SortKey::Name,
      SortKey::Downloads,
      SortKey::Rating,
    ])
    .map(|key| {
      let current = (filters.sort == key).then_some(filters.direction);
      let id = match key {
        SortKey::BestMatch => "sort-best-match",
        SortKey::LastUpdated => "sort-last-updated",
        SortKey::Name => "sort-name",
        SortKey::Downloads => "sort-downloads",
        SortKey::Rating => "sort-rating",
      };

      Button::new(id)
        .label(text::sort(key, current))
        .small()
        .selected(current.is_some())
        .on_click(cx.listener(move |this, _, _, cx| this.dispatch(Msg::SortPressed(key), cx)))
    })
    .collect();
  let categories: Vec<_> = browse
    .categories
    .iter()
    .map(|category| {
      let toggled = category.clone();

      Button::new(SharedString::from(format!("category-{category}")))
        .label(SharedString::from(category.clone()))
        .xsmall()
        .selected(filters.categories.contains(category))
        .on_click(cx.listener(move |this, _, _, cx| {
          this.dispatch(Msg::CategoryToggled(toggled.clone()), cx);
        }))
    })
    .collect();

  v_flex()
    .gap_2()
    .child(
      h_flex()
        .gap_2()
        .child(
          div()
            .id("package-search")
            .flex_1()
            .child(Input::new(search))
            .test_support(),
        )
        .child(
          Button::new("clear-filters")
            .label(text::CLEAR_FILTERS)
            .disabled(!filters.is_filtering())
            .on_click(cx.listener(|this, _, window, cx| {
              this.dispatch(Msg::FiltersCleared, cx);
              this.empty_package_search(window, cx);
            })),
        ),
    )
    .child(
      h_flex()
        .gap_2()
        .child(div().text_sm().child(text::SORT_BY))
        .children(sorts)
        .child(div().flex_1())
        .child(
          Checkbox::new("show-nsfw")
            .label(text::SHOW_NSFW)
            .checked(filters.show_nsfw)
            .on_click(cx.listener(|this, show: &bool, _, cx| {
              this.dispatch(Msg::ShowNsfw(*show), cx);
            })),
        )
        .child(
          Checkbox::new("show-deprecated")
            .label(text::SHOW_DEPRECATED)
            .checked(filters.show_deprecated)
            .on_click(cx.listener(|this, show: &bool, _, cx| {
              this.dispatch(Msg::ShowDeprecated(*show), cx);
            })),
        ),
    )
    .child(h_flex().flex_wrap().gap_1().children(categories))
    .into_any_element()
}

fn render_browse(
  browse: &Browse,
  search: &Entity<InputState>,
  icons: &Entity<IconCache>,
  cx: &mut Context<Shell>,
) -> AnyElement {
  let palette = Palette::new(cx);

  match browse.view() {
    View::Loading => div()
      .flex_1()
      .child(text::LOADING_PACKAGES)
      .into_any_element(),
    View::Unavailable(reason) => failure(palette, text::unavailable(reason)).into_any_element(),
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

              let selected = is_selected(state.readme.as_ref(), full_name);

              package_row(row, action, selected, cx)
            })
            .collect()
        }),
      )
      .flex_1();
      let empty = if browse.filters.is_filtering() {
        text::NO_PACKAGE_MATCHES
      } else {
        text::NO_PACKAGES
      };
      let filters = render_filters(browse, search, cx);

      v_flex()
        .flex_1()
        .gap_2()
        .overflow_hidden()
        .image_cache(icons.clone())
        .children(stale.map(|reason| {
          div()
            .id("stale")
            .child(failure(palette, text::stale(reason)))
            .test_support()
        }))
        .when(refreshing, |list| list.child(text::REFRESHING))
        .child(filters)
        .when(count == 0, |list| list.child(empty))
        .child(rows)
        .into_any_element()
    }
  }
}

fn package_row(
  row: PackageRow<'_>,
  action: AnyElement,
  selected: bool,
  cx: &mut Context<Shell>,
) -> AnyElement {
  let palette = Palette::new(cx);
  let muted = palette.muted_text;
  let icon = match row.icon() {
    Some(url) => img(SharedString::from(url.to_string()))
      .size(px(ICON_SIZE))
      .flex_none()
      .into_any_element(),
    None => div().size(px(ICON_SIZE)).flex_none().into_any_element(),
  };
  let text = |field: Option<&str>| SharedString::from(field.unwrap_or_default().to_string());
  let selection = Package::listed(row);

  let item = h_flex()
    .id(text(row.full_name()))
    .size_full()
    .overflow_hidden()
    .px_3()
    .gap_3()
    .map(|row| paint_row(row, selected, palette))
    .when_some(selection, |row, selection| {
      row.on_click(cx.listener(move |this, _, _, cx| {
        this.dispatch(Msg::PackageSelected(selection.clone()), cx);
      }))
    })
    .child(icon)
    .child(
      v_flex()
        .flex_1()
        .overflow_hidden()
        .child(headline(
          row.name().unwrap_or_default().to_string(),
          text::byline(
            row.owner().unwrap_or_default(),
            row.version_number().unwrap_or_default(),
          ),
          muted,
        ))
        .child(
          div()
            .id("description")
            .line_clamp(2)
            .text_ellipsis()
            .text_sm()
            .text_color(muted)
            .child(text::one_paragraph(row.description().unwrap_or_default()))
            .test_support(),
        ),
    )
    .child(action)
    .test_support();

  row_slot(ROW_HEIGHT, item).into_any_element()
}

fn failure(palette: Palette, failure: text::Failure) -> impl IntoElement {
  v_flex()
    .gap_1()
    .p_3()
    .border_1()
    .border_color(palette.danger_border)
    .child(
      div()
        .font_weight(FontWeight::SEMIBOLD)
        .child(failure.headline),
    )
    .child(failure.detail)
    .child(failure.next)
}

impl Render for Shell {
  fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
    if let Some(text) = self.fill_profile_name.take() {
      self.profile_name.update(cx, |input, cx| {
        input.set_value(text, window, cx);
        input.focus(window, cx);
      });
    }

    if std::mem::take(&mut self.close_window) {
      window.remove_window();
    }

    let screen = match self.state.screen.clone() {
      Screen::Starting => v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .child(text::LOADING)
        .into_any_element(),
      Screen::Picker => self.render_picker(cx),
      Screen::Game(game) => self.render_game(&game, cx),
    };

    v_flex()
      .size_full()
      .children(self.render_closing(cx))
      .child(div().w_full().flex_1().overflow_hidden().child(screen))
  }
}
