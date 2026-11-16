//! The gpui shell: draws [`State`], turns input into [`Msg`]s, runs the
//! [`Effect`]s that need the window, and hands the rest to the commands
//! thread.

pub mod assets;
mod icons;
mod modal;
pub mod palette;
mod segmented;
pub mod text;
pub mod theme;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use gpui_kit::component::avatar::Avatar;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::description_list::DescriptionList;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::resizable::{ResizableState, h_resizable, resizable_panel};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{
  ActiveTheme as _, Disableable as _, Icon, IconName, Selectable as _, Sizable as _, Theme,
  WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  Anchor, AnyElement, App, AppContext as _, Bounds, ClickEvent, ClipboardItem, Context,
  DEFAULT_WINDOW_SIZE, Div, ElementId, Entity, FontWeight, Hsla, InteractiveElement as _,
  IntoElement, ParentElement as _, PathPromptOptions, Pixels, Render, SharedString, Size, Stateful,
  StatefulInteractiveElement as _, Styled as _, StyledImage as _, Subscription, Task,
  TestSupportExt as _, WeakEntity, Window, div, img, uniform_list,
};
use icons::IconCache;
use modal::Modal;
use palette::Palette;
use rivet_core::browse::{Browse, Filters, SortKey, View};
use rivet_core::details::{Action, Details};
use rivet_core::install::{InstalledPackage, Operation, OperationStatus, Profile, ProfileStatus};
use rivet_core::launch::{AttemptStatus, OptionStatus, SteamOption};
use rivet_core::manage::{Sort, Tab};
use rivet_core::notice::{Level, Notice};
use rivet_core::picker::Step;
use rivet_core::portability::{Exporting, Importing};
use rivet_core::readme::{Package, Readme, Status};
use rivet_core::settings::{Appearance, INTERFACE_SIZES, PickerLayout, RuntimeChoice, ThemeFamily};
use rivet_core::settings_screen::{self, Target};
use rivet_core::switcher::Naming;
use rivet_core::updates::Badges;
use rivet_core::{Command, Effect, Msg, Screen, State};
use segmented::{Segment, Segmented};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use theme::{Typography as _, spec};
use thunderstore_engine::ecosystem::GameDef;
use thunderstore_engine::profile::launch::{HostOs, LaunchMode, Runtime};
use thunderstore_engine::query::PackageRow;

/// The root view. It owns the state and is the only place `update` is called.
pub struct Shell {
  state: State,
  commands: UnboundedSender<Command>,
  search: Entity<InputState>,
  package_search: Entity<InputState>,
  profile_name: Entity<InputState>,
  /// The field a profile code to import is entered in.
  import_code: Entity<InputState>,
  /// Text for the profile name field, put there and focused on the next
  /// render, since setting it needs the window.
  fill_profile_name: Option<String>,
  launch_arguments: Entity<InputState>,
  /// Text for the launch arguments field, put there on the next render,
  /// since setting it needs the window.
  fill_launch_arguments: Option<String>,
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
  /// The widths of the list and the detail pane, kept while the app runs so
  /// the pane reopens at the width it was given.
  panes: Entity<ResizableState>,
  /// The interface size, as a percentage, picked for the display the window
  /// is on, used until the user chooses one.
  display_size: u16,
  /// Whether the window has been sized for the interface size, which waits
  /// for the settings to load.
  window_fitted: bool,
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
    let import_code = cx.new(|cx| InputState::new(window, cx));
    let coded = cx.subscribe_in(
      &import_code,
      window,
      |this: &mut Self, _, event: &InputEvent, _, cx| {
        if let InputEvent::PressEnter { .. } = event {
          this.submit_import_code(cx);
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
    let launch_arguments = cx.new(|cx| InputState::new(window, cx));
    let arguments_typed = cx.subscribe_in(
      &launch_arguments,
      window,
      |this: &mut Self, input, event: &InputEvent, _, cx| {
        if let InputEvent::Change = event {
          let arguments = input.read(cx).value().to_string();

          this.dispatch(Msg::LaunchArgumentsChanged(arguments), cx);
        }
      },
    );
    let focused = cx.observe_window_activation(window, |this: &mut Self, window, cx| {
      if window.is_window_active() {
        this.dispatch(Msg::WindowFocused, cx);
      }
    });
    let appearance = cx.observe_window_appearance(window, |_: &mut Self, _, cx| cx.notify());
    let moved = cx.observe_window_display(window, Self::measure_display);
    let rescaled = cx.observe_window_bounds(window, Self::measure_display);
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
      import_code,
      fill_profile_name: None,
      launch_arguments,
      fill_launch_arguments: None,
      close_window: false,
      notices: Vec::new(),
      modal: None,
      installed_search,
      icons: cx.new(|_| IconCache::new()),
      panes: cx.new(|_| ResizableState::default()),
      display_size: display_size(window, cx),
      window_fitted: false,
      _subscriptions: vec![
        searched,
        package_searched,
        named,
        coded,
        installed_searched,
        arguments_typed,
        focused,
        appearance,
        moved,
        rescaled,
      ],
      _inbox,
    };

    shell.dispatch(Msg::Started, cx);

    shell
  }

  pub fn state(&self) -> &State {
    &self.state
  }

  /// Picks the interface size again for the display the window is on, after
  /// it moved to another or its scale changed.
  fn measure_display(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    let size = display_size(window, cx);

    if size != self.display_size {
      self.display_size = size;
      cx.notify();
    }
  }

  /// Draws the interface at the chosen size, or the display's, and keeps
  /// toasts clear of the status bar at that size. Once the settings load,
  /// sizes the window for it too.
  fn apply_size(&mut self, window: &mut Window, cx: &mut App) {
    let size = self
      .state
      .settings
      .interface_size
      .unwrap_or(self.display_size);

    theme::apply_size(size, window, cx);

    if !self.window_fitted && self.state.screen != Screen::Starting {
      self.window_fitted = true;
      fit_window(size, window, cx);
    }

    let margins = &cx.theme().notification.margins;
    let bottom = spec(STATUS_BAR_HEIGHT).to_pixels(window.rem_size()) + margins.right;

    if margins.bottom != bottom {
      Theme::update(cx, |theme| theme.notification.margins.bottom = bottom);
    }
  }

  fn dispatch(&mut self, msg: Msg, cx: &mut Context<Self>) {
    for effect in rivet_core::update(&mut self.state, msg) {
      match effect {
        Effect::Run(command) => {
          self.commands.unbounded_send(command).ok();
        }
        Effect::ChooseFolder => self.choose_folder(cx),
        Effect::ChooseExportFile(suggested) => self.choose_export_file(&suggested, cx),
        Effect::ChooseImportFile => self.choose_import_file(cx),
        Effect::FillProfileName(text) => self.fill_profile_name = Some(text),
        Effect::FillLaunchArguments(text) => self.fill_launch_arguments = Some(text),
        Effect::OpenUrl(url) => cx.open_url(&url),
        Effect::CloseWindow => self.close_window = true,
        Effect::CopyToClipboard(text) => {
          cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
        Effect::Notify(notice) => self.notices.push(notice),
        Effect::After { delay, msg } => self.send_after(delay, *msg, cx),
      }
    }

    cx.notify();
  }

  /// Dispatches `msg` once `delay` has passed, unless the shell is gone.
  fn send_after(&mut self, delay: Duration, msg: Msg, cx: &mut Context<Self>) {
    cx.spawn(async move |this, cx| {
      cx.background_executor().timer(delay).await;
      this.update(cx, |this, cx| this.dispatch(msg, cx)).ok();
    })
    .detach();
  }

  /// Empties the package search box to match a list whose search was reset.
  fn empty_package_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
    self
      .package_search
      .update(cx, |input, cx| input.set_value("", window, cx));
  }

  /// Sends the name field's text as the name of the import being asked
  /// about, or else of the profile the switcher asked a name for.
  fn submit_profile_name(&mut self, cx: &mut Context<Self>) {
    let name = self.profile_name.read(cx).value().to_string();
    let msg = match self.state.importing {
      Some(Importing::Naming(_)) => Msg::ImportNameSubmitted(name),
      _ => Msg::ProfileNameSubmitted(name),
    };

    self.dispatch(msg, cx);
  }

  fn submit_import_code(&mut self, cx: &mut Context<Self>) {
    let code = self.import_code.read(cx).value().to_string();

    self.dispatch(Msg::ImportCodeSubmitted(code), cx);
  }

  fn choose_folder(&mut self, cx: &mut Context<Self>) {
    let answer = cx.prompt_for_paths(PathPromptOptions {
      files: false,
      directories: true,
      multiple: false,
      prompt: None,
    });

    self.answer_path_prompt(answer, Msg::FolderChosen, cx);
  }

  fn choose_import_file(&mut self, cx: &mut Context<Self>) {
    let answer = cx.prompt_for_paths(PathPromptOptions {
      files: true,
      directories: false,
      multiple: false,
      prompt: None,
    });

    self.answer_path_prompt(answer, Msg::ImportFileChosen, cx);
  }

  /// Dispatches the first path a dialog answers with, wrapped by `msg`, or
  /// `None` when it was cancelled or could not open.
  fn answer_path_prompt<Closed, Failed>(
    &mut self,
    answer: impl Future<Output = Result<Result<Option<Vec<PathBuf>>, Failed>, Closed>> + 'static,
    msg: fn(Option<PathBuf>) -> Msg,
    cx: &mut Context<Self>,
  ) {
    cx.spawn(async move |this, cx| {
      let path = answer
        .await
        .ok()
        .and_then(Result::ok)
        .flatten()
        .and_then(|paths| paths.into_iter().next());

      this
        .update(cx, |this, cx| this.dispatch(msg(path), cx))
        .ok();
    })
    .detach();
  }

  fn choose_export_file(&mut self, suggested: &str, cx: &mut Context<Self>) {
    let folder = rivet_platform::paths::save_folder().unwrap_or_default();
    let answer = cx.prompt_for_new_path(&folder, Some(suggested));

    cx.spawn(async move |this, cx| {
      let path = answer.await.ok().and_then(Result::ok).flatten();

      this
        .update(cx, |this, cx| {
          this.dispatch(Msg::ExportFileChosen(path), cx)
        })
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
          .text_small()
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
    let layout = self.state.settings.picker_layout;
    let empty = games.is_empty().then_some(text::NO_GAME_MATCHES);
    let games = match layout {
      PickerLayout::List => {
        let (set_up, rest) = rivet_core::picker::set_up_first(games, &self.state.settings);
        let headed = !set_up.is_empty();
        let heading = |id: &'static str, label: &'static str| {
          div()
            .id(id)
            .pt_2()
            .text_small()
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(palette.muted_text)
            .child(label)
            .test_support()
        };
        let set_up: Vec<_> = set_up
          .into_iter()
          .map(|game| self.render_pickable(game, layout, cx))
          .collect();
        let rest: Vec<_> = rest
          .into_iter()
          .map(|game| self.render_pickable(game, layout, cx))
          .collect();
        let all = (headed && !rest.is_empty()).then(|| heading("all-games", text::ALL_GAMES));

        v_flex()
          .id("games-list")
          .flex_1()
          .w_full()
          .max_w(spec(LIST_WIDTH))
          .mx_auto()
          .overflow_y_scroll()
          .children(empty)
          .children(headed.then(|| heading("your-games", text::YOUR_GAMES)))
          .children(set_up)
          .children(all)
          .children(rest)
      }
      PickerLayout::Grid => {
        let tiles: Vec<_> = games
          .into_iter()
          .map(|game| self.render_pickable(game, layout, cx))
          .collect();

        v_flex()
          .id("games-grid")
          .flex_1()
          .overflow_y_scroll()
          .child(
            div()
              .flex()
              .flex_row()
              .flex_wrap()
              .justify_center()
              .gap_3()
              .children(empty)
              .children(tiles),
          )
      }
    }
    .test_support();
    let shell = cx.weak_entity();
    let labelled = |icon: gpui_kit::assets::IconName, label: &'static str| {
      h_flex()
        .gap_1p5()
        .child(Icon::new(icon).small())
        .child(label)
    };
    let layouts = Segmented::new("picker-layouts")
      .child(Segment::new(
        "picker-list",
        labelled(gpui_kit::assets::IconName::List, text::PICKER_LIST),
        layout == PickerLayout::List,
        send(&shell, Msg::PickerLayoutChosen(PickerLayout::List)),
      ))
      .child(Segment::new(
        "picker-grid",
        labelled(gpui_kit::assets::IconName::LayoutGrid, text::PICKER_GRID),
        layout == PickerLayout::Grid,
        send(&shell, Msg::PickerLayoutChosen(PickerLayout::Grid)),
      ));
    let toolbar = h_flex()
      .flex_none()
      .h(spec(TOOLBAR_HEIGHT))
      .gap_3()
      .child(
        div()
          .id("picker-search")
          .flex_1()
          .min_w(spec(SEARCH_MIN_WIDTH))
          .child(
            Input::new(&self.search)
              .small()
              .prefix(Icon::new(IconName::Search))
              .cleanable(true),
          )
          .test_support(),
      )
      .child(div().id("picker-layout").child(layouts).test_support());
    let header = v_flex()
      .flex_none()
      .w_full()
      .max_w(spec(LIST_WIDTH))
      .mx_auto()
      .gap_3()
      .child(
        div()
          .text_size(spec(GAME_NAME_SIZE))
          .font_weight(FontWeight::BOLD)
          .child(text::PICKER_TITLE),
      )
      .children(
        self
          .state
          .settings_error
          .as_ref()
          .map(|error| failure(palette, text::settings_error(error))),
      )
      .children(fallback)
      .children(refusal.map(|refusal| failure(palette, refusal)))
      .child(toolbar);

    v_flex()
      .size_full()
      .p(spec(MAIN_PADDING))
      .gap_3()
      .child(header)
      .child(games)
      .into_any_element()
  }

  /// `game` in the picker, found in tests by its community slug. In the
  /// list it is a row with its cover, its name over the stores it is sold
  /// on, and a tag when Rivet has its folder; in the grid, a cover tile with
  /// its name under it. Either way a game whose mods Rivet cannot install is
  /// marked under its name.
  fn render_pickable(
    &self,
    game: &GameDef,
    layout: PickerLayout,
    cx: &mut Context<Self>,
  ) -> AnyElement {
    let palette = Palette::new(cx);
    let slug = game.slug.clone();
    let name = game.display_name();
    let cover = self
      .state
      .picker
      .covers
      .get(&game.slug)
      .map(PathBuf::as_path);
    let no_mod_support = (!rivet_core::picker::installs_mods(game)).then(|| {
      div()
        .id("no-mod-support")
        .truncate()
        .text_small()
        .text_color(palette.muted_text)
        .aria_label(text::NO_MOD_SUPPORT)
        .child(text::NO_MOD_SUPPORT)
        .test_support()
    });
    let item = match layout {
      PickerLayout::List => {
        let stores = rivet_core::picker::stores(game);
        let stores = (!stores.is_empty()).then(|| {
          let names = text::stores(&stores);

          div()
            .id("stores")
            .truncate()
            .text_small()
            .text_color(palette.muted_text)
            .aria_label(names.clone())
            .child(names)
            .test_support()
        });
        let set_up = self.state.settings.folder(&game.slug).is_some().then(|| {
          let color = cx.theme().success;

          div()
            .id("ready")
            .flex_none()
            .aria_label(text::READY)
            .child(
              Tag::custom(color, color, color)
                .outline()
                .small()
                .rounded_full()
                .text_size(spec(TAG_TEXT_SIZE))
                .child(
                  h_flex()
                    .gap_1()
                    .child(Icon::new(IconName::Check).xsmall())
                    .child(text::READY),
                ),
            )
            .test_support()
        });

        list_row(slug.clone(), false, palette)
          .child(game_cover(
            name,
            cover,
            LIST_COVER_WIDTH,
            LIST_COVER_HEIGHT,
            cx,
          ))
          .child(
            v_flex()
              .flex_1()
              .min_w_0()
              .gap_0p5()
              .child(name_label(name))
              .children(stores)
              .children(no_mod_support),
          )
          .children(set_up)
          .child(
            Icon::new(IconName::ChevronRight)
              .small()
              .text_color(palette.muted_text),
          )
      }
      PickerLayout::Grid => v_flex()
        .id(SharedString::from(slug.clone()))
        .w(spec(GRID_COVER_WIDTH + 2. * GRID_TILE_PADDING))
        .p(spec(GRID_TILE_PADDING))
        .gap_2()
        .rounded(spec(ROW_RADIUS))
        .map(|tile| paint_row(tile, false, palette))
        .child(game_cover(
          name,
          cover,
          GRID_COVER_WIDTH,
          GRID_COVER_HEIGHT,
          cx,
        ))
        .child(name_label(name))
        .children(no_mod_support),
    };
    let item = item
      .cursor_pointer()
      .on_click(cx.listener(move |this, _, _, cx| {
        this.dispatch(Msg::GamePicked(slug.clone()), cx);
      }))
      .test_support();

    match layout {
      PickerLayout::List => div()
        .flex_none()
        .h(spec(GAME_ROW_HEIGHT))
        .py(spec(ROW_GAP / 2.))
        .child(item)
        .into_any_element(),
      PickerLayout::Grid => item.into_any_element(),
    }
  }

  /// The bar along the bottom of the window, [`STATUS_BAR_HEIGHT`] tall
  /// whatever it shows, so nothing above it moves. It shows the running
  /// operation with how many wait behind it, or else what the app is busy
  /// with, and is empty when neither. On Windows it ends in a label saying
  /// the build is experimental.
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
          .text_small()
          .text_color(palette.muted_text)
          .child(note)
          .test_support()
          .into_any_element()
      }),
    };
    let experimental = (self.state.host == Some(HostOs::Windows)).then(|| {
      div()
        .id("experimental")
        .flex_none()
        .text_small()
        .text_color(palette.warning)
        .child(text::EXPERIMENTAL)
        .test_support()
    });

    h_flex()
      .flex_none()
      .h(spec(STATUS_BAR_HEIGHT))
      .px_4()
      .gap_3()
      .overflow_hidden()
      .bg(palette.panel)
      .border_t_1()
      .border_color(palette.border)
      .child(
        h_flex()
          .id("status-bar")
          .flex_1()
          .min_w_0()
          .h_full()
          .children(shown)
          .test_support(),
      )
      .children(experimental)
      .into_any_element()
  }

  /// What the app is busy with outside the queue: starting a game, changing
  /// a profile, exporting one, reading one to import, finding a game from
  /// the picker or checking a folder chosen in settings, refreshing the
  /// package list, or checking Steam's launch options, first match in that
  /// order. `None` when it is busy with none.
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
    let exporting = || match &state.exporting {
      Some(Exporting::Running(export)) => Some(text::exporting(&export.profile)),
      _ => None,
    };
    let reading = || match &state.importing {
      Some(Importing::Reading(_)) => Some(text::READING_SHARED.to_string()),
      _ => None,
    };
    let finding = || match (&state.screen, &state.picker.step) {
      (Screen::Picker, Step::Locating(game)) => Some(text::locating(&self.game_name(game))),
      (Screen::Picker, Step::Checking { folder, .. }) => Some(text::checking(folder)),
      (Screen::Settings(_), _) => match &state.settings_screen.step {
        settings_screen::Step::Checking { folder, .. } => Some(text::checking(folder)),
        _ => None,
      },
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
      .or_else(exporting)
      .or_else(reading)
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

  /// A game: the sidebar, and beside it the banners that need action, the
  /// toolbar over the list, and the detail pane while it is shown. The pane starts [`DETAILS_SHARE`] of the width beside the
  /// sidebar, and keeps whatever width it is dragged to.
  fn render_game(&self, game: &str, window: &Window, cx: &mut Context<Self>) -> AnyElement {
    let palette = Palette::new(cx);
    let shown = self.state.details_shown;
    let pixels = |size: f32| spec(size).to_pixels(window.rem_size());
    let content = window.viewport_size().width - pixels(SIDEBAR_WIDTH + 2. * MAIN_PADDING);
    let pane_width = (content * DETAILS_SHARE).max(pixels(DETAILS_MIN_WIDTH));
    let content = match self.state.tab {
      Tab::Browse => self
        .state
        .browse
        .as_ref()
        .map(|browse| render_browse(browse, &self.icons, cx)),
      Tab::Installed => Some(self.render_installed(cx)),
    };
    let list = v_flex().flex_1().min_w_0().pr_3().overflow_hidden().child(
      v_flex()
        .id("mod-list")
        .flex_1()
        .min_h_0()
        .overflow_hidden()
        .children(content)
        .test_support(),
    );
    let details = resizable_panel()
      .visible(shown)
      .size(pane_width)
      .size_range(pixels(DETAILS_MIN_WIDTH)..Pixels::MAX)
      .when(shown, |pane| pane.child(self.render_details(cx)));
    let panes = h_resizable("panes")
      .with_state(&self.panes)
      .child(
        resizable_panel()
          .size_range(pixels(LIST_MIN_WIDTH)..Pixels::MAX)
          .child(list),
      )
      .child(details);
    let main = v_flex()
      .flex_1()
      .min_w_0()
      .h_full()
      .p(spec(MAIN_PADDING))
      .gap_3()
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
      .child(self.render_toolbar(cx))
      .child(div().flex().flex_1().min_h_0().child(panes));

    div()
      .flex()
      .size_full()
      .child(self.render_sidebar(game, cx))
      .child(main)
      .into_any_element()
  }

  /// The settings of `game`: its folder, launch arguments, on Linux whether
  /// it runs natively or under Proton, and the loader files launches copied
  /// into its folder, then the Steam folder every game shares. While a folder
  /// is being changed, no other can be.
  fn render_settings(&self, game: &str, cx: &mut Context<Self>) -> AnyElement {
    let palette = Palette::new(cx);
    let state = &self.state;
    let idle = state.settings_screen.step == settings_screen::Step::Idle;
    let name = self.game_name(game);
    let label = |label: &'static str| div().w(spec(SETTINGS_LABEL_WIDTH)).flex_none().child(label);
    let folder_row = |id: &'static str, title: &'static str, folder: String, target: Target| {
      h_flex()
        .gap_3()
        .items_center()
        .child(label(title))
        .child(
          div()
            .id(id)
            .flex_1()
            .min_w_0()
            .truncate()
            .child(folder)
            .test_support(),
        )
        .child(
          Button::new(SharedString::from(format!("change-{id}")))
            .label(text::CHANGE)
            .small()
            .disabled(!idle)
            .on_click(cx.listener(move |this, _, _, cx| {
              this.dispatch(Msg::ChangeFolderPressed(target), cx);
            })),
        )
    };
    let section = |title: SharedString| {
      v_flex()
        .gap_3()
        .p_3()
        .border_1()
        .border_color(palette.border)
        .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
    };
    let game_folder = state
      .settings
      .folder(game)
      .map(|folder| folder.display().to_string())
      .unwrap_or_default();
    let steam_folder = state.settings.steam_folder.as_ref().map_or_else(
      || text::STEAM_FOLDER_FOUND.to_string(),
      |folder| folder.display().to_string(),
    );
    let launch_arguments = v_flex()
      .gap_1()
      .child(
        h_flex()
          .gap_3()
          .items_center()
          .child(label(text::LAUNCH_ARGUMENTS))
          .child(
            div()
              .id("launch-arguments")
              .flex_1()
              .child(Input::new(&self.launch_arguments))
              .test_support(),
          ),
      )
      .child(
        div()
          .text_small()
          .text_color(palette.muted_text)
          .child(text::LAUNCH_ARGUMENTS_HINT),
      );
    let chosen = state.settings.runtime(game);
    let runtime_segment = |id: &'static str, title: &'static str, choice: RuntimeChoice| {
      Segment::new(
        id,
        title,
        chosen == choice,
        cx.listener(move |this, _, _, cx| {
          this.dispatch(Msg::RuntimeChosen(choice), cx);
        }),
      )
    };
    let runtime = (state.host == Some(HostOs::Linux)).then(|| {
      v_flex()
        .gap_1()
        .child(
          h_flex()
            .gap_2()
            .items_center()
            .child(label(text::RUNS_WITH))
            .child(
              Segmented::new("runtime")
                .child(runtime_segment(
                  "runtime-detect",
                  text::RUNTIME_DETECT,
                  RuntimeChoice::Detect,
                ))
                .child(runtime_segment(
                  "runtime-native",
                  text::RUNTIME_NATIVE,
                  RuntimeChoice::Native,
                ))
                .child(runtime_segment(
                  "runtime-proton",
                  text::RUNTIME_PROTON,
                  RuntimeChoice::Proton,
                )),
            ),
        )
        .child(
          div()
            .text_small()
            .text_color(palette.muted_text)
            .child(text::RUNTIME_HINT),
        )
    });
    let appearance_segment = |id: &'static str, title: &'static str, choice: Appearance| {
      Segment::new(
        id,
        title,
        state.settings.appearance == choice,
        cx.listener(move |this, _, _, cx| {
          this.dispatch(Msg::AppearanceChosen(choice), cx);
        }),
      )
    };
    let appearance = h_flex()
      .gap_2()
      .items_center()
      .child(label(text::APPEARANCE))
      .child(
        Segmented::new("appearance")
          .child(appearance_segment(
            "appearance-system",
            text::APPEARANCE_SYSTEM,
            Appearance::System,
          ))
          .child(appearance_segment(
            "appearance-light",
            text::APPEARANCE_LIGHT,
            Appearance::Light,
          ))
          .child(appearance_segment(
            "appearance-dark",
            text::APPEARANCE_DARK,
            Appearance::Dark,
          )),
      );
    let theme_segment = |id: &'static str, title: &'static str, choice: ThemeFamily| {
      Segment::new(
        id,
        title,
        state.settings.theme == choice,
        cx.listener(move |this, _, _, cx| {
          this.dispatch(Msg::ThemeChosen(choice), cx);
        }),
      )
    };
    let theme = h_flex()
      .gap_2()
      .items_center()
      .child(label(text::THEME))
      .child(
        Segmented::new("theme")
          .child(theme_segment(
            "theme-default",
            text::THEME_DEFAULT,
            ThemeFamily::Default,
          ))
          .child(theme_segment(
            "theme-one",
            text::THEME_ONE,
            ThemeFamily::One,
          )),
      );
    let size_segment = |id: SharedString, title: SharedString, size: Option<u16>| {
      Segment::new(
        id,
        title,
        state.settings.interface_size == size,
        cx.listener(move |this, _, _, cx| {
          this.dispatch(Msg::InterfaceSizeChosen(size), cx);
        }),
      )
    };
    let interface_size = h_flex()
      .gap_2()
      .items_center()
      .child(label(text::INTERFACE_SIZE))
      .child(INTERFACE_SIZES.into_iter().fold(
        Segmented::new("interface-size").child(size_segment(
          "interface-size-auto".into(),
          text::INTERFACE_SIZE_AUTO.into(),
          None,
        )),
        |sizes, size| {
          sizes.child(size_segment(
            format!("interface-size-{size}").into(),
            text::percent(size).into(),
            Some(size),
          ))
        },
      ));
    let loader_files = v_flex()
      .gap_1()
      .child(
        h_flex()
          .gap_3()
          .items_center()
          .child(label(text::LOADER_FILES))
          .child(
            div()
              .id("loader-files")
              .flex_1()
              .min_w_0()
              .truncate()
              .child(text::loader_files(state.loader_files(game).len()))
              .test_support(),
          )
          .child(
            Button::new("remove-loader-files")
              .label(text::REMOVE)
              .small()
              .disabled(!state.can_remove_loader_files(game))
              .on_click(cx.listener(|this, _, _, cx| {
                this.dispatch(Msg::RemoveLoaderFilesPressed, cx);
              })),
          ),
      )
      .child(
        div()
          .text_small()
          .text_color(palette.muted_text)
          .child(text::LOADER_FILES_HINT),
      );

    v_flex()
      .size_full()
      .p_4()
      .gap_4()
      .child(
        h_flex()
          .gap_3()
          .items_center()
          .child(
            Button::new("close-settings")
              .label(text::BACK)
              .ghost()
              .on_click(cx.listener(|this, _, _, cx| this.dispatch(Msg::SettingsClosed, cx))),
          )
          .child(div().text_xl().child(text::settings_title(&name))),
      )
      .children(
        state
          .settings_error
          .as_ref()
          .map(|error| failure(palette, text::settings_error(error))),
      )
      .children(state.settings_screen.refusal.as_ref().map(|refusal| {
        div()
          .id("settings-refusal")
          .child(failure(palette, text::refusal(&name, refusal)))
          .test_support()
      }))
      .child(
        section(name.clone())
          .child(folder_row(
            "game-folder",
            text::GAME_FOLDER,
            game_folder,
            Target::GameFolder,
          ))
          .child(launch_arguments)
          .children(runtime)
          .child(loader_files),
      )
      .child(section(text::EVERY_GAME.into()).child(folder_row(
        "steam-folder",
        text::STEAM_FOLDER,
        steam_folder,
        Target::SteamFolder,
      )))
      .child(
        section(text::DISPLAY.into())
          .child(theme)
          .child(appearance)
          .child(interface_size),
      )
      .into_any_element()
  }

  /// The bar over the list, [`TOOLBAR_HEIGHT`] tall whichever view it shows,
  /// so switching views leaves the list where it was: the segmented control
  /// choosing the view, the search box of the view shown, the Filter menu,
  /// that view's actions, and at the far end Details while the detail pane is
  /// closed.
  /// Details keeps its slot [`DETAILS_BUTTON_WIDTH`] wide while the pane is
  /// open, so nothing moves when it comes and goes.
  fn render_toolbar(&self, cx: &mut Context<Self>) -> AnyElement {
    let shell = cx.weak_entity();
    let (search_id, search) = match self.state.tab {
      Tab::Installed => ("installed-search", &self.installed_search),
      Tab::Browse => ("package-search", &self.package_search),
    };
    let tools = match self.state.tab {
      Tab::Installed => self.render_profile_tools(&shell),
      Tab::Browse => self.render_browse_tools(&shell),
    };
    let open_details = (!self.state.details_shown).then(|| {
      Button::new("open-details")
        .label(text::DETAILS)
        .icon(IconName::PanelRightOpen)
        .ghost()
        .small()
        .on_click(send(&shell, Msg::ShowDetails(true)))
    });

    h_flex()
      .id("toolbar")
      .flex_none()
      .h(spec(TOOLBAR_HEIGHT))
      .gap_3()
      .child(self.render_views(&shell))
      .child(
        div()
          .id(search_id)
          .flex_shrink(1.)
          .min_w(spec(SEARCH_MIN_WIDTH))
          .w(spec(SEARCH_WIDTH))
          .child(
            Input::new(search)
              .small()
              .prefix(Icon::new(IconName::Search))
              .cleanable(true),
          )
          .test_support(),
      )
      .child(self.render_filter_menu(&shell))
      .child(div().flex_1())
      .children(tools)
      .child(
        h_flex()
          .flex_none()
          .w(spec(DETAILS_BUTTON_WIDTH))
          .justify_end()
          .children(open_details),
      )
      .test_support()
      .into_any_element()
  }

  /// The segmented control choosing between the open profile's mods, with
  /// their count once known, and Thunderstore's. The count keeps
  /// [`VIEW_COUNT_WIDTH`] while unknown and whatever its digits, so the
  /// controls after it stay put when profiles switch or mods are added.
  fn render_views(&self, shell: &WeakEntity<Self>) -> Segmented {
    let state = &self.state;
    let count = state
      .profile
      .as_ref()
      .and_then(|profile| state.mod_count(&profile.name))
      .map(|count| {
        div()
          .id("view-mod-count")
          .aria_label(text::mod_count(count))
          .child(count.to_string())
          .test_support()
      });
    let count = div().w(spec(VIEW_COUNT_WIDTH)).tabular().children(count);

    Segmented::new("views")
      .child(Segment::new(
        "tab-installed",
        h_flex().gap_1p5().child(text::TAB_INSTALLED).child(count),
        state.tab == Tab::Installed,
        send(shell, Msg::TabChosen(Tab::Installed)),
      ))
      .child(Segment::new(
        "tab-browse",
        text::TAB_BROWSE,
        state.tab == Tab::Browse,
        send(shell, Msg::TabChosen(Tab::Browse)),
      ))
  }

  /// The profile view's actions: a menu with the list's sort and turning
  /// every mod on or off.
  fn render_profile_tools(&self, shell: &WeakEntity<Self>) -> Vec<AnyElement> {
    let state = &self.state;
    let can_change = state.can_manage();
    let sort = state.manage.sort;
    let shell = shell.clone();
    let menu = Button::new("profile-menu")
      .icon(IconName::Ellipsis)
      .ghost()
      .small()
      .accessibility_label(text::PROFILE_MENU)
      .dropdown_menu(move |menu, _, _| {
        let sort_item = |id: &'static str, label: &'static str, choice: Sort| {
          menu_item(id, label, None)
            .checked(sort == choice)
            .on_click(send(&shell, Msg::InstalledSortChosen(choice)))
        };

        menu
          .label(text::SORT_BY)
          .item(sort_item(
            "installed-sort-name",
            text::SORT_NAME,
            Sort::Name,
          ))
          .item(sort_item(
            "installed-sort-author",
            text::SORT_AUTHOR,
            Sort::Author,
          ))
          .item(sort_item(
            "installed-sort-newest",
            text::SORT_NEWEST,
            Sort::Newest,
          ))
          .separator()
          .item(
            menu_item("enable-all", text::ENABLE_ALL, None)
              .disabled(!can_change)
              .on_click(send(&shell, Msg::EnableAllPressed)),
          )
          .item(
            menu_item("disable-all", text::DISABLE_ALL, None)
              .disabled(!can_change)
              .on_click(send(&shell, Msg::DisableAllPressed)),
          )
      });

    vec![menu.into_any_element()]
  }

  /// The menu filtering the view shown: a checkbox per category, which both
  /// views share, Show deprecated and Show NSFW on Thunderstore, and Clear
  /// filters. Its label counts the filters on, and it stays
  /// [`FILTER_BUTTON_WIDTH`] wide whatever the count, so the toolbar does not
  /// move. Disabled until the game's list exists.
  fn render_filter_menu(&self, shell: &WeakEntity<Self>) -> impl IntoElement {
    let tab = self.state.tab;
    let (categories, filters) = self
      .state
      .browse
      .as_ref()
      .map(|browse| (browse.categories.clone(), browse.filters.clone()))
      .unwrap_or_default();
    let shown = match tab {
      Tab::Browse => usize::from(filters.show_deprecated) + usize::from(filters.show_nsfw),
      Tab::Installed => 0,
    };
    let shell = shell.clone();

    Button::new("filter-menu")
      .label(text::filter_menu(filters.categories.len() + shown))
      .ghost()
      .small()
      .flex_none()
      .w(spec(FILTER_BUTTON_WIDTH))
      .disabled(self.state.browse.is_none())
      .dropdown_caret(true)
      .dropdown_menu(move |menu, _, _| filter_menu(menu, tab, &categories, &filters, &shell))
  }

  /// Thunderstore's actions: its sort and refreshing it. None until the
  /// game's list exists.
  fn render_browse_tools(&self, shell: &WeakEntity<Self>) -> Vec<AnyElement> {
    let Some(browse) = self.state.browse.as_ref() else {
      return Vec::new();
    };
    let filters = &browse.filters;
    let sort = Button::new("browse-sort")
      .label(text::sort_menu(filters.sort, filters.direction))
      .ghost()
      .small()
      .dropdown_caret(true)
      .dropdown_menu({
        let shell = shell.clone();
        let filters = filters.clone();

        move |menu, _, _| sort_menu(menu, &filters, &shell)
      });
    let refresh = Button::new("refresh")
      .icon(IconName::RefreshCw)
      .ghost()
      .small()
      .tooltip(text::REFRESH)
      .accessibility_label(text::REFRESH)
      .on_click(send(shell, Msg::RefreshPressed));

    vec![sort.into_any_element(), refresh.into_any_element()]
  }

  /// The open profile's packages the search and chosen categories keep, with
  /// their badges, or why there are none. Only the rows in view are built.
  fn render_installed(&self, cx: &mut Context<Self>) -> AnyElement {
    let state = &self.state;
    let installed = state.profile.as_ref().and_then(Profile::installed);
    let count = state.installed_rows().len();
    let empty = match installed {
      Some(installed) if installed.is_empty() => Some(text::NO_INSTALLED),
      Some(_) if count == 0 => Some(text::NO_INSTALLED_MATCHES),
      _ => None,
    };
    let rows = uniform_list(
      "installed-rows",
      count,
      cx.processor(|this, range: Range<usize>, _, cx| {
        let state = &this.state;
        let badges = state.badges();
        let rows = state.installed_rows();

        rows
          .get(range)
          .unwrap_or_default()
          .iter()
          .map(|&(full_name, package)| {
            installed_row(state, full_name, package, badges.get(full_name), cx)
          })
          .collect()
      }),
    )
    .flex_1();
    let updates = self.render_updates_available(cx);

    v_flex()
      .flex_1()
      .min_h_0()
      .gap_2()
      .image_cache(self.icons.clone())
      .children(updates)
      .children(empty)
      .child(
        v_flex()
          .id("installed-packages")
          .flex_1()
          .min_h_0()
          .child(rows)
          .test_support(),
      )
      .into_any_element()
  }

  /// The line over the profile's list saying how many of its mods an update
  /// would change, with Update all, disabled while [`State::can_update`] is
  /// false.
  /// `None` while no mod has an update.
  fn render_updates_available(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
    let state = &self.state;
    let updatable = state
      .badges()
      .values()
      .filter(|badges| badges.updatable())
      .count();

    if updatable == 0 {
      return None;
    }

    let palette = Palette::new(cx);
    let count = div()
      .id("updates-available-count")
      .flex_1()
      .truncate()
      .aria_label(text::updates_available(updatable))
      .child(text::updates_available(updatable))
      .test_support();
    let update = Button::new("update-all")
      .label(text::UPDATE_ALL)
      .small()
      .flex_none()
      .disabled(!state.can_update())
      .on_click(send(&cx.weak_entity(), Msg::UpdateAllPressed));

    Some(
      h_flex()
        .id("updates-available")
        .flex_none()
        .items_center()
        .gap_2()
        .px_3()
        .py_1p5()
        .bg(palette.panel)
        .border_1()
        .border_color(palette.border)
        .rounded(spec(PANE_RADIUS))
        .child(count)
        .child(update)
        .test_support()
        .into_any_element(),
    )
  }

  /// The detail pane: the shown mod's icon, name, author and tags with
  /// closing the pane, its facts, what can be done to it, then its readme.
  /// While the list shows no mod it says so, keeping its place and width.
  fn render_details(&self, cx: &mut Context<Self>) -> AnyElement {
    let palette = Palette::new(cx);
    let close = Button::new("close-details")
      .icon(IconName::Close)
      .ghost()
      .small()
      .flex_none()
      .tooltip(text::HIDE_DETAILS)
      .accessibility_label(text::HIDE_DETAILS)
      .on_click(cx.listener(|this, _, _, cx| this.dispatch(Msg::ShowDetails(false), cx)));
    let pane = v_flex()
      .id("details")
      .size_full()
      .min_w_0()
      .overflow_hidden()
      .bg(palette.panel)
      .border_1()
      .border_color(palette.border)
      .rounded(spec(PANE_RADIUS));
    let (Some(details), Some(readme)) = (self.state.details(), self.state.readme.as_ref()) else {
      let empty = div()
        .id("details-empty")
        .flex_1()
        .text_color(palette.muted_text)
        .child(text::NO_MOD_SHOWN)
        .test_support();

      return pane
        .child(
          h_flex()
            .items_start()
            .gap_2()
            .p_4()
            .child(empty)
            .child(close),
        )
        .test_support()
        .into_any_element();
    };
    let head = v_flex()
      .flex_none()
      .px_5()
      .pt(spec(18.))
      .pb(spec(14.))
      .gap(spec(14.))
      .border_b_1()
      .border_color(palette.border)
      .child(self.render_details_header(&details, close, cx))
      .child(render_facts(&details))
      .child(self.render_details_actions(&details, cx));

    pane
      .child(head)
      .child(
        v_flex()
          .flex_1()
          .min_h_0()
          .px_5()
          .pt_4()
          .child(render_readme(readme, cx)),
      )
      .test_support()
      .into_any_element()
  }

  /// The top of the detail pane: the mod's icon, its name, author and tags,
  /// and `close`.
  fn render_details_header(
    &self,
    details: &Details,
    close: Button,
    cx: &mut Context<Self>,
  ) -> impl IntoElement + use<> {
    let palette = Palette::new(cx);
    let state = &self.state;
    let full_name = details.full_name.as_str();
    let listing = details.listing.as_ref();
    let tags = if state.is_installed(full_name) {
      mod_tags(
        state,
        full_name,
        state.badges().get(full_name),
        "details-",
        cx,
      )
    } else {
      listing
        .filter(|listing| listing.deprecated)
        .map(|_| {
          state_tag(
            format!("details-deprecated-badge-{full_name}"),
            palette.tag_deprecated,
            text::DEPRECATED.to_string(),
          )
        })
        .into_iter()
        .collect()
    };
    let icon = listing.and_then(|listing| listing.icon.as_deref());
    let name = div()
      .id("details-name")
      .truncate()
      .text_size(spec(DETAILS_NAME_SIZE))
      .font_weight(FontWeight::BOLD)
      .aria_label(details.name.clone())
      .child(details.name.clone())
      .test_support();

    h_flex()
      .items_start()
      .gap(spec(14.))
      .child(mod_icon(&details.name, icon, DETAILS_ICON_SIZE, cx))
      .child(
        v_flex()
          .flex_1()
          .min_w_0()
          .child(name)
          .child(
            div()
              .truncate()
              .text_small()
              .text_color(palette.muted_text)
              .child(text::author(&details.owner)),
          )
          .child(h_flex().flex_wrap().gap_1p5().mt_2().children(tags)),
      )
      .child(close)
  }

  /// A button for each thing the detail pane offers to do to its mod, the
  /// rarer ones quieter, each enabled while [`State::can_take`] it, then a
  /// line saying why when the game's mods cannot be installed.
  fn render_details_actions(&self, details: &Details, cx: &mut Context<Self>) -> impl IntoElement {
    let state = &self.state;
    let unsupported = (!state.installs_mods()).then(|| {
      div()
        .id("details-unsupported")
        .text_small()
        .text_color(Palette::new(cx).muted_text)
        .aria_label(text::MODS_UNSUPPORTED)
        .child(text::MODS_UNSUPPORTED)
        .test_support()
    });
    let buttons: Vec<Button> = details
      .actions
      .iter()
      .map(|action| {
        let (id, label, quiet) = match action {
          Action::Install => ("details-install", text::INSTALL.to_string(), false),
          Action::Update(version) => ("details-update", text::update_to(version), false),
          Action::Unpin => ("details-unpin", text::UNPIN.to_string(), false),
          Action::ChooseVersion => ("details-version", text::CHOOSE_VERSION.to_string(), true),
          Action::Uninstall => ("details-uninstall", text::UNINSTALL.to_string(), true),
        };
        let msg = action.msg(&details.full_name);

        Button::new(id)
          .label(label)
          .small()
          .when(quiet, |button| button.ghost())
          .disabled(!state.can_take(action))
          .on_click(cx.listener(move |this, _, _, cx| this.dispatch(msg.clone(), cx)))
      })
      .collect();

    v_flex()
      .gap_2()
      .child(h_flex().flex_wrap().gap_2().children(buttons))
      .children(unsupported)
  }

  /// The column along the left of a game, [`SIDEBAR_WIDTH`] wide whatever
  /// it shows: the game with switching to another, how it runs on Linux, the
  /// launch buttons, the game's profiles with the ways to add one, then the
  /// actions on the current profile and the game's settings. The line saying
  /// how the game runs keeps its height while that is found out, so nothing
  /// under it moves.
  fn render_sidebar(&self, game: &str, cx: &mut Context<Self>) -> AnyElement {
    let palette = Palette::new(cx);
    let shell = cx.weak_entity();
    let state = &self.state;
    let can_launch = state.can_launch();
    let runtime = (state.host == Some(HostOs::Linux)).then(|| {
      let runs = state
        .launcher
        .as_ref()
        .and_then(|launcher| launcher.runtime)
        .map(|runtime| match runtime {
          Runtime::Proton => ("runs-proton", text::RUNS_UNDER_PROTON),
          _ => ("runs-native", text::RUNS_NATIVELY),
        });

      div()
        .h_5()
        .mt_1()
        .text_small()
        .text_color(palette.muted_text)
        .children(runs.map(|(id, label)| div().id(id).child(label).test_support()))
    });
    let heading = v_flex()
      .flex_none()
      .px_4()
      .pt(spec(18.))
      .pb_4()
      .border_b_1()
      .border_color(palette.border)
      .child(
        h_flex()
          .gap_2()
          .child(
            div()
              .id("game-name")
              .flex_1()
              .min_w_0()
              .truncate()
              .text_size(spec(GAME_NAME_SIZE))
              .font_weight(FontWeight::BOLD)
              .child(self.game_name(game))
              .test_support(),
          )
          .child(
            Button::new("switch-game")
              .label(text::SWITCH_GAME)
              .ghost()
              .small()
              .on_click(cx.listener(|this, _, window, cx| {
                this.dispatch(Msg::SwitchGamePressed, cx);
                this.empty_package_search(window, cx);
              })),
          ),
      )
      .children(runtime)
      .child(
        Button::new("launch-modded")
          .label(text::LAUNCH_MODDED)
          .primary()
          .w_full()
          .mt_3p5()
          .disabled(!can_launch)
          .on_click(cx.listener(|this, _, _, cx| {
            this.dispatch(Msg::LaunchPressed(LaunchMode::Modded), cx);
          })),
      )
      .child(
        Button::new("launch-vanilla")
          .label(text::LAUNCH_VANILLA)
          .w_full()
          .mt_1p5()
          .disabled(!can_launch)
          .on_click(cx.listener(|this, _, _, cx| {
            this.dispatch(Msg::LaunchPressed(LaunchMode::Vanilla), cx);
          })),
      );
    let profiles_heading = h_flex()
      .flex_none()
      .gap_1()
      .pl_4()
      .pr_2()
      .pt_3p5()
      .pb_1p5()
      .child(
        div()
          .flex_1()
          .min_w_0()
          .truncate()
          .text_small()
          .font_weight(FontWeight::SEMIBOLD)
          .text_color(palette.muted_text)
          .child(text::PROFILES),
      )
      .child(
        Button::new("new-profile")
          .label(text::NEW_PROFILE_BUTTON)
          .ghost()
          .small()
          .tooltip(text::NEW_PROFILE)
          .accessibility_label(text::NEW_PROFILE)
          .disabled(!state.can_switch_profile())
          .on_click(send(&shell, Msg::ProfileNamePressed(Naming::Create))),
      )
      .child(self.render_import_menu(&shell));
    let profiles = v_flex()
      .id("profiles")
      .flex_1()
      .min_h_0()
      .overflow_y_scroll()
      .px_2()
      .pb_1p5()
      .children(self.render_profile_items(&shell, cx));
    let foot = v_flex()
      .flex_none()
      .p_2()
      .border_t_1()
      .border_color(palette.border)
      .child(
        sidebar_item("open-settings", text::SETTINGS).on_click(send(&shell, Msg::SettingsPressed)),
      );

    v_flex()
      .id("sidebar")
      .flex_none()
      .w(spec(SIDEBAR_WIDTH))
      .h_full()
      .bg(palette.panel)
      .border_r_1()
      .border_color(palette.border)
      .child(heading)
      .child(profiles_heading)
      .child(profiles)
      .child(foot)
      .test_support()
      .into_any_element()
  }

  /// The open game's profiles, each with its mod count when known, the
  /// current one selected, pressed for accessibility, and marked with a dot
  /// in the primary color. Clicking one picks it; right-clicking one opens
  /// [`Shell::profile_menu`] for it.
  fn render_profile_items(
    &self,
    shell: &WeakEntity<Self>,
    cx: &mut Context<Self>,
  ) -> Vec<AnyElement> {
    let state = &self.state;
    let palette = Palette::new(cx);
    let primary = cx.theme().primary;
    let current = state.profile.as_ref().map(|profile| profile.name.as_str());
    let can_switch = state.can_switch_profile();

    state
      .profile_names()
      .into_iter()
      .map(|name| {
        let is_current = current == Some(name);
        let picked = name.to_string();
        let dot = if is_current { primary } else { palette.border };
        let count = state.mod_count(name).map(|count| {
          div()
            .id(SharedString::from(format!("mod-count-{name}")))
            .flex_none()
            .text_small()
            .tabular()
            .text_color(palette.muted_text)
            .aria_label(text::mod_count(count))
            .child(count.to_string())
            .test_support()
        });

        Button::new(SharedString::from(format!("pick-profile-{name}")))
          .ghost()
          .w_full()
          .accessibility_label(picked.clone())
          .selected(is_current)
          .toggled(is_current)
          .disabled(!can_switch)
          .child(
            div()
              .flex_none()
              .size(spec(PROFILE_DOT_SIZE))
              .rounded_full()
              .bg(dot),
          )
          .child(div().flex_1().min_w_0().truncate().child(picked.clone()))
          .children(count)
          .on_click(cx.listener(move |this, _, _, cx| {
            this.dispatch(Msg::ProfilePicked(picked.clone()), cx);
          }))
          .context_menu(self.profile_menu(name, shell))
          .into_any_element()
      })
      .collect()
  }

  /// The Import button beside the "Profiles" heading, with its menu of the
  /// places to import from: a code, a file, and r2modman when it holds
  /// profiles for the open game.
  fn render_import_menu(&self, shell: &WeakEntity<Self>) -> impl IntoElement + use<> {
    let state = &self.state;
    let can_import = state.can_import_profile();
    let can_offer_r2modman = state.can_offer_r2modman();
    let shell = shell.clone();
    let button = Button::new("import-profile")
      .label(text::IMPORT_PROFILE_BUTTON)
      .ghost()
      .small()
      .tooltip(text::IMPORT_PROFILE)
      .accessibility_label(text::IMPORT_PROFILE);

    button.dropdown_menu(move |menu, _, _| {
      menu
        .item(
          menu_item("import-code", text::IMPORT_CODE, None)
            .disabled(!can_import)
            .on_click(send(&shell, Msg::ImportCodePressed)),
        )
        .item(
          menu_item("import-file", text::IMPORT_FILE, None)
            .disabled(!can_import)
            .on_click(send(&shell, Msg::ImportFilePressed)),
        )
        .when(can_offer_r2modman, |menu| {
          menu.item(
            menu_item("import-r2modman", text::IMPORT_R2MODMAN, None)
              .on_click(send(&shell, Msg::R2modmanPressed)),
          )
        })
    })
  }

  /// Builds the right-click menu of what can be done to the profile `name`.
  fn profile_menu(
    &self,
    name: &str,
    shell: &WeakEntity<Self>,
  ) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + use<> {
    let state = &self.state;
    let can_rename_or_delete = state.can_rename_or_delete_profile(name);
    let can_duplicate = state.can_duplicate_profile(name);
    let can_export = state.can_export_profile(name);
    let name = name.to_string();
    let shell = shell.clone();

    move |menu, _, cx| {
      let danger = can_rename_or_delete.then(|| Palette::new(cx).danger_text);

      menu
        .item(
          menu_item("rename-profile", text::RENAME_PROFILE, None)
            .disabled(!can_rename_or_delete)
            .on_click(send(
              &shell,
              Msg::ProfileNamePressed(Naming::Rename(name.clone())),
            )),
        )
        .item(
          menu_item("duplicate-profile", text::DUPLICATE_PROFILE, None)
            .disabled(!can_duplicate)
            .on_click(send(
              &shell,
              Msg::ProfileNamePressed(Naming::Duplicate(name.clone())),
            )),
        )
        .item(
          menu_item("share-code", text::SHARE_CODE, None)
            .disabled(!can_export)
            .on_click(send(&shell, Msg::ExportCodePressed(name.clone()))),
        )
        .item(
          menu_item("share-file", text::SHARE_FILE, None)
            .disabled(!can_export)
            .on_click(send(&shell, Msg::ExportFilePressed(name.clone()))),
        )
        .separator()
        .item(
          menu_item("delete-profile", text::DELETE_PROFILE, danger)
            .disabled(!can_rename_or_delete)
            .on_click(send(&shell, Msg::DeleteProfilePressed(name.clone()))),
        )
    }
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
    self
      .state
      .launcher
      .as_ref()
      .and_then(|launcher| launcher.steam.as_ref())
      .and_then(|steam| render_steam_option(steam, cx))
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
fn menu_item(
  id: impl Into<ElementId>,
  label: impl Into<SharedString>,
  color: Option<Hsla>,
) -> PopupMenuItem {
  let id = id.into();
  let label = label.into();

  PopupMenuItem::element(move |_, _| {
    div()
      .id(id.clone())
      .when_some(color, |item, color| item.text_color(color))
      .child(label.clone())
      .test_support()
  })
}

/// `menu` with Thunderstore's sorts, the current one checked with an arrow
/// for its direction. Choosing it again reverses it. Best match is offered
/// only while searching.
fn sort_menu(menu: PopupMenu, filters: &Filters, shell: &WeakEntity<Shell>) -> PopupMenu {
  let best_match = (!filters.search.is_empty()).then_some(SortKey::BestMatch);
  let keys = best_match.into_iter().chain([
    SortKey::LastUpdated,
    SortKey::Name,
    SortKey::Downloads,
    SortKey::Rating,
  ]);

  keys.fold(menu, |menu, key| {
    let current = (filters.sort == key).then_some(filters.direction);
    let id = match key {
      SortKey::BestMatch => "sort-best-match",
      SortKey::LastUpdated => "sort-last-updated",
      SortKey::Name => "sort-name",
      SortKey::Downloads => "sort-downloads",
      SortKey::Rating => "sort-rating",
    };

    menu.item(
      menu_item(id, text::sort(key, current), None)
        .checked(current.is_some())
        .on_click(send(shell, Msg::SortPressed(key))),
    )
  })
}

/// `menu` with a checkbox per one of `categories`, and on Thunderstore one
/// each for showing deprecated and NSFW packages, then Clear filters. On
/// Thunderstore that clears its search and every filter; on the profile's
/// view it clears only the categories, the one filter that view applies.
fn filter_menu(
  menu: PopupMenu,
  tab: Tab,
  categories: &[String],
  filters: &Filters,
  shell: &WeakEntity<Shell>,
) -> PopupMenu {
  let menu = categories.iter().fold(
    menu.scrollable(true).label(text::CATEGORIES),
    |menu, category| {
      menu.item(
        checkbox_item(
          format!("category-{category}"),
          category.clone(),
          filters.categories.contains(category),
        )
        .on_click(send(shell, Msg::CategoryToggled(category.clone()))),
      )
    },
  );
  let clear = shell.clone();

  match tab {
    Tab::Browse => menu
      .separator()
      .item(
        checkbox_item(
          "show-deprecated",
          text::SHOW_DEPRECATED,
          filters.show_deprecated,
        )
        .on_click(send(shell, Msg::ShowDeprecated(!filters.show_deprecated))),
      )
      .item(
        checkbox_item("show-nsfw", text::SHOW_NSFW, filters.show_nsfw)
          .on_click(send(shell, Msg::ShowNsfw(!filters.show_nsfw))),
      )
      .separator()
      .item(
        menu_item("clear-filters", text::CLEAR_FILTERS, None)
          .disabled(!filters.is_filtering())
          .on_click(move |_, window, cx| {
            clear
              .update(cx, |this, cx| {
                this.dispatch(Msg::FiltersCleared, cx);
                this.empty_package_search(window, cx);
              })
              .ok();
          }),
      ),
    Tab::Installed => menu.separator().item(
      menu_item("clear-filters", text::CLEAR_FILTERS, None)
        .disabled(filters.categories.is_empty())
        .on_click(send(shell, Msg::CategoriesCleared)),
    ),
  }
}

/// A menu item holding a checkbox reading `label`, found in tests by `id`.
/// The item's click toggles it, so the checkbox takes no clicks of its own.
fn checkbox_item(
  id: impl Into<ElementId>,
  label: impl Into<SharedString>,
  checked: bool,
) -> PopupMenuItem {
  let id = id.into();
  let label = label.into();

  PopupMenuItem::element(move |_, _| {
    Checkbox::new(id.clone())
      .label(label.clone())
      .checked(checked)
      .tab_stop(false)
  })
}

/// A full-width row of the sidebar reading `label` from its left edge.
fn sidebar_item(id: &'static str, label: &'static str) -> Button {
  Button::new(id)
    .label(label)
    .ghost()
    .w_full()
    .child(div().flex_1())
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
  let mono = Theme::global(cx).mono_font_family.clone();
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
          .font_family(mono)
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
    .text_small()
    .tabular()
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

/// A mod of the open profile's tags, each found in tests by
/// `{scope}{kind}-badge-{full_name}`: the mod loader's, then a newer
/// version, its pin, its deprecation, and the mods it misses.
fn mod_tags(
  state: &State,
  full_name: &str,
  badges: Option<&Badges>,
  scope: &str,
  cx: &App,
) -> Vec<AnyElement> {
  let palette = Palette::new(cx);
  let loader = !state.can_toggle(full_name);
  let tag = |kind: &str, color: Hsla, label: String| {
    state_tag(format!("{scope}{kind}-badge-{full_name}"), color, label)
  };
  let state_tags = badges.into_iter().flat_map(|badges| {
    [
      badges.update.as_deref().map(|version| {
        tag(
          "update",
          palette.tag_update,
          text::update_available(version),
        )
      }),
      badges
        .pinned
        .then(|| tag("pinned", palette.muted_text, text::PINNED.to_string())),
      badges.deprecated.then(|| {
        tag(
          "deprecated",
          palette.tag_deprecated,
          text::DEPRECATED.to_string(),
        )
      }),
      (!badges.missing.is_empty()).then(|| {
        tag(
          "missing",
          palette.tag_missing,
          text::missing_badge(&badges.missing),
        )
      }),
    ]
  });

  loader
    .then(|| tag("loader", palette.tag_loader, text::MOD_LOADER.to_string()))
    .into_iter()
    .chain(state_tags.flatten())
    .collect()
}

/// One mod of the open profile: its icon, name, author and version with its
/// state tags, its main action, and its switch. The mod loader has no switch
/// and is tagged as the loader, and a disabled mod is dimmed. Clicking the
/// row selects it, showing the rest of its actions in the detail pane;
/// right-clicking it opens [`mod_menu`] for it.
fn installed_row(
  state: &State,
  full_name: &str,
  package: &InstalledPackage,
  badges: Option<&Badges>,
  cx: &mut Context<Shell>,
) -> AnyElement {
  let palette = Palette::new(cx);
  let on = cx.theme().success;
  let can_change = state.can_manage();
  let loader = !state.can_toggle(full_name);
  let selected = is_selected(state.readme.as_ref(), full_name);
  let selection = Package::installed(full_name, package);
  let name = package.name(full_name);
  let icon = state
    .browse
    .as_ref()
    .and_then(|browse| browse.index.as_ref())
    .and_then(|index| index.package(full_name))
    .and_then(|row| row.icon());
  let tags = mod_tags(state, full_name, badges, "", cx);
  let pinned = badges.is_some_and(|badges| badges.pinned);
  let outdated = badges.is_some_and(|badges| badges.update.is_some());
  let main = match (pinned, outdated) {
    (true, _) => Some((
      "unpin",
      text::UNPIN,
      Msg::UnpinPressed(full_name.to_string()),
      can_change,
    )),
    (false, true) => Some((
      "update",
      text::UPDATE,
      Msg::UpdatePressed(full_name.to_string()),
      state.can_update(),
    )),
    (false, false) => None,
  }
  .map(|(id, label, msg, enabled)| {
    Button::new(SharedString::from(format!("{id}-{full_name}")))
      .label(label)
      .small()
      .disabled(!enabled)
      .on_click(cx.listener(move |this, _, _, cx| this.dispatch(msg.clone(), cx)))
      .into_any_element()
  });
  let actions = main.into_iter().collect();
  let switch = (!loader).then(|| {
    let toggled = full_name.to_string();

    Switch::new(SharedString::from(format!("switch-{full_name}")))
      .small()
      .checked(package.enabled)
      .color(on)
      .disabled(!can_change)
      .accessibility_label(text::ENABLED)
      .on_click(cx.listener(move |this, enable: &bool, _, cx| {
        let msg = if *enable {
          Msg::EnablePressed(toggled.clone())
        } else {
          Msg::DisablePressed(toggled.clone())
        };

        this.dispatch(msg, cx);
      }))
  });
  let byline = h_flex()
    .gap_1p5()
    .overflow_hidden()
    .child(
      div()
        .id("byline")
        .truncate()
        .text_small()
        .tabular()
        .text_color(palette.muted_text)
        .child(text::byline(&package.author, &package.version))
        .test_support(),
    )
    .children(tags);
  let item = list_row(format!("profile-{full_name}"), selected, palette)
    .on_click(cx.listener(move |this, _, _, cx| {
      this.dispatch(Msg::PackageSelected(selection.clone()), cx);
    }))
    .child(
      row_lead(
        mod_icon(name, icon, ICON_SIZE, cx),
        name_label(name),
        byline,
      )
      .when(!package.enabled, |lead| lead.opacity(DIMMED)),
    )
    .child(actions_slot(full_name, selected, actions))
    .child(div().flex_none().w(spec(SWITCH_WIDTH)).children(switch))
    .test_support()
    .context_menu(mod_menu(state, full_name, &cx.weak_entity()));

  row_slot(item).into_any_element()
}

/// Builds the right-click menu of what can be done to the open profile's mod
/// `full_name`: uninstalling it, as the detail pane's button does.
fn mod_menu(
  state: &State,
  full_name: &str,
  shell: &WeakEntity<Shell>,
) -> impl Fn(PopupMenu, &mut Window, &mut Context<PopupMenu>) -> PopupMenu + use<> {
  let can_uninstall = state.can_take(&Action::Uninstall);
  let msg = Action::Uninstall.msg(full_name);
  let shell = shell.clone();

  move |menu, _, cx| {
    let danger = can_uninstall.then(|| Palette::new(cx).danger_text);

    menu.item(
      menu_item("uninstall-mod", text::UNINSTALL, danger)
        .disabled(!can_uninstall)
        .on_click(send(&shell, msg.clone())),
    )
  }
}

/// A row of either list, found in tests by `id`, painted for whether it is
/// `selected`. It is a [`ROW_GROUP`], so its actions can show while the
/// pointer is over it.
fn list_row(id: impl Into<SharedString>, selected: bool, palette: Palette) -> Stateful<Div> {
  h_flex()
    .id(id.into())
    .group(ROW_GROUP)
    .size_full()
    .overflow_hidden()
    .px_2()
    .gap_3()
    .rounded(spec(ROW_RADIUS))
    .map(|row| paint_row(row, selected, palette))
}

/// A row's icon, then its `first` and `second` lines of text, taking the
/// width the row's other parts leave.
fn row_lead(icon: impl IntoElement, first: impl IntoElement, second: impl IntoElement) -> Div {
  h_flex().flex_1().min_w_0().gap_3().child(icon).child(
    v_flex()
      .flex_1()
      .min_w_0()
      .overflow_hidden()
      .child(first)
      .child(second),
  )
}

/// A mod's icon, `size` spec pixels square, from `url`, or the initials of
/// `name` when there is none or it fails to load.
fn mod_icon(name: &str, url: Option<&str>, size: f32, cx: &App) -> AnyElement {
  let name = SharedString::from(name.to_string());
  let avatar_size = spec(size).to_pixels(cx.theme().font_size);
  let initials = move || {
    div()
      .id("initials")
      .child(
        Avatar::new()
          .name(name.clone())
          .with_size(avatar_size)
          .rounded(spec(ICON_RADIUS)),
      )
      .test_support()
      .into_any_element()
  };
  let icon = match url {
    Some(url) => img(SharedString::from(url.to_string()))
      .size(spec(size))
      .rounded(spec(ICON_RADIUS))
      .with_fallback(initials)
      .into_any_element(),
    None => initials(),
  };

  div()
    .id("icon")
    .flex_none()
    .size(spec(size))
    .child(icon)
    .test_support()
    .into_any_element()
}

/// A game's cover, `width` by `height` spec pixels, from the cached file at
/// `path`, found in tests as `cover`. Until the cover is cached, or when it
/// fails to load, the initials of `name` show in its place.
fn game_cover(name: &str, path: Option<&Path>, width: f32, height: f32, cx: &App) -> AnyElement {
  let name = SharedString::from(name.to_string());
  let avatar_size = spec(width).to_pixels(cx.theme().font_size);
  let initials = move || {
    v_flex()
      .id("initials")
      .size_full()
      .justify_center()
      .child(
        Avatar::new()
          .name(name.clone())
          .with_size(avatar_size)
          .rounded(spec(ICON_RADIUS)),
      )
      .test_support()
      .into_any_element()
  };
  let cover = match path {
    Some(path) => div()
      .id("cover")
      .size_full()
      .child(
        img(path.to_path_buf())
          .size_full()
          .rounded(spec(ICON_RADIUS))
          .with_fallback(initials),
      )
      .test_support()
      .into_any_element(),
    None => initials(),
  };

  div()
    .flex_none()
    .w(spec(width))
    .h(spec(height))
    .child(cover)
    .into_any_element()
}

/// A mod's state as a tag, found in tests by `id`, outlined and lettered in
/// `color`.
fn state_tag(id: String, color: Hsla, label: String) -> AnyElement {
  div()
    .id(SharedString::from(id))
    .flex_none()
    .child(
      Tag::custom(color, color, color)
        .outline()
        .small()
        .rounded_full()
        .text_size(spec(TAG_TEXT_SIZE))
        .child(label),
    )
    .test_support()
    .into_any_element()
}

/// A row's `actions`, at its end in a slot [`ACTIONS_WIDTH`] wide. They show
/// while `shown` or while the pointer is over the row, and hidden they keep
/// their space, so showing them moves nothing.
fn actions_slot(full_name: &str, shown: bool, actions: Vec<AnyElement>) -> AnyElement {
  h_flex()
    .id(SharedString::from(format!("actions-{full_name}")))
    .flex_none()
    .w(spec(ACTIONS_WIDTH))
    .justify_end()
    .gap_1()
    .when(!shown, |slot| {
      slot
        .opacity(0.)
        .group_hover(ROW_GROUP, |slot| slot.opacity(1.))
    })
    .children(actions)
    .test_support()
    .into_any_element()
}

/// A row's name on one line, cut short with an ellipsis when the row is too
/// narrow for it.
fn name_label(name: &str) -> AnyElement {
  div()
    .id("name")
    .truncate()
    .font_weight(FontWeight::SEMIBOLD)
    .child(name.to_string())
    .test_support()
    .into_any_element()
}

/// A row's name and byline on one line, each cut short with an ellipsis
/// when the row is too narrow for it.
fn headline(name: &str, byline: String, muted: Hsla) -> Div {
  h_flex()
    .gap_2()
    .overflow_hidden()
    .child(name_label(name))
    .child(
      div()
        .id("byline")
        .truncate()
        .text_small()
        .text_color(muted)
        .child(byline)
        .test_support(),
    )
}

/// Puts a row in a slot [`ROW_HEIGHT`] tall that leaves a [`ROW_GAP`] to the
/// next, so every row is the same height and the row paints only its own
/// background.
fn row_slot(row: impl IntoElement) -> Div {
  div()
    .flex_none()
    .h(spec(ROW_HEIGHT))
    .py(spec(ROW_GAP / 2.))
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
      .text_small()
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

/// The interface size, as a percentage, for the display `window` is on, at
/// the scale the OS draws it at. 100% when the display is unknown.
fn display_size(window: &Window, cx: &App) -> u16 {
  window.display(cx).map_or(100, |display| {
    theme::automatic_size(window.scale_factor(), display.bounds().size)
  })
}

/// Resizes `window` to gpui's default window size scaled for an interface
/// size of `percent`, so the layout has the room it has at 100%, but no
/// larger than fits in the visible part of its display. gpui cannot move an
/// open window, so the window keeps its top-left corner and grows only as
/// far as the display's right and bottom edges.
fn fit_window(percent: u16, window: &mut Window, cx: &App) {
  let scale = theme::scale(percent);
  let wanted = DEFAULT_WINDOW_SIZE.map(|side| side * scale);
  let content = window.viewport_size();
  let size = window.display(cx).map_or(wanted, |display| {
    wanted.min(&room(window.bounds(), content, display.visible_bounds()))
  });

  if size != content {
    window.resize(size);
  }
}

/// The largest content a window can hold without leaving `visible` while
/// its top-left corner stays put, given its outer `bounds` around content
/// of size `content`. The difference between the two is its frame.
fn room(bounds: Bounds<Pixels>, content: Size<Pixels>, visible: Bounds<Pixels>) -> Size<Pixels> {
  let origin = bounds.origin.max(&visible.origin);

  Size {
    width: visible.right() - origin.x - (bounds.size.width - content.width),
    height: visible.bottom() - origin.y - (bounds.size.height - content.height),
  }
}

/// Fixed so the lists can be virtualized: only the rows in view are built.
/// Tall enough for an icon and two lines of text, with a [`ROW_GAP`].
const ROW_HEIGHT: f32 = 62.0;
/// The space between two rows of a list.
const ROW_GAP: f32 = 6.0;
const ROW_RADIUS: f32 = 6.0;
/// Names a row to the actions that show while the pointer is over it.
const ROW_GROUP: &str = "row";
const ICON_SIZE: f32 = 40.0;
const ICON_RADIUS: f32 = 6.0;
/// The picker's list, centered so its rows don't stretch across a wide
/// window.
const LIST_WIDTH: f32 = 760.0;
/// A cover in the picker's list, in the covers' 3:4 shape.
const LIST_COVER_WIDTH: f32 = 45.0;
const LIST_COVER_HEIGHT: f32 = 60.0;
/// Tall enough for a [`LIST_COVER_HEIGHT`] cover, with a [`ROW_GAP`].
const GAME_ROW_HEIGHT: f32 = 76.0;
/// A cover in the picker's grid, in the covers' 3:4 shape.
const GRID_COVER_WIDTH: f32 = 120.0;
const GRID_COVER_HEIGHT: f32 = 160.0;
/// The space around a cover inside its grid tile, so hovering paints a
/// margin around it.
const GRID_TILE_PADDING: f32 = 6.0;
/// Wide enough for the widest main action.
const ACTIONS_WIDTH: f32 = 84.0;
/// Wide enough for a small switch. The mod loader's row keeps it empty, so
/// the columns line up.
const SWITCH_WIDTH: f32 = 28.0;
/// Wide enough for a count [`text::downloads`] writes.
const DOWNLOADS_WIDTH: f32 = 64.0;
const TAG_TEXT_SIZE: f32 = 11.5;
/// How much of a disabled mod's icon and text shows.
const DIMMED: f32 = 0.55;
/// Tall enough for one line of small text and an extra-small button.
const STATUS_BAR_HEIGHT: f32 = 36.0;
/// Wide enough for the longest label on the settings screen.
const SETTINGS_LABEL_WIDTH: f32 = 140.0;
const SIDEBAR_WIDTH: f32 = 248.0;
const GAME_NAME_SIZE: f32 = 22.0;
const PROFILE_DOT_SIZE: f32 = 7.0;
/// Tall enough for a small input or button, so the bar is one height on both
/// views.
const TOOLBAR_HEIGHT: f32 = 36.0;
/// Wide enough for a three-digit mod count.
const VIEW_COUNT_WIDTH: f32 = 28.0;
const SEARCH_WIDTH: f32 = 340.0;
const SEARCH_MIN_WIDTH: f32 = 160.0;
/// Wide enough for a small ghost button reading `Filter (99)` with its caret.
const FILTER_BUTTON_WIDTH: f32 = 104.0;
/// The padding around a game's main column, beside the sidebar.
const MAIN_PADDING: f32 = 16.0;
/// How much of the width beside the sidebar the detail pane starts with.
const DETAILS_SHARE: f32 = 0.42;
const DETAILS_MIN_WIDTH: f32 = 320.0;
/// Room for a row's icon, its name, and its main action.
const LIST_MIN_WIDTH: f32 = 300.0;
/// Wide enough for a small ghost button reading [`text::DETAILS`] with its
/// icon.
const DETAILS_BUTTON_WIDTH: f32 = 96.0;
const DETAILS_ICON_SIZE: f32 = 64.0;
const DETAILS_NAME_SIZE: f32 = 20.0;
const PANE_RADIUS: f32 = 8.0;
/// Wide enough for the longest label of the detail pane's facts.
const FACT_LABEL_WIDTH: f32 = 96.0;

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

/// The detail pane's facts: the mod's version, then what Thunderstore lists
/// about it when the index has it, each found in tests by its `fact-` id.
fn render_facts(details: &Details) -> impl IntoElement + use<> {
  let fact = |id: &'static str, value: String| {
    div()
      .id(id)
      .tabular()
      .aria_label(value.clone())
      .child(value)
      .test_support()
      .into_any_element()
  };
  let now = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map_or(0, |elapsed| {
      i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX)
    });
  let listed = details.listing.iter().flat_map(|listing| {
    [
      (
        text::DOWNLOADS,
        fact("fact-downloads", text::downloads(listing.downloads)),
      ),
      (
        text::UPDATED,
        fact("fact-updated", text::ago(listing.updated, now)),
      ),
      (
        text::CATEGORIES,
        fact("fact-categories", text::categories(&listing.categories)),
      ),
    ]
  });

  std::iter::once((text::VERSION, fact("fact-version", details.version.clone())))
    .chain(listed)
    .fold(
      DescriptionList::horizontal()
        .columns(1)
        .bordered(false)
        .label_width(spec(FACT_LABEL_WIDTH))
        .small(),
      |facts, (label, value)| facts.item(label, value, 1),
    )
}

/// The readme of the mod the detail pane shows, or why it is not shown.
fn render_readme(readme: &Readme, cx: &mut Context<Shell>) -> AnyElement {
  let palette = Palette::new(cx);
  let message = |id: &'static str, message: String| {
    div()
      .id(id)
      .text_color(palette.muted_text)
      .child(message)
      .test_support()
      .into_any_element()
  };

  match &readme.status {
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
  }
}

/// Thunderstore's packages the search and filters keep, or why there are
/// none, or that they are still loading.
fn render_browse(
  browse: &Browse,
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
          let can_install = state.can_add_mods();

          browse.rows[range]
            .iter()
            .filter_map(|&row| PackageRow::new(&index.0, row))
            .map(|row| {
              let full_name = row.full_name().unwrap_or_default();
              let installed = state.is_installed(full_name);
              let selected = is_selected(state.readme.as_ref(), full_name);

              package_row(row, installed, can_install, selected, cx)
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

      v_flex()
        .flex_1()
        .min_h_0()
        .gap_2()
        .image_cache(icons.clone())
        .when(count == 0, |list| list.child(empty))
        .child(rows)
        .into_any_element()
    }
  }
}

/// One Thunderstore package: its icon, name and author, one line of its
/// description, its downloads, and Install, or a mark when the open profile
/// holds it, always shown. Clicking the row selects it.
fn package_row(
  row: PackageRow<'_>,
  installed: bool,
  can_install: bool,
  selected: bool,
  cx: &mut Context<Shell>,
) -> AnyElement {
  let palette = Palette::new(cx);
  let muted = palette.muted_text;
  let full_name = row.full_name().unwrap_or_default();
  let name = row.name().unwrap_or_default();
  let selection = Package::listed(row);
  let author = headline(name, text::author(row.owner().unwrap_or_default()), muted);
  let description = div()
    .id("description")
    .truncate()
    .text_small()
    .text_color(muted)
    .child(text::one_paragraph(row.description().unwrap_or_default()))
    .test_support();
  let downloads = h_flex()
    .id("downloads")
    .flex_none()
    .w(spec(DOWNLOADS_WIDTH))
    .justify_end()
    .text_small()
    .tabular()
    .text_color(muted)
    .child(text::downloads(row.downloads()))
    .test_support();
  let action = install_action(full_name, installed, can_install, cx);
  let item = list_row(full_name.to_string(), selected, palette)
    .when_some(selection, |row, selection| {
      row.on_click(cx.listener(move |this, _, _, cx| {
        this.dispatch(Msg::PackageSelected(selection.clone()), cx);
      }))
    })
    .child(row_lead(
      mod_icon(name, row.icon(), ICON_SIZE, cx),
      author,
      description,
    ))
    .child(downloads)
    .child(actions_slot(full_name, true, vec![action]))
    .test_support();

  row_slot(item).into_any_element()
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
    theme::apply(
      self.state.settings.theme,
      self.state.settings.appearance,
      window,
      cx,
    );
    // After the theme, since loading one puts back its text size.
    self.apply_size(window, cx);

    if let Some(text) = self.fill_profile_name.take() {
      self.profile_name.update(cx, |input, cx| {
        input.set_value(text, window, cx);
        input.focus(window, cx);
      });
    }

    if let Some(text) = self.fill_launch_arguments.take() {
      self
        .launch_arguments
        .update(cx, |input, cx| input.set_value(text, window, cx));
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
      Screen::Game(game) => self.render_game(&game, window, cx),
      Screen::Settings(game) => self.render_settings(&game, cx),
    };
    let status_bar = (self.state.screen != Screen::Starting).then(|| self.render_status_bar(cx));

    v_flex()
      .size_full()
      .child(div().w_full().flex_1().overflow_hidden().child(screen))
      .children(status_bar)
  }
}
