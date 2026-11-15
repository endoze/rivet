use futures::channel::mpsc::unbounded;
use gpui_kit::component::WindowExt as _;
use gpui_kit::test::{ElementSnapshot, TestQueryExt as _, TestWindowExt as _};
use gpui_kit::{
  AnyWindowHandle, AppContext as _, Role, TestAppContext, VisualTestContext, Window, WindowOptions,
};
use rivet_app::Shell;
use rivet_core::browse::{Fetch, Index, Refresh};
use rivet_core::install::{DEFAULT_PROFILE, Installed, InstalledPackage, Outcome, Progress, Stage};
use rivet_core::readme::{self, Request};
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

/// Whether a toast holding the element `id` is up, drawing the window twice:
/// once to raise any toast waiting, and once to show it.
fn toasted(cx: &mut TestAppContext, window: AnyWindowHandle, id: &'static str) -> bool {
  cx.update_window(window, |_, window, cx| window.render_frame(cx))
    .unwrap();
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    window.try_find(id).is_some()
  })
  .unwrap()
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
        profile: None,
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
  assert_eq!(sent.try_recv().ok(), Some(open_default_profile()));

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

  assert!(
    toasted(cx, window, "notice-warning"),
    "a failed refresh was not told"
  );

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    assert!(window.try_find("Owner-ModA").is_some());

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

#[gpui_kit::test]
fn searching_sorting_and_clearing_the_package_list(cx: &mut TestAppContext) {
  use rivet_core::browse::{Filters, SortDirection, SortKey};

  cx.update(gpui_kit::init);

  let (commands, _sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let (window, shell) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(State::new(one_game()), commands, inbox, window, cx))
    })
    .expect("open test window")
  });

  for msg in [
    Msg::SettingsLoaded(Ok(valheim_settings())),
    Msg::CachedIndexRead {
      community: "valheim".to_string(),
      index: Ok(Some(mock_index())),
    },
  ] {
    answers.unbounded_send(msg).unwrap();
  }

  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    assert!(window.try_find("sort-best-match").is_none());

    window.click("package-search", cx);
    window.input("modc", cx);
  })
  .unwrap();
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    assert!(window.try_find("Owner-ModC").is_some());
    assert!(window.try_find("Owner-ModA").is_none());
    assert!(window.try_find("sort-best-match").is_some());

    window.click("sort-name", cx);
    window.click("show-deprecated", cx);
    window.render_frame(cx);
    window.click("clear-filters", cx);
    window.render_frame(cx);

    assert!(window.try_find("Owner-ModA").is_some());
    assert!(window.try_find("sort-best-match").is_none());
  })
  .unwrap();

  let filters = shell.read_with(cx, |shell, _| {
    shell.state().browse.as_ref().unwrap().filters.clone()
  });

  assert_eq!(
    filters,
    Filters {
      sort: SortKey::Name,
      direction: SortDirection::Ascending,
      ..Filters::default()
    }
  );
}

fn open_default_profile() -> Command {
  Command::OpenProfile {
    game: "valheim".to_string(),
    profile: DEFAULT_PROFILE.to_string(),
  }
}

fn valheim_settings() -> Settings {
  Settings {
    last_game: Some("valheim".to_string()),
    games: [(
      "valheim".to_string(),
      GameSettings {
        folder: PathBuf::from("/games/Valheim"),
        profile: None,
      },
    )]
    .into(),
  }
}

#[gpui_kit::test]
fn installing_a_package_confirms_then_marks_it_installed(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let (commands, mut sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let (window, _) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(State::new(one_game()), commands, inbox, window, cx))
    })
    .expect("open test window")
  });

  for msg in [
    Msg::SettingsLoaded(Ok(valheim_settings())),
    Msg::CachedIndexRead {
      community: "valheim".to_string(),
      index: Ok(Some(mock_index())),
    },
    Msg::ProfileOpened {
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      installed: Ok(Installed::new()),
    },
  ] {
    answers.unbounded_send(msg).unwrap();
  }

  cx.run_until_parked();

  let tabs = cx
    .update_window(window, |_, window, cx| {
      window.render_frame(cx);

      let tabs = window.find("tab-browse").bounds();

      window.click("install-Owner-ModC", cx);

      tabs
    })
    .unwrap();

  settle(cx, window);

  assert!(dialog_open(cx, window), "install opened no modal");

  cx.update_window(window, |_, window, cx| {
    assert_eq!(
      window.find("tab-browse").bounds(),
      tabs,
      "the confirmation moved the tabs"
    );
    assert!(
      window
        .within("install-dialog")
        .try_find("confirm-install")
        .is_some(),
      "the confirmation is not in the modal"
    );

    window.click("confirm-install", cx);
  })
  .unwrap();
  settle(cx, window);

  assert!(!dialog_open(cx, window), "confirming left the modal open");

  let install = std::iter::from_fn(|| sent.try_recv().ok())
    .find_map(|command| match command {
      Command::Install(install) => Some(install),
      _ => None,
    })
    .expect("an install was sent");

  assert_eq!(install.package, "Owner-ModC");

  answers
    .unbounded_send(Msg::OperationFinished {
      operation: install.operation,
      outcome: Ok(Outcome {
        changed: vec!["Owner-ModA".to_string(), "Owner-ModC".to_string()],
        failed: vec![],
        cancelled: vec![],
        profile: Ok(
          [
            package("Owner-ModA", &[]),
            package("Owner-ModC", &["Owner-ModA"]),
          ]
          .into(),
        ),
      }),
    })
    .unwrap();
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    assert!(window.try_find("installed-Owner-ModC").is_some());
    assert!(window.try_find("install-Owner-ModC").is_none());
    assert!(window.try_find("operation-1").is_none());
  })
  .unwrap();

  assert!(
    toasted(cx, window, "notice-success"),
    "the finished install was not told"
  );
}

#[gpui_kit::test]
fn closing_the_install_modal_any_way_installs_nothing(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let (commands, mut sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let (window, shell) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(State::new(one_game()), commands, inbox, window, cx))
    })
    .expect("open test window")
  });

  for msg in [
    Msg::SettingsLoaded(Ok(valheim_settings())),
    Msg::CachedIndexRead {
      community: "valheim".to_string(),
      index: Ok(Some(mock_index())),
    },
    Msg::ProfileOpened {
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      installed: Ok(Installed::new()),
    },
  ] {
    answers.unbounded_send(msg).unwrap();
  }

  cx.run_until_parked();

  for how in ["escape", "close", "dismiss-install"] {
    cx.update_window(window, |_, window, cx| {
      window.render_frame(cx);
      window.click("install-Owner-ModC", cx);
    })
    .unwrap();
    settle(cx, window);

    assert!(dialog_open(cx, window), "install opened no modal");

    cx.update_window(window, |_, window, cx| {
      if how == "escape" {
        window.press("escape", cx);
      } else {
        window.click(how, cx);
      }
    })
    .unwrap();
    settle(cx, window);

    assert!(!dialog_open(cx, window), "{how} left the modal open");
    assert_eq!(
      shell.read_with(cx, |shell, _| shell.state().confirmation.clone()),
      None,
      "{how} left the install pending"
    );
  }

  let installed = std::iter::from_fn(|| sent.try_recv().ok())
    .any(|command| matches!(command, Command::Install(_)));

  assert!(!installed, "closing the modal installed something");
}

#[gpui_kit::test]
fn closing_while_an_install_runs_asks_then_cancels_it_and_closes(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let (commands, mut sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let (window, _) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(State::new(one_game()), commands, inbox, window, cx))
    })
    .expect("open test window")
  });

  for msg in [
    Msg::SettingsLoaded(Ok(valheim_settings())),
    Msg::CachedIndexRead {
      community: "valheim".to_string(),
      index: Ok(Some(mock_index())),
    },
    Msg::ProfileOpened {
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      installed: Ok(Installed::new()),
    },
  ] {
    answers.unbounded_send(msg).unwrap();
  }

  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("install-Owner-ModC", cx);
  })
  .unwrap();
  settle(cx, window);
  cx.update_window(window, |_, window, cx| {
    window.click("confirm-install", cx);
  })
  .unwrap();
  settle(cx, window);
  cx.update_window(window, |_, window, _| {
    assert!(window.try_find("operation-1").is_some());
    assert!(window.try_find("cancel-operation-1").is_some());
  })
  .unwrap();

  cx.update_window(window, |_, window, cx| {
    window.click("install-Owner-ModA", cx);
  })
  .unwrap();
  settle(cx, window);

  assert!(dialog_open(cx, window), "the second install asked nothing");

  let mut visual = VisualTestContext::from_window(window, cx);

  assert!(!visual.simulate_close());

  settle(cx, window);
  cx.update_window(window, |_, window, cx| {
    assert!(
      window.within("closing").try_find("close-cancel").is_some(),
      "the close question is not in the modal"
    );
    assert!(
      window.try_find("install-dialog").is_none(),
      "the install modal still covers the close question"
    );

    window.click("close-cancel", cx);
  })
  .unwrap();

  let cancelled =
    std::iter::from_fn(|| sent.try_recv().ok()).any(|command| command == Command::Cancel(1));

  assert!(cancelled);

  answers
    .unbounded_send(Msg::OperationFinished {
      operation: 1,
      outcome: Ok(Outcome {
        changed: vec![],
        failed: vec![],
        cancelled: vec!["Owner-ModC".to_string()],
        profile: Ok(Installed::new()),
      }),
    })
    .unwrap();
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| window.render_frame(cx))
    .ok();
  cx.run_until_parked();

  assert!(cx.update_window(window, |_, _, _| ()).is_err());
}

/// An enabled 1.0.0 `Owner-` package needing `dependencies`.
fn package(full_name: &str, dependencies: &[&str]) -> (String, InstalledPackage) {
  (
    full_name.to_string(),
    InstalledPackage {
      author: "Owner".to_string(),
      version: "1.0.0".to_string(),
      enabled: true,
      dependencies: dependencies.iter().map(|name| name.to_string()).collect(),
      installed_at: 0,
    },
  )
}

#[gpui_kit::test]
fn the_installed_tab_asks_before_disabling_a_dependency_and_the_disable_can_be_cancelled(
  cx: &mut TestAppContext,
) {
  cx.update(gpui_kit::init);

  let (commands, mut sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let (window, _) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(State::new(one_game()), commands, inbox, window, cx))
    })
    .expect("open test window")
  });

  for msg in [
    Msg::SettingsLoaded(Ok(valheim_settings())),
    Msg::ProfileOpened {
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      installed: Ok(
        [
          package("Owner-ModA", &[]),
          package("Owner-ModC", &["Owner-ModA"]),
        ]
        .into(),
      ),
    },
  ] {
    answers.unbounded_send(msg).unwrap();
  }

  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    assert!(window.try_find("profile-Owner-ModA").is_none());

    window.click("tab-installed", cx);
    window.render_frame(cx);

    assert!(window.try_find("profile-Owner-ModA").is_some());
    assert!(window.try_find("enabled-Owner-ModC").is_some());

    window.click("disable-Owner-ModA", cx);
    window.render_frame(cx);

    assert!(window.try_find("dependants-question").is_some());

    window.click("with-dependants", cx);
  })
  .unwrap();

  let set = std::iter::from_fn(|| sent.try_recv().ok())
    .find_map(|command| match command {
      Command::SetEnabled(set) => Some(set),
      _ => None,
    })
    .expect("a disable was sent");

  assert_eq!(set.operation, 1);
  assert_eq!(set.packages, ["Owner-ModA", "Owner-ModC"]);
  assert!(!set.enabled);

  answers
    .unbounded_send(Msg::OperationProgress {
      operation: 1,
      progress: Progress {
        package: "Owner-ModC".to_string(),
        stage: Stage::Disabling,
        done: 0,
        total: None,
      },
    })
    .unwrap();
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("cancel-operation-1", cx);
  })
  .unwrap();

  let cancelled =
    std::iter::from_fn(|| sent.try_recv().ok()).any(|command| command == Command::Cancel(1));

  assert!(cancelled);
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
    window.click("game-menu", cx);
    window.render_frame(cx);
    window.click("switch-game", cx);
  })
  .unwrap();

  assert_eq!(
    shell.read_with(cx, |shell, _| shell.state().screen.clone()),
    Screen::Picker
  );
}

/// Valheim as a native BepInEx game on Steam.
fn native_steam_game() -> Ecosystem {
  Ecosystem::from_json(
    r#"{
      "games": {
        "valheim": {
          "label": "valheim",
          "meta": { "displayName": "Valheim" },
          "r2modman": [{
            "gameInstanceType": "game",
            "packageLoader": "bepinex",
            "exeNames": ["valheim.x86_64"],
            "distributions": [{ "platform": "steam", "identifier": "892970" }],
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

#[gpui_kit::test]
fn picking_a_steam_game_finds_its_folder_with_no_user_action(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let folder = PathBuf::from("/steam/steamapps/common/Valheim");
  let (commands, mut sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let (window, shell) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(State::new(native_steam_game()), commands, inbox, window, cx))
    })
    .expect("open test window")
  });

  answers
    .unbounded_send(Msg::SettingsLoaded(Ok(Settings::default())))
    .unwrap();
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("valheim", cx);
  })
  .unwrap();

  assert_eq!(
    std::iter::from_fn(|| sent.try_recv().ok()).last(),
    Some(Command::FindInSteam {
      game: "valheim".to_string(),
      app_id: 892970,
    })
  );

  answers
    .unbounded_send(Msg::SteamSearched {
      game: "valheim".to_string(),
      folder: Ok(folder.clone()),
    })
    .unwrap();
  cx.run_until_parked();

  assert_eq!(
    sent.try_recv().ok(),
    Some(Command::ListFolder(folder.clone()))
  );

  answers
    .unbounded_send(Msg::FolderListed {
      folder: folder.clone(),
      entries: Ok(vec!["valheim.x86_64".to_string()]),
    })
    .unwrap();
  cx.run_until_parked();

  assert!(!cx.did_prompt_for_paths());
  assert_eq!(
    shell.read_with(cx, |shell, _| shell.state().screen.clone()),
    Screen::Game("valheim".to_string())
  );
  assert_eq!(
    shell.read_with(cx, |shell, _| shell
      .state()
      .settings
      .folder("valheim")
      .map(PathBuf::from)),
    Some(folder)
  );
}

#[gpui_kit::test]
fn a_steam_game_steam_does_not_have_asks_for_its_folder_saying_why(cx: &mut TestAppContext) {
  use rivet_core::picker::NotFound;

  cx.update(gpui_kit::init);

  let (commands, _sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let (window, _) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(State::new(native_steam_game()), commands, inbox, window, cx))
    })
    .expect("open test window")
  });

  answers
    .unbounded_send(Msg::SettingsLoaded(Ok(Settings::default())))
    .unwrap();
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("valheim", cx);
  })
  .unwrap();
  answers
    .unbounded_send(Msg::SteamSearched {
      game: "valheim".to_string(),
      folder: Err(NotFound::NotInstalled),
    })
    .unwrap();
  cx.run_until_parked();

  assert!(cx.did_prompt_for_paths());

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    assert!(window.try_find("steam-fallback").is_some());
  })
  .unwrap();
}

#[cfg(target_os = "linux")]
#[gpui_kit::test]
fn a_native_steam_game_shows_its_launch_option_and_launches(cx: &mut TestAppContext) {
  use rivet_core::launch::SteamOptionRead;
  use thunderstore_engine::profile::launch::LaunchMode;

  cx.update(gpui_kit::init);

  let line = r#""/data/valheim/steam-wrapper.sh" %command%"#;
  let (commands, mut sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let (window, _) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(State::new(native_steam_game()), commands, inbox, window, cx))
    })
    .expect("open test window")
  });

  answers
    .unbounded_send(Msg::SettingsLoaded(Ok(valheim_settings())))
    .unwrap();
  cx.run_until_parked();

  let read = std::iter::from_fn(|| sent.try_recv().ok()).find(|command| {
    matches!(command, Command::ReadSteamOption { game, app_id: 892970 } if game == "valheim")
  });

  assert!(read.is_some(), "Steam's launch options were not read");

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    assert!(
      window.try_find("launch-option").is_none(),
      "the line showed before Steam was read"
    );
  })
  .unwrap();

  for msg in [
    Msg::ProfileOpened {
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      installed: Ok(Installed::new()),
    },
    Msg::SteamOptionRead(SteamOptionRead {
      game: "valheim".to_string(),
      line: line.to_string(),
      current: Ok(None),
    }),
  ] {
    answers.unbounded_send(msg).unwrap();
  }

  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    assert!(window.try_find("launch-option-not-set").is_some());
    assert!(window.try_find("missing-loader").is_some());

    window.click("copy-launch-option", cx);
    window.click("launch-vanilla", cx);
  })
  .unwrap();

  assert_eq!(
    cx.read_from_clipboard().and_then(|item| item.text()),
    Some(line.to_string())
  );
  assert!(
    toasted(cx, window, "notice-info"),
    "copying the line was not told"
  );

  let launch = std::iter::from_fn(|| sent.try_recv().ok())
    .find_map(|command| match command {
      Command::Launch(launch) => Some(launch),
      _ => None,
    })
    .expect("a launch was sent");

  assert_eq!(launch.context.mode, LaunchMode::Vanilla);
  assert_eq!(launch.folder, PathBuf::from("/games/Valheim"));

  answers
    .unbounded_send(Msg::SteamOptionRead(SteamOptionRead {
      game: "valheim".to_string(),
      line: line.to_string(),
      current: Ok(Some(line.to_string())),
    }))
    .unwrap();
  cx.run_until_parked();

  assert!(
    toasted(cx, window, "notice-success"),
    "Steam being set up was not told"
  );

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    assert!(window.try_find("launch-option").is_none());
  })
  .unwrap();
}

/// The menu row holding the element `id`, which carries the row's check.
fn menu_row(window: &Window, id: &'static str) -> ElementSnapshot {
  let inner = window.find(id);

  window
    .elements()
    .into_iter()
    .find(|row| row.role() == Some(Role::MenuItem) && inner.path().starts_with(row.path()))
    .unwrap_or_else(|| panic!("{id} is not in a menu row"))
}

/// Draws the window, gives the shell time to open or close a dialog, and
/// draws it again.
fn settle(cx: &mut TestAppContext, window: AnyWindowHandle) {
  cx.update_window(window, |_, window, cx| window.render_frame(cx))
    .unwrap();
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| window.render_frame(cx))
    .unwrap();
}

/// Whether a dialog is open and not on its way out.
fn dialog_open(cx: &mut TestAppContext, window: AnyWindowHandle) -> bool {
  cx.update_window(window, |_, window, cx| window.has_active_dialog(cx))
    .unwrap()
}

#[gpui_kit::test]
fn the_profile_header_picks_names_and_deletes_profiles(cx: &mut TestAppContext) {
  use rivet_core::switcher::ProfileChange;

  cx.update(gpui_kit::init);

  let (commands, mut sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let (window, _) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(State::new(one_game()), commands, inbox, window, cx))
    })
    .expect("open test window")
  });
  let opened = |profile: &str| Msg::ProfileOpened {
    game: "valheim".to_string(),
    profile: profile.to_string(),
    installed: Ok(Installed::new()),
  };
  let mut drain = move || std::iter::from_fn(|| sent.try_recv().ok()).collect::<Vec<_>>();

  for msg in [
    Msg::SettingsLoaded(Ok(valheim_settings())),
    Msg::ProfileOpened {
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      installed: Ok([package("Owner-ModA", &[])].into()),
    },
    Msg::ProfilesListed {
      game: "valheim".to_string(),
      names: Ok(vec![DEFAULT_PROFILE.to_string(), "Modded".to_string()]),
    },
  ] {
    answers.unbounded_send(msg).unwrap();
  }

  cx.run_until_parked();
  drain();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("profile-actions", cx);
    window.render_frame(cx);
    window.click("rename-profile", cx);
  })
  .unwrap();
  settle(cx, window);

  assert!(
    !dialog_open(cx, window),
    "the default profile could be renamed"
  );

  cx.update_window(window, |_, window, cx| {
    window.press("escape", cx);
    window.click("profile-menu", cx);
    window.render_frame(cx);

    assert_eq!(
      menu_row(window, "pick-profile-Default").checked(),
      Some(true)
    );
    assert_ne!(
      menu_row(window, "pick-profile-Modded").checked(),
      Some(true)
    );
    assert!(window.try_find("mod-count-Default").is_some());
    assert!(
      window.try_find("mod-count-Modded").is_none(),
      "a count was shown for a profile never read"
    );

    window.click("pick-profile-Modded", cx);
  })
  .unwrap();

  assert!(
    drain().contains(&Command::OpenProfile {
      game: "valheim".to_string(),
      profile: "Modded".to_string(),
    }),
    "picking a profile did not open it"
  );

  answers.unbounded_send(opened("Modded")).unwrap();
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("profile-menu", cx);
    window.render_frame(cx);
    window.click("new-profile", cx);
  })
  .unwrap();
  settle(cx, window);

  assert!(dialog_open(cx, window), "naming a profile opened no dialog");

  cx.update_window(window, |_, window, cx| {
    window.input("CON", cx);
    window.click("save-profile-name", cx);
    window.render_frame(cx);

    assert!(
      window
        .within("profile-name-dialog")
        .try_find("profile-name-refused")
        .is_some(),
      "the refusal was not shown in the dialog"
    );

    window.click("cancel-profile-name", cx);
  })
  .unwrap();
  settle(cx, window);

  assert!(!dialog_open(cx, window), "cancelling left the dialog open");

  cx.update_window(window, |_, window, cx| {
    window.click("profile-menu", cx);
    window.render_frame(cx);
    window.click("new-profile", cx);
  })
  .unwrap();
  settle(cx, window);
  cx.update_window(window, |_, window, cx| {
    window.input("Fresh", cx);
    window.press("enter", cx);
  })
  .unwrap();

  let created = ProfileChange::Create("Fresh".to_string());

  assert!(
    drain().contains(&Command::ChangeProfile {
      game: "valheim".to_string(),
      change: created.clone(),
    }),
    "the typed name was not submitted"
  );

  for msg in [
    Msg::ProfileChanged {
      game: "valheim".to_string(),
      change: created,
      result: Ok(()),
    },
    opened("Fresh"),
  ] {
    answers.unbounded_send(msg).unwrap();
  }

  cx.run_until_parked();
  drain();

  assert!(
    toasted(cx, window, "notice-success"),
    "creating the profile was not told"
  );

  assert!(
    !dialog_open(cx, window),
    "a saved name left the dialog open"
  );

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("profile-actions", cx);
    window.render_frame(cx);
    window.click("delete-profile", cx);
  })
  .unwrap();
  settle(cx, window);
  cx.update_window(window, |_, window, cx| {
    assert!(
      window
        .within("delete-profile-dialog")
        .try_find("confirm-delete-profile")
        .is_some(),
      "deleting asked nothing"
    );

    window.click("confirm-delete-profile", cx);
  })
  .unwrap();

  assert_eq!(
    drain(),
    vec![Command::ChangeProfile {
      game: "valheim".to_string(),
      change: ProfileChange::Delete("Fresh".to_string()),
    }]
  );

  answers
    .unbounded_send(Msg::ProfileChanged {
      game: "valheim".to_string(),
      change: ProfileChange::Delete("Fresh".to_string()),
      result: Err("busy".to_string()),
    })
    .unwrap();
  cx.run_until_parked();

  assert!(
    toasted(cx, window, "notice-error"),
    "the failed delete was not told"
  );
}

/// The readme fetch the shell sent last, if any.
fn readme_request(
  sent: &mut futures::channel::mpsc::UnboundedReceiver<Command>,
) -> Option<Request> {
  std::iter::from_fn(|| sent.try_recv().ok())
    .filter_map(|command| match command {
      Command::FetchReadme(request) => Some(request),
      _ => None,
    })
    .last()
}

#[gpui_kit::test]
fn selecting_a_package_in_either_tab_shows_its_readme_beside_the_list(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let (commands, mut sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let (window, _) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(State::new(one_game()), commands, inbox, window, cx))
    })
    .expect("open test window")
  });

  for msg in [
    Msg::SettingsLoaded(Ok(valheim_settings())),
    Msg::CachedIndexRead {
      community: "valheim".to_string(),
      index: Ok(Some(mock_index())),
    },
    Msg::ProfileOpened {
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      installed: Ok([package("Owner-ModB", &[])].into()),
    },
  ] {
    answers.unbounded_send(msg).unwrap();
  }

  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("Owner-ModD", cx);
    window.render_frame(cx);

    assert!(window.try_find("readme-loading").is_some());
  })
  .unwrap();

  let browsed = readme_request(&mut sent).expect("a readme fetch was sent");

  assert_eq!(
    browsed.package,
    readme::Package {
      owner: "Owner".to_string(),
      name: "ModD".to_string(),
      version: "2.0.0".to_string(),
    }
  );

  answers
    .unbounded_send(Msg::ReadmeFetched {
      request: browsed.id,
      readme: Ok(Some(
        "# ModD\n\nSee [the wiki](https://example.com).".to_string(),
      )),
    })
    .unwrap();
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    assert!(window.try_find("readme").is_some());

    window.click("tab-installed", cx);
    window.render_frame(cx);

    assert!(window.try_find("readme").is_none());

    window.click("profile-Owner-ModB", cx);
  })
  .unwrap();

  let installed = readme_request(&mut sent).expect("a readme fetch was sent");

  assert_eq!(installed.package.full_name(), "Owner-ModB");

  answers
    .unbounded_send(Msg::ReadmeFetched {
      request: installed.id,
      readme: Ok(None),
    })
    .unwrap();
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    assert!(window.try_find("readme-missing").is_some());
  })
  .unwrap();
}
