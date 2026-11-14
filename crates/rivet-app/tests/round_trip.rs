use futures::channel::mpsc::unbounded;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, TestAppContext, WindowOptions};
use rivet_app::Shell;
use rivet_core::browse::{Fetch, Index, Refresh};
use rivet_core::settings::{GameSettings, Settings};
use rivet_core::{Command, Msg, Screen, State};
use std::path::PathBuf;
use thunderstore_engine::ecosystem::Ecosystem;
use thunderstore_engine::models::{Package, PackageIndex};
use thunderstore_engine::test_support::index_json;

/// The mock Thunderstore's five packages, `Owner-ModA` first.
fn mock_index() -> Index {
  let packages: Vec<Package> = serde_json::from_str(&index_json("http://mock")).unwrap();

  PackageIndex::from(packages).into()
}

#[gpui_kit::test]
fn the_package_list_shows_its_rows_and_refreshes_on_demand(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let (commands, mut sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let (window, _) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(State::new(one_game()), commands, inbox, window, cx))
    })
    .expect("open test window")
  });
  let valheim = Settings {
    last_game: Some("valheim".to_string()),
    games: [(
      "valheim".to_string(),
      GameSettings {
        folder: PathBuf::from("/games/Valheim"),
      },
    )]
    .into(),
  };

  assert_eq!(sent.try_recv().ok(), Some(Command::LoadSettings));

  answers
    .unbounded_send(Msg::SettingsLoaded(Ok(valheim)))
    .unwrap();
  cx.run_until_parked();

  assert_eq!(
    sent.try_recv().ok(),
    Some(Command::ReadCachedIndex("valheim".to_string()))
  );

  answers
    .unbounded_send(Msg::CachedIndexRead {
      community: "valheim".to_string(),
      index: Ok(Some(mock_index())),
    })
    .unwrap();
  cx.run_until_parked();

  assert_eq!(
    sent.try_recv().ok(),
    Some(Command::RefreshIndex {
      refresh: valheim_refresh(1),
      fetch: Fetch::IfStale,
    })
  );

  answers
    .unbounded_send(Msg::IndexRefreshed {
      refresh: valheim_refresh(1),
      index: Err("offline".to_string()),
    })
    .unwrap();
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    assert!(window.try_find("Owner-ModA").is_some());
    assert!(window.try_find("stale").is_some());

    window.click("refresh", cx);
  })
  .unwrap();

  assert_eq!(
    sent.try_recv().ok(),
    Some(Command::RefreshIndex {
      refresh: valheim_refresh(2),
      fetch: Fetch::Always,
    })
  );
}

fn valheim_refresh(id: u64) -> Refresh {
  Refresh {
    community: "valheim".to_string(),
    id,
  }
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
      }
    }"#,
  )
  .unwrap()
}

#[gpui_kit::test]
fn picking_a_game_and_its_folder_opens_the_game(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let folder = PathBuf::from("/games/Valheim");
  let (commands, mut sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let (window, shell) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(State::new(one_game()), commands, inbox, window, cx))
    })
    .expect("open test window")
  });

  assert_eq!(sent.try_recv().ok(), Some(Command::LoadSettings));

  answers
    .unbounded_send(Msg::SettingsLoaded(Ok(Settings::default())))
    .unwrap();
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("valheim", cx);
  })
  .unwrap();

  assert!(cx.did_prompt_for_paths());

  cx.simulate_path_prompt_response(|_| Some(vec![folder.clone()]));
  cx.run_until_parked();

  assert_eq!(
    sent.try_recv().ok(),
    Some(Command::ListFolder(folder.clone()))
  );

  answers
    .unbounded_send(Msg::FolderListed {
      folder,
      entries: Ok(vec!["valheim.x86_64".to_string()]),
    })
    .unwrap();
  cx.run_until_parked();

  assert_eq!(
    shell.read_with(cx, |shell, _| shell.state().screen.clone()),
    Screen::Game("valheim".to_string())
  );

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("switch-game", cx);
  })
  .unwrap();

  assert_eq!(
    shell.read_with(cx, |shell, _| shell.state().screen.clone()),
    Screen::Picker
  );
}
