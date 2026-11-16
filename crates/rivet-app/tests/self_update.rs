use futures::channel::mpsc::unbounded;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, TestAppContext, WindowOptions};
use rivet_app::Shell;
use rivet_core::self_update::{Asset, Release};
use rivet_core::settings::{GameSettings, Settings};
use rivet_core::{Command, Msg, State};
use std::path::PathBuf;
use thunderstore_engine::ecosystem::Ecosystem;

const BUILD: &str = "rivet-linux-x86_64";

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
      }
    }"#,
  )
  .unwrap()
}

fn asset(name: &str) -> Asset {
  Asset {
    name: name.to_string(),
    url: format!("https://github.com/endoze/rivet/releases/download/v0.3.0/{name}"),
  }
}

/// Clicks the element `id`, then lets every message it caused land.
fn click(cx: &mut TestAppContext, window: AnyWindowHandle, id: &'static str) {
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click(id, cx);
    window.render_frame(cx);
  })
  .unwrap();
  cx.run_until_parked();
}

fn shows(cx: &mut TestAppContext, window: AnyWindowHandle, id: &'static str) -> bool {
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    window.try_find(id).is_some()
  })
  .unwrap()
}

/// GitHub's answer to a check: Rivet 0.3.0, with the Linux build.
fn newer_found() -> Msg {
  Msg::RivetReleasesFetched(Ok(vec![Release {
    tag: "v0.3.0".to_string(),
    draft: false,
    prerelease: false,
    page: "https://github.com/endoze/rivet/releases/tag/v0.3.0".to_string(),
    assets: vec![asset(BUILD), asset(&format!("{BUILD}.sha256"))],
  }]))
}

/// A state running Rivet 0.2.0 on Linux.
fn running_0_2_0() -> State {
  let mut state = State::new(one_game());

  state.self_update.running = "0.2.0".to_string();
  state.self_update.asset = Some(BUILD.to_string());

  state
}

#[gpui_kit::test]
fn turning_down_a_newer_rivet_downloads_nothing(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let (commands, mut sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let state = running_0_2_0();
  let (window, _) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(state, commands, inbox, window, cx))
    })
    .expect("open test window")
  });
  let settings = Settings {
    update_automatically: true,
    ..Settings::default()
  };

  answers
    .unbounded_send(Msg::SettingsLoaded(Ok(settings)))
    .unwrap();
  cx.run_until_parked();
  answers.unbounded_send(newer_found()).unwrap();
  cx.run_until_parked();

  assert!(
    shows(cx, window, "decline-rivet-update"),
    "the check on start offered nothing"
  );

  click(cx, window, "decline-rivet-update");

  assert!(
    !std::iter::from_fn(|| sent.try_recv().ok())
      .any(|command| matches!(command, Command::DownloadRivet(_))),
    "turning the update down downloaded it"
  );
  assert!(
    !shows(cx, window, "rivet-update-dialog"),
    "the offer stayed open"
  );
}

#[gpui_kit::test]
fn settings_check_for_a_newer_rivet_and_the_status_bar_restarts_into_it(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let (commands, mut sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let state = running_0_2_0();
  let (window, _) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(state, commands, inbox, window, cx))
    })
    .expect("open test window")
  });
  let mut drain = move || std::iter::from_fn(|| sent.try_recv().ok()).collect::<Vec<_>>();
  let settings = Settings {
    last_game: Some("valheim".to_string()),
    games: [("valheim".to_string(), GameSettings::new("/games/Valheim"))].into(),
    ..Settings::default()
  };

  answers
    .unbounded_send(Msg::SettingsLoaded(Ok(settings)))
    .unwrap();
  cx.run_until_parked();
  click(cx, window, "open-settings");
  drain();

  assert!(shows(cx, window, "rivet-version"));
  assert_eq!(
    cx.update_window(window, |_, window, _| window
      .find("update-automatically")
      .checked())
      .unwrap(),
    Some(false)
  );

  click(cx, window, "update-automatically");

  assert!(
    drain()
      .iter()
      .any(|command| matches!(command, Command::SaveSettings(saved) if saved.update_automatically)),
    "turning updating on was not saved"
  );

  answers.unbounded_send(Msg::SettingsSaved(Ok(()))).unwrap();
  cx.run_until_parked();

  click(cx, window, "check-for-updates");

  assert_eq!(drain(), vec![Command::FetchRivetReleases]);

  answers.unbounded_send(newer_found()).unwrap();
  cx.run_until_parked();

  assert_eq!(drain(), vec![], "a newer Rivet downloaded before asking");
  assert!(
    shows(cx, window, "accept-rivet-update"),
    "a newer Rivet was not offered"
  );

  click(cx, window, "accept-rivet-update");

  assert!(matches!(
    drain().as_slice(),
    [Command::DownloadRivet(download)] if download.found.version == "0.3.0"
  ));
  assert!(
    shows(cx, window, "cancel-rivet-download"),
    "the download is not in the status bar"
  );

  answers
    .unbounded_send(Msg::RivetDownloaded(Ok(PathBuf::from(
      "/opt/rivet/rivet.new",
    ))))
    .unwrap();
  cx.run_until_parked();
  click(cx, window, "restart-to-update");

  assert_eq!(
    drain(),
    vec![Command::ReplaceRivet {
      file: PathBuf::from("/opt/rivet/rivet.new"),
      to: "0.3.0".to_string(),
    }]
  );
}
