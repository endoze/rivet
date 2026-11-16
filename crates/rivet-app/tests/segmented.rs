use futures::channel::mpsc::unbounded;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, App, AppContext as _, TestAppContext, Window, WindowOptions};
use rivet_app::Shell;
use rivet_core::install::DEFAULT_PROFILE;
use rivet_core::settings::{GameSettings, Settings};
use rivet_core::{Command, Msg, State};
use thunderstore_engine::ecosystem::Ecosystem;
use thunderstore_engine::profile::launch::HostOs;

#[gpui_kit::test]
fn the_views_control_marks_only_the_chosen_view_selected(cx: &mut TestAppContext) {
  let window = open_valheim_on_linux(cx);
  let views = ["tab-installed", "tab-browse"];

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    for chosen in views {
      window.click(chosen, cx);
      window.render_frame(cx);

      assert_only_selected(window, &views, chosen);
    }
  })
  .unwrap();
}

#[gpui_kit::test]
fn the_runtime_control_marks_only_the_chosen_runtime_selected(cx: &mut TestAppContext) {
  let window = open_valheim_on_linux(cx);
  let runtimes = ["runtime-detect", "runtime-native", "runtime-proton"];

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("open-settings", cx);
    window.render_frame(cx);

    assert_only_selected(window, &runtimes, "runtime-detect");

    for chosen in ["runtime-proton", "runtime-native"] {
      window.click(chosen, cx);
      window.render_frame(cx);

      assert_only_selected(window, &runtimes, chosen);
    }
  })
  .unwrap();
}

#[gpui_kit::test]
fn the_appearance_control_marks_only_the_chosen_appearance_selected(cx: &mut TestAppContext) {
  let window = open_valheim_on_linux(cx);
  let appearances = ["appearance-system", "appearance-light", "appearance-dark"];

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("open-settings", cx);
    window.render_frame(cx);

    assert_only_selected(window, &appearances, "appearance-system");

    for chosen in ["appearance-dark", "appearance-light"] {
      window.click(chosen, cx);
      window.render_frame(cx);

      assert_only_selected(window, &appearances, chosen);
    }
  })
  .unwrap();
}

#[gpui_kit::test]
fn tab_reaches_a_segment_and_enter_or_space_chooses_it(cx: &mut TestAppContext) {
  let window = open_valheim_on_linux(cx);
  let runtimes = ["runtime-detect", "runtime-native", "runtime-proton"];

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("open-settings", cx);
    window.render_frame(cx);

    for (chosen, key) in [("runtime-proton", "enter"), ("runtime-native", "space")] {
      tab_to(window, chosen, cx);
      window.press(key, cx);
      window.render_frame(cx);

      assert_only_selected(window, &runtimes, chosen);
    }
  })
  .unwrap();
}

/// Presses Tab until the element `id` has keyboard focus, failing if a few
/// dozen presses never reach it. Focus starts on the first tab stop, as
/// Tab moves focus only once something in the window has it.
fn tab_to(window: &mut Window, id: &'static str, cx: &mut App) {
  if window.focused(cx).is_none() {
    window.focus_next(cx);
  }

  for _ in 0..40 {
    window.press("tab", cx);
    window.render_frame(cx);

    if window.find(id).focused() == Some(true) {
      return;
    }
  }

  panic!("Tab never reached {id}");
}

/// Asserts that of `segments`, `chosen` alone reports itself checked.
fn assert_only_selected(window: &mut Window, segments: &[&'static str], chosen: &str) {
  for segment in segments {
    assert_eq!(
      window.find(*segment).checked(),
      Some(*segment == chosen),
      "with {chosen} chosen, {segment} is not marked as it should be"
    );
  }
}

/// Opens Valheim's default profile on Linux, where settings offer a runtime.
fn open_valheim_on_linux(cx: &mut TestAppContext) -> AnyWindowHandle {
  cx.update(gpui_kit::init);

  let (commands, _sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let mut state = State::new(one_game());

  state.host = Some(HostOs::Linux);

  let (window, _) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(state, commands, inbox, window, cx))
    })
    .expect("open test window")
  });
  let settings = Settings {
    last_game: Some("valheim".to_string()),
    games: [("valheim".to_string(), GameSettings::new("/games/Valheim"))].into(),
    ..Settings::default()
  };

  for msg in [
    Msg::SettingsLoaded(Ok(settings)),
    Msg::ProfileOpened {
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      installed: Ok([].into()),
    },
  ] {
    answers.unbounded_send(msg).unwrap();
  }

  cx.run_until_parked();

  window
}

fn one_game() -> Ecosystem {
  Ecosystem::from_json(
    r#"{
      "games": {
        "valheim": {
          "label": "valheim",
          "meta": { "displayName": "Valheim" },
          "r2modman": [{
            "gameInstanceType": "game",
            "exeNames": ["valheim.x86_64"],
            "packageIndex": "https://thunderstore.io/c/valheim/api/v1/package/"
          }]
        }
      },
      "modloaderPackages": [
        { "packageId": "denikson-BepInExPack_Valheim", "rootFolder": "BepInExPack_Valheim", "loader": "bepinex" }
      ]
    }"#,
  )
  .unwrap()
}
