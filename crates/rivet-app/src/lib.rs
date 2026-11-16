//! The gpui shell: draws [`State`], turns input into [`Msg`]s, runs the
//! [`Effect`]s that need the window, and hands the rest to the commands
//! thread.

mod icons;
mod modal;
pub mod palette;
pub mod text;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{
  Disableable as _, Icon, IconName, Selectable as _, Sizable as _, Theme, WindowExt as _, h_flex,
  v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  Anchor, AnyElement, App, AppContext as _, ClickEvent, ClipboardItem, Context, Div, Entity,
  FontWeight, Hsla, InteractiveElement as _, IntoElement, ParentElement as _, PathPromptOptions,
  Render, SharedString, Stateful, StatefulInteractiveElement as _, Styled as _, Subscription, Task,
  TestSupportExt as _, WeakEntity, Window, div, img, px, uniform_list,
};
use icons::IconCache;
use modal::Modal;
use palette::Palette;
use rivet_core::browse::{Browse, SortKey, View};
use rivet_core::install::{InstalledPackage, Operation, OperationStatus, Profile, ProfileStatus};
use rivet_core::launch::{AttemptStatus, OptionStatus, SteamOption};
use rivet_core::manage::{Sort, Tab};
use rivet_core::notice::{Level, Notice};
use rivet_core::picker::Step;
use rivet_core::readme::{Package, Readme, Status};
use rivet_core::switcher::Naming;
use rivet_core::updates::Badges;
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
  /// Notices shown as toasts on the next render, since showing one needs the
  /// window.
  notices: Vec<Notice>,
  /// The modal open over the window, kept in step with
  /// [`modal::wanted_modal`] on each render.
  modal: Option<Modal>,
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
    let focused = cx.observe_window_activation(window, |this: &mut Self, window, cx| {
      if window.is_window_active() {
        this.dispatch(Msg::WindowFocused, cx);
      }
    });
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
    Theme::update(cx, |theme| {
      let margins = &mut theme.notification.margins;

      margins.bottom = px(STATUS_BAR_HEIGHT) + margins.right;
    });

    let mut shell = Self {
      state,
      commands,
      search,
      package_search,
      profile_name,
      fill_profile_name: None,
      close_window: false,
      notices: Vec::new(),
      modal: None,
      installed_search,
      icons: cx.new(|_| IconCache::new()),
      _subscriptions: vec![
        searched,
        package_searched,
        named,
        installed_searched,
        focused,
      ],
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
        Effect::CopyToClipboard(text) => {
          cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        Effect::Notify(notice) => self.notices.push(notice),
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

  /// `notice` as a toast: an icon and outline for its level, its headline,
  /// and its detail. Errors stay until closed, and the rest go on their own.
  /// Toasts rise in the bottom corner above the status bar, clear of the
  /// buttons along the top.
  fn toast(&self, notice: &Notice, cx: &mut Context<Self>) -> Notification {
    let palette = Palette::new(cx);
    let level = notice.level();
    let (id, icon, accent) = match level {
      Level::Info => ("notice-info", IconName::Info, palette.info),
      Level::Success => ("notice-success", IconName::CircleCheck, palette.success),
      Level::Warning => ("notice-warning", IconName::TriangleAlert, palette.warning),
      Level::Error => ("notice-error", IconName::CircleX, palette.error),
    };
    let told = text::notice(notice, |game| self.game_name(game).to_string());
    let detail = told.detail;

    Notification::new()
      .placement(Anchor::BottomRight)
      .title(told.title)
      .icon(Icon::new(icon).text_color(accent))
      .bg(palette.toast)
      .text_color(palette.text)
      .border_color(accent)
      .autohide(level != Level::Error)
      .content(move |_, _, _| {
        v_flex()
          .id(id)
          .text_sm()
          .children(detail.iter().map(|line| div().child(line.clone())))
          .test_support()
          .into_any_element()
      })
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
      .child(Input::new(&self.search))
      .child(list)
      .into_any_element()
  }

  /// The bar along the bottom of the window, [`STATUS_BAR_HEIGHT`] tall
  /// whatever it shows, so nothing above it moves. It shows the running
  /// operation with how many wait behind it, or else what the app is busy
  /// with, and is empty when neither.
  fn render_status_bar(&self, cx: &mut Context<Self>) -> AnyElement {
    let palette = Palette::new(cx);
    let queue = &self.state.queue;
    let shown = match queue.running() {
      Some(operation) => Some(render_running(
        operation,
        &self.game_name(&operation.game),
        queue.waiting(),
        cx,
      )),
      None => self.status_note().map(|note| {
        div()
          .id("status-note")
          .min_w_0()
          .truncate()
          .text_sm()
          .text_color(palette.muted_text)
          .child(note)
          .test_support()
          .into_any_element()
      }),
    };

    h_flex()
      .id("status-bar")
      .flex_none()
      .h(px(STATUS_BAR_HEIGHT))
      .px_4()
      .overflow_hidden()
      .border_t_1()
      .border_color(palette.border)
      .children(shown)
      .test_support()
      .into_any_element()
  }

  /// What the app is busy with outside the queue: starting a game, changing
  /// a profile, finding a game from the picker, refreshing the package list,
  /// or checking Steam's launch options, first match in that order. `None`
  /// when it is busy with none.
  fn status_note(&self) -> Option<String> {
    let state = &self.state;
    let launching = state
      .launch
      .as_ref()
      .filter(|attempt| attempt.status == AttemptStatus::Preparing)
      .map(|attempt| text::launching(&self.game_name(&attempt.game), attempt.mode));
    let changing = || {
      state
        .switcher
        .as_ref()
        .and_then(|switcher| switcher.changing.as_ref())
        .map(text::changing)
    };
    let finding = || match (&state.screen, &state.picker.step) {
      (Screen::Picker, Step::Locating(game)) => Some(text::locating(&self.game_name(game))),
      (Screen::Picker, Step::Checking { folder, .. }) => Some(text::checking(folder)),
      _ => None,
    };
    let refreshing = || {
      let browse = state.browse.as_ref()?;

      matches!(browse.view(), View::List { refreshing: true }).then(|| text::REFRESHING.to_string())
    };

    let checking_steam = || {
      let steam = state.launcher.as_ref()?.steam.as_ref()?;

      steam
        .checking
        .then(|| text::STEAM_OPTION_CHECKING.to_string())
    };

    launching
      .or_else(changing)
      .or_else(finding)
      .or_else(refreshing)
      .or_else(checking_steam)
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
      .child(self.render_header(game, cx))
      .children(folder)
      .children(self.render_profile_notes(cx))
      .children(self.render_launch_notes(cx))
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

  /// The open profile's packages with their badges, with search, sort,
  /// updating every outdated one, and the bulk toggles.
  fn render_installed(&self, cx: &mut Context<Self>) -> AnyElement {
    let state = &self.state;
    let can_change = state.can_manage();
    let manage = &state.manage;
    let badges = state.badges();
    let any_outdated = badges.values().any(|badge| badge.update.is_some());
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
        Button::new("update-all")
          .label(text::UPDATE_ALL)
          .small()
          .disabled(!can_change || !any_outdated)
          .on_click(cx.listener(|this, _, _, cx| this.dispatch(Msg::UpdateAllPressed, cx))),
      )
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
    let installed = state.profile.as_ref().and_then(Profile::installed);
    let rows: Vec<AnyElement> = installed
      .map(|installed| rivet_core::manage::rows(installed, &manage.query, manage.sort))
      .unwrap_or_default()
      .into_iter()
      .map(|(full_name, package)| {
        let toggle = state.can_toggle(full_name);
        let selected = is_selected(state.readme.as_ref(), full_name);
        let badges = badges.get(full_name);

        installed_row(full_name, package, badges, toggle, can_change, selected, cx)
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
      .children(empty)
      .child(
        v_flex()
          .id("installed-packages")
          .flex_1()
          .overflow_y_scroll()
          .children(rows)
          .test_support(),
      )
      .into_any_element()
  }

  /// The bar along the top of a game: the game, with switching to another,
  /// the selected profile with the others to pick from, the actions on the
  /// selected profile, and the launch buttons.
  fn render_header(&self, game: &str, cx: &mut Context<Self>) -> AnyElement {
    let palette = Palette::new(cx);
    let shell = cx.weak_entity();
    let can_launch = self.state.can_launch();
    let game_menu = Button::new("game-menu")
      .label(self.game_name(game))
      .ghost()
      .dropdown_caret(true)
      .dropdown_menu({
        let shell = shell.clone();

        move |menu, _, _| {
          let shell = shell.clone();

          menu.item(menu_item("switch-game", text::SWITCH_GAME, None).on_click(
            move |_, window, cx| {
              shell
                .update(cx, |this, cx| {
                  this.dispatch(Msg::SwitchGamePressed, cx);
                  this.empty_package_search(window, cx);
                })
                .ok();
            },
          ))
        }
      });
    let profile_menus = self.render_profile_menus(&shell).map(|menus| {
      h_flex()
        .gap_1()
        .items_center()
        .child(
          div()
            .text_color(palette.muted_text)
            .child(text::HEADER_SEPARATOR),
        )
        .children(menus)
    });

    h_flex()
      .id("header")
      .gap_1()
      .items_center()
      .child(game_menu)
      .children(profile_menus)
      .child(div().flex_1())
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
      .test_support()
      .into_any_element()
  }

  /// The profile dropdown, which picks a profile or starts a new one, and
  /// the menu of actions on the selected profile. `None` until a profile is
  /// selected.
  fn render_profile_menus(&self, shell: &WeakEntity<Self>) -> Option<[AnyElement; 2]> {
    let state = &self.state;

    state.switcher.as_ref()?;

    let selected = state.profile.as_ref()?.name.clone();
    let can_switch = state.can_switch_profile();
    let can_rename_or_delete = state.can_rename_or_delete_profile();
    let can_duplicate = state.can_duplicate_profile();
    let profiles: Vec<(String, Option<usize>)> = state
      .profile_names()
      .into_iter()
      .map(|name| (name.to_string(), state.mod_count(name)))
      .collect();
    let picker = Button::new("profile-menu")
      .label(selected.clone())
      .ghost()
      .dropdown_caret(true)
      .dropdown_menu({
        let shell = shell.clone();

        move |menu, _, cx| {
          let muted = Palette::new(cx).muted_text;

          profiles
            .iter()
            .fold(menu, |menu, (name, count)| {
              menu.item(
                profile_item(name, *count, muted)
                  .checked(*name == selected)
                  .disabled(!can_switch)
                  .on_click(send(&shell, Msg::ProfilePicked(name.clone()))),
              )
            })
            .separator()
            .item(
              menu_item("new-profile", text::NEW_PROFILE, None)
                .disabled(!can_switch)
                .on_click(send(&shell, Msg::ProfileNamePressed(Naming::Create))),
            )
        }
      });
    let actions = Button::new("profile-actions")
      .icon(IconName::Ellipsis)
      .ghost()
      .tooltip(text::PROFILE_ACTIONS)
      .dropdown_menu({
        let shell = shell.clone();

        move |menu: PopupMenu, _, cx| {
          let danger = can_rename_or_delete.then(|| Palette::new(cx).danger_text);

          menu
            .item(
              menu_item("rename-profile", text::RENAME_PROFILE, None)
                .disabled(!can_rename_or_delete)
                .on_click(send(&shell, Msg::ProfileNamePressed(Naming::Rename))),
            )
            .item(
              menu_item("duplicate-profile", text::DUPLICATE_PROFILE, None)
                .disabled(!can_duplicate)
                .on_click(send(&shell, Msg::ProfileNamePressed(Naming::Duplicate))),
            )
            .separator()
            .item(
              menu_item("delete-profile", text::DELETE_PROFILE, danger)
                .disabled(!can_rename_or_delete)
                .on_click(send(&shell, Msg::DeleteProfilePressed)),
            )
        }
      });

    Some([picker.into_any_element(), actions.into_any_element()])
  }

  /// Why the profiles could not be listed, or `None` when they were.
  fn render_profile_notes(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
    let palette = Palette::new(cx);
    let reason = self.state.switcher.as_ref()?.list_failure.as_ref()?;

    Some(failure(palette, text::profiles_unlisted(reason)).into_any_element())
  }

  /// What the user must fix before a launch can work, or `None` when there
  /// is nothing to say.
  fn render_launch_notes(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
    let palette = Palette::new(cx);
    let launcher = self.state.launcher.as_ref();
    let missing_loader = self.state.missing_loader().and_then(|loader| {
      let profile = self.state.profile.as_ref()?;

      Some(
        div()
          .id("missing-loader")
          .child(failure(
            palette,
            text::missing_loader(&profile.name, loader),
          ))
          .test_support()
          .into_any_element(),
      )
    });
    let steam = launcher
      .and_then(|launcher| launcher.steam.as_ref())
      .and_then(|steam| render_steam_option(steam, cx));
    let notes: Vec<AnyElement> = [missing_loader, steam].into_iter().flatten().collect();

    if notes.is_empty() {
      return None;
    }

    Some(v_flex().gap_2().children(notes).into_any_element())
  }
}

/// A click handler that hands `msg` to the shell, while it is open.
fn send(
  shell: &WeakEntity<Shell>,
  msg: Msg,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + use<> {
  let shell = shell.clone();

  move |_, _, cx| {
    shell
      .update(cx, |this, cx| this.dispatch(msg.clone(), cx))
      .ok();
  }
}

/// A menu item reading `label`, found in tests by `id`, in `color` when
/// given and the theme's text color otherwise.
fn menu_item(id: &'static str, label: &'static str, color: Option<Hsla>) -> PopupMenuItem {
  PopupMenuItem::element(move |_, _| {
    div()
      .id(id)
      .when_some(color, |item, color| item.text_color(color))
      .child(label)
      .test_support()
  })
}

/// A profile in the profile dropdown: its name, and how many packages it
/// holds when that is known.
fn profile_item(name: &str, count: Option<usize>, muted: Hsla) -> PopupMenuItem {
  let name = name.to_string();

  PopupMenuItem::element(move |_, _| {
    h_flex()
      .id(SharedString::from(format!("pick-profile-{name}")))
      .w_full()
      .gap_4()
      .justify_between()
      .child(name.clone())
      .children(count.map(|count| {
        div()
          .id(SharedString::from(format!("mod-count-{name}")))
          .text_sm()
          .text_color(muted)
          .child(text::mod_count(count))
          .test_support()
      }))
      .test_support()
  })
}

/// The line to paste into Steam's launch options, with a copy button, and
/// what the last check of Steam found, or `None` when
/// [`SteamOption::needs_attention`] says nothing is wrong. A check under way
/// keeps the last finding, so the banner keeps its size.
fn render_steam_option(steam: &SteamOption, cx: &mut Context<Shell>) -> Option<AnyElement> {
  if !steam.needs_attention() {
    return None;
  }

  let palette = Palette::new(cx);
  let (id, status) = match &steam.found {
    Some(OptionStatus::Unknown(reason)) => {
      ("launch-option-unknown", text::steam_option_unknown(reason))
    }
    _ => (
      "launch-option-not-set",
      text::STEAM_OPTION_NOT_SET.to_string(),
    ),
  };
  let line = steam.line.clone().map(|line| {
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
          .on_click(cx.listener(|this, _, _, cx| this.dispatch(Msg::LaunchLineCopyPressed, cx))),
      )
  });

  Some(
    v_flex()
      .id("launch-option")
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
          .child(div().id(id).flex_1().min_w_0().child(status).test_support())
          .child(
            Button::new("check-launch-option")
              .label(text::CHECK_AGAIN)
              .small()
              .disabled(steam.checking)
              .on_click(
                cx.listener(|this, _, _, cx| this.dispatch(Msg::SteamOptionCheckPressed, cx)),
              ),
          ),
      )
      .test_support()
      .into_any_element(),
  )
}

/// The running operation on one line of the status bar: the game and
/// profile it changes, what it is doing, how many operations wait behind it,
/// and Cancel until it has been asked to stop.
fn render_running(
  operation: &Operation,
  game: &str,
  waiting: usize,
  cx: &mut Context<Shell>,
) -> AnyElement {
  let palette = Palette::new(cx);
  let id = operation.id;
  let action = &operation.action;
  let (doing_id, doing) = match &operation.status {
    OperationStatus::Cancelling(progress) => (
      "operation-stopping",
      text::stopping(action, progress.as_ref()),
    ),
    OperationStatus::Running(progress) => (
      "operation-running",
      text::running(action, progress.as_ref()),
    ),
    OperationStatus::Queued => ("operation-running", text::running(action, None)),
  };
  let cancel = operation.can_cancel().then(|| {
    Button::new(SharedString::from(format!("cancel-operation-{id}")))
      .label(text::CANCEL)
      .xsmall()
      .on_click(cx.listener(move |this, _, _, cx| {
        this.dispatch(Msg::OperationCancelPressed(id), cx);
      }))
  });
  let waiting = (waiting > 0).then(|| {
    div()
      .id("waiting-count")
      .flex_none()
      .text_color(palette.muted_text)
      .child(text::waiting(waiting))
      .test_support()
  });

  h_flex()
    .id(SharedString::from(format!("operation-{id}")))
    .size_full()
    .min_w_0()
    .gap_3()
    .text_sm()
    .child(
      div()
        .min_w_0()
        .truncate()
        .text_color(palette.muted_text)
        .child(text::place(game, &operation.profile)),
    )
    .child(
      div()
        .id(doing_id)
        .flex_1()
        .min_w_0()
        .truncate()
        .child(doing)
        .test_support(),
    )
    .children(waiting)
    .children(cancel)
    .test_support()
    .into_any_element()
}

/// The Browse and Installed tabs, with refreshing the package list at the
/// far end.
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
    .child(div().flex_1())
    .child(
      Button::new("refresh")
        .label(text::REFRESH)
        .ghost()
        .on_click(cx.listener(|this, _, _, cx| this.dispatch(Msg::RefreshPressed, cx))),
    )
    .into_any_element()
}

/// One package of the Installed tab: its name, author, version, badges,
/// whether it is enabled, and the buttons that change it. Clicking it
/// selects it.
fn installed_row(
  full_name: &str,
  package: &InstalledPackage,
  badges: Option<&Badges>,
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
  let badge = |kind: &str, tag: Tag, label: String| {
    div()
      .id(SharedString::from(format!("{kind}-badge-{full_name}")))
      .flex_none()
      .child(tag.small().child(label))
      .test_support()
  };
  let outdated = badges.is_some_and(|badges| badges.update.is_some());
  let tags = badges
    .into_iter()
    .flat_map(|badges| {
      [
        badges
          .update
          .as_deref()
          .map(|version| badge("update", Tag::info(), text::update_available(version))),
        badges
          .deprecated
          .then(|| badge("deprecated", Tag::warning(), text::DEPRECATED.to_string())),
        (!badges.missing.is_empty()).then(|| {
          badge(
            "missing",
            Tag::danger(),
            text::missing_badge(&badges.missing),
          )
        }),
      ]
    })
    .flatten();
  let update = outdated.then(|| {
    let msg = Msg::UpdatePressed(full_name.to_string());

    Button::new(SharedString::from(format!("update-{full_name}")))
      .label(text::UPDATE)
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
    .children(tags)
    .child(
      div()
        .id(SharedString::from(format!("{state_id}-{full_name}")))
        .flex_none()
        .text_sm()
        .child(state_label)
        .test_support(),
    )
    .children(update)
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
/// Tall enough for one line of small text and an extra-small button.
const STATUS_BAR_HEIGHT: f32 = 36.0;
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
    View::List { .. } => {
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

    for notice in std::mem::take(&mut self.notices) {
      let toast = self.toast(&notice, cx);

      window.defer(cx, move |window, cx| window.push_notification(toast, cx));
    }

    self.sync_modal(window, cx);

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
    let status_bar = (self.state.screen != Screen::Starting).then(|| self.render_status_bar(cx));

    v_flex()
      .size_full()
      .child(div().w_full().flex_1().overflow_hidden().child(screen))
      .children(status_bar)
  }
}
