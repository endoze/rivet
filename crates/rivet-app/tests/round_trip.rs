use futures::channel::mpsc::unbounded;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, TestAppContext, WindowOptions};
use rivet_app::Shell;
use rivet_core::browse::{Fetch, Index, Refresh};
use rivet_core::install::{DEFAULT_PROFILE, Installed, InstalledPackage, Outcome};
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
    window.click("package-search", cx);
    window.input("modc", cx);
  })
  .unwrap();
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    assert!(window.try_find("Owner-ModC").is_some());
    assert!(window.try_find("Owner-ModA").is_none());

    window.click("sort-name", cx);
    window.click("show-deprecated", cx);
    window.render_frame(cx);
    window.click("clear-filters", cx);
    window.render_frame(cx);

    assert!(window.try_find("Owner-ModA").is_some());
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
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("install-Owner-ModC", cx);
    window.render_frame(cx);
    window.click("confirm-install", cx);
  })
  .unwrap();

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
  })
  .unwrap();
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
fn the_installed_tab_lists_the_profile_and_asks_before_disabling_a_dependency(
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

  assert_eq!(set.packages, ["Owner-ModA", "Owner-ModC"]);
  assert!(!set.enabled);
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

  let launch = std::iter::from_fn(|| sent.try_recv().ok())
    .find_map(|command| match command {
      Command::Launch(launch) => Some(launch),
      _ => None,
    })
    .expect("a launch was sent");

  assert_eq!(launch.context.mode, LaunchMode::Vanilla);
  assert_eq!(launch.folder, PathBuf::from("/games/Valheim"));
}

#[gpui_kit::test]
fn the_profile_switcher_picks_names_and_deletes_profiles(cx: &mut TestAppContext) {
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
    opened(DEFAULT_PROFILE),
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
    window.click("profile-Modded", cx);
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
    window.click("new-profile", cx);
    window.render_frame(cx);
    window.input("CON", cx);
    window.click("save-profile-name", cx);
    window.render_frame(cx);

    assert!(window.try_find("profile-name-refused").is_some());

    window.click("cancel-profile-name", cx);
    window.click("new-profile", cx);
    window.render_frame(cx);
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
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    assert!(window.try_find("profile-Fresh").is_some());

    window.click("delete-profile", cx);
    window.render_frame(cx);
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
}
