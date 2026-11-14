//! The gpui shell: draws [`State`], turns input into [`Msg`]s, runs the
//! [`Effect`]s that need the window, and hands the rest to the commands
//! thread.

pub mod text;

use futures::StreamExt as _;
use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender};
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
  AnyElement, AppContext as _, Context, Entity, FontWeight, InteractiveElement as _, IntoElement,
  ParentElement as _, PathPromptOptions, Render, SharedString, StatefulInteractiveElement as _,
  Styled as _, Subscription, Task, TestSupportExt as _, Window, div, rgb,
};
use rivet_core::picker::Step;
use rivet_core::{Command, Effect, Msg, Screen, State};

/// The root view. It owns the state and is the only place `update` is called.
pub struct Shell {
  state: State,
  commands: UnboundedSender<Command>,
  search: Entity<InputState>,
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
            Button::new("switch-game")
              .label(text::SWITCH_GAME)
              .on_click(cx.listener(|this, _, _, cx| this.dispatch(Msg::SwitchGamePressed, cx))),
          ),
      )
      .children(folder)
      .children(
        self
          .state
          .settings_error
          .as_ref()
          .map(|error| failure(text::settings_error(error))),
      )
      .into_any_element()
  }
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
