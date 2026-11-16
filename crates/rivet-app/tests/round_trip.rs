use futures::channel::mpsc::unbounded;
use gpui_kit::component::WindowExt as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
  AnyWindowHandle, AppContext as _, TestAppContext, VisualTestContext, Window, WindowOptions, px,
};
use rivet_app::Shell;
use rivet_core::browse::{Fetch, Index, Refresh};
use rivet_core::install::{DEFAULT_PROFILE, Installed, InstalledPackage, Outcome, Progress, Stage};
use rivet_core::loader_files::{FileRemoval, LoaderFile};
use rivet_core::readme::{self, Request};
use rivet_core::settings::{GameSettings, RuntimeChoice, Settings};
use rivet_core::{Command, Msg, Screen, State};
use std::path::PathBuf;
use thunderstore_engine::ecosystem::Ecosystem;
use thunderstore_engine::models::{Package, PackageIndex};
use thunderstore_engine::profile::launch::{HostOs, Runtime};
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
    games: [("valheim".to_string(), GameSettings::new("/games/Valheim"))].into(),
    ..Settings::default()
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

  if cfg!(target_os = "linux") {
    assert_eq!(
      sent.try_recv().ok(),
      Some(Command::ListGameFolder {
        game: "valheim".to_string(),
        folder: PathBuf::from("/games/Valheim"),
      })
    );
  }

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

    let row = window.find("Owner-ModA").bounds();

    window.click("refresh", cx);
    window.render_frame(cx);

    assert!(
      window
        .within("status-bar")
        .try_find("status-note")
        .is_some(),
      "refreshing is not in the status bar"
    );
    assert_eq!(
      window.find("Owner-ModA").bounds(),
      row,
      "refreshing moved the package list"
    );
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
    games: [("valheim".to_string(), GameSettings::new("/games/Valheim"))].into(),
    ..Settings::default()
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

/// Presses install on `package` and confirms it in the modal.
fn install(cx: &mut TestAppContext, window: AnyWindowHandle, package: &'static str) {
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click(package, cx);
  })
  .unwrap();
  settle(cx, window);
  cx.update_window(window, |_, window, cx| {
    window.click("confirm-install", cx);
  })
  .unwrap();
  settle(cx, window);
}

#[gpui_kit::test]
fn the_status_bar_shows_the_running_change_without_moving_the_tabs(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let (commands, _sent) = unbounded::<Command>();
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
    Msg::IndexRefreshed {
      refresh: valheim_refresh(1),
      index: Ok(None),
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

  let (tabs, bar) = cx
    .update_window(window, |_, window, cx| {
      window.render_frame(cx);

      assert!(
        window.within("status-bar").elements().is_empty(),
        "the bar is not empty with nothing running"
      );

      (
        window.find("tab-browse").bounds(),
        window.find("status-bar").bounds(),
      )
    })
    .unwrap();
  let unmoved = |window: &mut Window, when: &str| {
    assert_eq!(
      window.find("tab-browse").bounds(),
      tabs,
      "the tabs moved {when}"
    );
    assert_eq!(
      window.find("status-bar").bounds(),
      bar,
      "the bar changed size {when}"
    );
  };

  assert!(bar.size.height > gpui_kit::px(0.), "the bar takes no space");

  install(cx, window, "install-Owner-ModC");
  cx.update_window(window, |_, window, _| {
    unmoved(window, "when a change started");

    let status = window.within("status-bar");

    assert!(status.try_find("operation-1").is_some());
    assert!(status.try_find("cancel-operation-1").is_some());
    assert!(status.try_find("waiting-count").is_none());
  })
  .unwrap();

  install(cx, window, "install-Owner-ModA");
  cx.update_window(window, |_, window, cx| {
    unmoved(window, "when a change was queued");

    assert!(
      window
        .within("status-bar")
        .try_find("waiting-count")
        .is_some(),
      "the bar does not say a change is waiting"
    );

    window.click("cancel-operation-1", cx);
    window.render_frame(cx);

    unmoved(window, "when a change was cancelled");

    let status = window.within("status-bar");

    assert!(status.try_find("operation-stopping").is_some());
    assert!(status.try_find("cancel-operation-1").is_none());
  })
  .unwrap();

  answers
    .unbounded_send(Msg::OperationFinished {
      operation: 1,
      outcome: Ok(Outcome {
        changed: vec![],
        failed: vec![rivet_core::install::FailedPackage {
          package: "Owner-ModC".to_string(),
          reason: "corrupt archive".to_string(),
        }],
        cancelled: vec![],
        profile: Ok(Installed::new()),
      }),
    })
    .unwrap();
  cx.run_until_parked();

  assert!(
    toasted(cx, window, "notice-error"),
    "the failed change was not told"
  );

  cx.update_window(window, |_, window, _| {
    unmoved(window, "when the next change started");

    let status = window.within("status-bar");

    assert!(status.try_find("operation-1").is_none());
    assert!(status.try_find("operation-2").is_some());
    assert!(status.try_find("waiting-count").is_none());
  })
  .unwrap();

  answers
    .unbounded_send(Msg::OperationFinished {
      operation: 2,
      outcome: Ok(Outcome {
        changed: vec!["Owner-ModA".to_string()],
        failed: vec![],
        cancelled: vec![],
        profile: Ok([package("Owner-ModA", &[])].into()),
      }),
    })
    .unwrap();
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    unmoved(window, "when the bar emptied");

    assert!(
      window.within("status-bar").elements().is_empty(),
      "a finished change stayed in the bar"
    );
  })
  .unwrap();
}

#[gpui_kit::test]
fn cancel_and_error_toasts_stay_clear_of_the_status_bar(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let (commands, _sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let (window, _) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(State::new(one_game()), commands, inbox, window, cx))
    })
    .expect("open test window")
  });
  let profile =
    "A profile whose name runs on long enough to fill the status bar by itself ".repeat(4);
  let mut settings = valheim_settings();

  settings.games.get_mut("valheim").unwrap().profile = Some(profile.clone());

  for msg in [
    Msg::SettingsLoaded(Ok(settings)),
    Msg::CachedIndexRead {
      community: "valheim".to_string(),
      index: Ok(Some(mock_index())),
    },
    Msg::ProfileOpened {
      game: "valheim".to_string(),
      profile,
      installed: Ok(Installed::new()),
    },
  ] {
    answers.unbounded_send(msg).unwrap();
  }

  cx.run_until_parked();
  install(cx, window, "install-Owner-ModC");
  cx.update_window(window, |_, window, _| {
    let bar = window.find("status-bar").bounds();
    let cancel = window.find("cancel-operation-1").bounds();

    assert!(
      cancel.right() <= bar.right(),
      "a long profile name pushed Cancel off the bar"
    );
  })
  .unwrap();

  answers
    .unbounded_send(Msg::OperationFinished {
      operation: 1,
      outcome: Ok(Outcome {
        changed: vec![],
        failed: vec![rivet_core::install::FailedPackage {
          package: "Owner-ModC".to_string(),
          reason: "corrupt archive".to_string(),
        }],
        cancelled: vec![],
        profile: Ok(Installed::new()),
      }),
    })
    .unwrap();
  cx.run_until_parked();

  assert!(toasted(cx, window, "notice-error"));

  cx.executor()
    .advance_clock(std::time::Duration::from_secs(1));
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    let bar = window.find("status-bar").bounds();
    let toast = window.find("notice-error").bounds();

    assert!(
      toast.bottom() <= bar.top(),
      "the error toast covers the status bar: {toast:?} {bar:?}"
    );
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
      pinned: false,
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
  })
  .unwrap();
  settle(cx, window);
  cx.update_window(window, |_, window, cx| {
    window
      .within("dependants-question")
      .click("with-dependants", cx);
  })
  .unwrap();
  settle(cx, window);

  assert!(
    !dialog_open(cx, window),
    "the modal stayed after the choice"
  );

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

/// The mock's package index with `Owner-ModB` deprecated.
fn index_with_mod_b_deprecated() -> Index {
  let mut packages: Vec<serde_json::Value> =
    serde_json::from_str(&index_json("http://mock")).unwrap();

  for package in &mut packages {
    if package["full_name"] == "Owner-ModB" {
      package["is_deprecated"] = true.into();
    }
  }

  let packages: Vec<Package> = serde_json::from_value(serde_json::Value::Array(packages)).unwrap();

  PackageIndex::from(packages).into()
}

/// The packages of the next update sent.
fn updated(sent: &mut futures::channel::mpsc::UnboundedReceiver<Command>) -> (u64, Vec<String>) {
  std::iter::from_fn(|| sent.try_recv().ok())
    .find_map(|command| match command {
      Command::Update(update) => Some((update.operation, update.packages)),
      _ => None,
    })
    .expect("an update was sent")
}

#[gpui_kit::test]
fn the_installed_tab_badges_packages_and_updates_one_or_all(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let (commands, mut sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let (window, _) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(State::new(one_game()), commands, inbox, window, cx))
    })
    .expect("open test window")
  });
  let profile: Installed = [
    package("Owner-ModB", &[]),
    package("Owner-ModC", &["Owner-ModA"]),
    package("Owner-ModD", &[]),
  ]
  .into();

  for msg in [
    Msg::SettingsLoaded(Ok(valheim_settings())),
    Msg::CachedIndexRead {
      community: "valheim".to_string(),
      index: Ok(Some(index_with_mod_b_deprecated())),
    },
    Msg::ProfileOpened {
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      installed: Ok(profile.clone()),
    },
  ] {
    answers.unbounded_send(msg).unwrap();
  }

  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("tab-installed", cx);
    window.render_frame(cx);

    assert!(window.try_find("update-badge-Owner-ModD").is_some());
    assert!(window.try_find("update-badge-Owner-ModB").is_none());
    assert!(window.try_find("deprecated-badge-Owner-ModB").is_some());
    assert!(window.try_find("missing-badge-Owner-ModC").is_some());
    assert!(window.try_find("update-Owner-ModC").is_none());

    window.click("update-all", cx);
  })
  .unwrap();

  assert_eq!(updated(&mut sent), (1, vec!["Owner-ModD".to_string()]));

  answers
    .unbounded_send(Msg::OperationFinished {
      operation: 1,
      outcome: Ok(Outcome {
        changed: vec![],
        failed: vec![],
        cancelled: vec![],
        profile: Ok(profile),
      }),
    })
    .unwrap();
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("update-Owner-ModD", cx);
  })
  .unwrap();

  assert_eq!(updated(&mut sent), (2, vec!["Owner-ModD".to_string()]));
}

#[gpui_kit::test]
fn a_version_chosen_in_the_install_modal_is_the_one_installed(cx: &mut TestAppContext) {
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
    window.click("install-Owner-ModD", cx);
  })
  .unwrap();
  settle(cx, window);
  cx.update_window(window, |_, window, cx| {
    window.within("install-dialog").click("install-version", cx);
    window.render_frame(cx);
    window.click("version-1.0.0", cx);
  })
  .unwrap();
  settle(cx, window);
  cx.update_window(window, |_, window, cx| {
    window.click("confirm-install", cx);
  })
  .unwrap();

  let version = std::iter::from_fn(|| sent.try_recv().ok())
    .find_map(|command| match command {
      Command::Install(install) => Some(install.version),
      _ => None,
    })
    .expect("an install was sent");

  assert_eq!(version.as_deref(), Some("1.0.0"));
}

#[gpui_kit::test]
fn a_pinned_package_is_marked_until_it_is_unpinned(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let (commands, mut sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let (window, _) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(State::new(one_game()), commands, inbox, window, cx))
    })
    .expect("open test window")
  });
  let (name, mut pinned) = package("Owner-ModD", &[]);

  pinned.pinned = true;

  for msg in [
    Msg::SettingsLoaded(Ok(valheim_settings())),
    Msg::CachedIndexRead {
      community: "valheim".to_string(),
      index: Ok(Some(mock_index())),
    },
    Msg::ProfileOpened {
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      installed: Ok([(name, pinned)].into()),
    },
  ] {
    answers.unbounded_send(msg).unwrap();
  }

  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("tab-installed", cx);
    window.render_frame(cx);

    assert!(window.try_find("pinned-badge-Owner-ModD").is_some());
    assert!(window.try_find("update-badge-Owner-ModD").is_some());
    assert!(window.try_find("update-Owner-ModD").is_none());

    window.click("unpin-Owner-ModD", cx);
  })
  .unwrap();

  let unpin = std::iter::from_fn(|| sent.try_recv().ok())
    .find_map(|command| match command {
      Command::Unpin(unpin) => Some(unpin),
      _ => None,
    })
    .expect("an unpin was sent");

  assert_eq!(unpin.package, "Owner-ModD");

  answers
    .unbounded_send(Msg::Unpinned {
      unpin,
      result: Ok(()),
    })
    .unwrap();
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    assert!(window.try_find("pinned-badge-Owner-ModD").is_none());
    assert!(window.try_find("update-badge-Owner-ModD").is_some());

    window.click("version-Owner-ModD", cx);
  })
  .unwrap();
  settle(cx, window);

  assert!(dialog_open(cx, window), "choosing a version asked nothing");
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
      steam: None,
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

    let row = window.find("valheim").bounds();

    window.click("valheim", cx);
    window.render_frame(cx);

    assert!(
      window
        .within("status-bar")
        .try_find("status-note")
        .is_some(),
      "looking for the game is not in the status bar"
    );
    assert_eq!(
      window.find("valheim").bounds(),
      row,
      "looking for the game moved the list"
    );
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

  for msg in [
    Msg::SettingsLoaded(Ok(valheim_settings())),
    Msg::GameFolderListed {
      game: "valheim".to_string(),
      entries: Ok(vec!["valheim.x86_64".to_string()]),
    },
  ] {
    answers.unbounded_send(msg).unwrap();
  }

  cx.run_until_parked();

  let read = std::iter::from_fn(|| sent.try_recv().ok()).find(|command| {
    matches!(command, Command::ReadSteamOption { game, app_id: 892970, .. } if game == "valheim")
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
      runtime: Runtime::Native,
      line: Some(line.to_string()),
      current: Ok(None),
    }),
  ] {
    answers.unbounded_send(msg).unwrap();
  }

  cx.run_until_parked();

  let tabs = cx
    .update_window(window, |_, window, cx| {
      window.render_frame(cx);

      assert!(window.try_find("launch-option-not-set").is_some());
      assert!(
        window.try_find("missing-loader").is_none(),
        "an empty profile warned that it has no mod loader"
      );

      let tabs = window.find("tab-browse").bounds();

      window.click("copy-launch-option", cx);
      window.within("sidebar").click("launch-vanilla", cx);
      window.render_frame(cx);

      tabs
    })
    .unwrap();

  cx.update_window(window, |_, window, _| {
    assert!(
      window
        .within("status-bar")
        .try_find("status-note")
        .is_some(),
      "starting the game is not in the status bar"
    );
    assert_eq!(
      window.find("tab-browse").bounds(),
      tabs,
      "starting the game moved the tabs"
    );
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
    .unbounded_send(Msg::LaunchFinished {
      launch: launch.id,
      result: Err(rivet_core::launch::LaunchFailure::Other(
        "the game exited at once".to_string(),
      )),
    })
    .unwrap();
  cx.run_until_parked();

  assert!(
    toasted(cx, window, "notice-error"),
    "the failed launch was not told"
  );

  cx.update_window(window, |_, window, _| {
    assert_eq!(
      window.find("tab-browse").bounds(),
      tabs,
      "the failed launch moved the tabs"
    );
    assert!(
      window
        .within("launch-option")
        .try_find("launch-option-not-set")
        .is_some(),
      "rechecking Steam changed what the banner says"
    );
  })
  .unwrap();

  answers
    .unbounded_send(Msg::SteamOptionRead(SteamOptionRead {
      game: "valheim".to_string(),
      runtime: Runtime::Native,
      line: Some(line.to_string()),
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
fn the_sidebar_picks_names_and_deletes_profiles(cx: &mut TestAppContext) {
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

  let place = |window: &mut Window| {
    [
      "pick-profile-Default",
      "pick-profile-Modded",
      "new-profile",
      "tab-browse",
    ]
    .map(|id| window.find(id).bounds())
  };
  let before = cx
    .update_window(window, |_, window, cx| {
      window.press("escape", cx);
      window.render_frame(cx);

      let sidebar = window.within("sidebar");

      assert_eq!(sidebar.find("pick-profile-Default").checked(), Some(true));
      assert_ne!(sidebar.find("pick-profile-Modded").checked(), Some(true));
      assert!(sidebar.try_find("mod-count-Default").is_some());
      assert!(
        sidebar.try_find("mod-count-Modded").is_none(),
        "a count was shown for a profile never read"
      );

      let before = place(window);

      window.click("pick-profile-Modded", cx);
      window.render_frame(cx);

      assert_eq!(place(window), before, "opening the profile moved the list");

      before
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

    assert_eq!(
      window.find("pick-profile-Modded").checked(),
      Some(true),
      "the opened profile is not marked current"
    );
    assert_ne!(window.find("pick-profile-Default").checked(), Some(true));
    assert!(
      window.try_find("mod-count-Default").is_some(),
      "the count of the profile left was dropped"
    );
    assert_eq!(place(window), before, "switching profile moved the list");

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

  cx.update_window(window, |_, window, cx| window.click("new-profile", cx))
    .unwrap();
  settle(cx, window);

  let tabs = cx
    .update_window(window, |_, window, cx| {
      let tabs = window.find("tab-browse").bounds();

      window.input("Fresh", cx);
      window.press("enter", cx);

      tabs
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

  settle(cx, window);
  cx.update_window(window, |_, window, _| {
    assert!(
      window
        .within("status-bar")
        .try_find("status-note")
        .is_some(),
      "creating the profile is not in the status bar"
    );
    assert_eq!(
      window.find("tab-browse").bounds(),
      tabs,
      "creating the profile moved the tabs"
    );
  })
  .unwrap();

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

#[gpui_kit::test]
fn the_sidebar_imports_a_profile_from_a_code_or_a_file(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let (commands, _sent) = unbounded::<Command>();
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
    window.click("import-profile", cx);
    window.render_frame(cx);

    assert!(window.try_find("import-file").is_some());
    assert!(
      window.try_find("import-r2modman").is_none(),
      "r2modman was offered with no r2modman profiles"
    );

    window.click("import-code", cx);
  })
  .unwrap();
  settle(cx, window);

  assert!(dialog_open(cx, window), "importing a code opened no dialog");

  cx.update_window(window, |_, window, cx| {
    window.press("escape", cx);
    window.render_frame(cx);
  })
  .unwrap();
  settle(cx, window);
  cx.update_window(window, |_, window, cx| {
    window.click("import-profile", cx);
    window.render_frame(cx);
    window.click("import-file", cx);
  })
  .unwrap();

  assert!(cx.did_prompt_for_paths(), "importing a file asked for none");
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

#[gpui_kit::test]
fn the_installed_tab_asks_about_an_uninstall_in_the_modal_without_moving_the_list(
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

  let list = cx
    .update_window(window, |_, window, cx| {
      window.render_frame(cx);
      window.click("tab-installed", cx);
      window.render_frame(cx);

      window.find("installed-packages").bounds()
    })
    .unwrap();

  cx.update_window(window, |_, window, cx| {
    window.click("uninstall-Owner-ModA", cx);
  })
  .unwrap();
  settle(cx, window);

  assert!(
    dialog_open(cx, window),
    "the dependants question is not in a modal"
  );

  cx.update_window(window, |_, window, cx| {
    assert_eq!(
      window.find("installed-packages").bounds(),
      list,
      "the dependants question moved the list"
    );
    assert!(
      window
        .within("dependants-question")
        .try_find("package-only")
        .is_some()
    );

    window.click("with-dependants", cx);
  })
  .unwrap();
  settle(cx, window);

  let packages = vec!["Owner-ModA".to_string(), "Owner-ModC".to_string()];
  let checked = std::iter::from_fn(|| sent.try_recv().ok()).any(|command| {
    command
      == Command::CheckUninstall {
        game: "valheim".to_string(),
        profile: DEFAULT_PROFILE.to_string(),
        packages: packages.clone(),
      }
  });

  assert!(checked, "the uninstall was not checked");
  assert!(dialog_open(cx, window), "the modal closed while checking");

  cx.update_window(window, |_, window, _| {
    assert!(window.try_find("checking-uninstall").is_some());
    assert_eq!(
      window.find("installed-packages").bounds(),
      list,
      "checking the uninstall moved the list"
    );
  })
  .unwrap();

  answers
    .unbounded_send(Msg::UninstallChecked {
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      packages: packages.clone(),
      untracked: Ok(vec![PathBuf::from("/games/Valheim/BepInEx/plugins/Stray")]),
    })
    .unwrap();
  cx.run_until_parked();
  settle(cx, window);

  assert!(
    dialog_open(cx, window),
    "the folders question is not in a modal"
  );

  cx.update_window(window, |_, window, cx| {
    assert_eq!(
      window.find("installed-packages").bounds(),
      list,
      "the folders question moved the list"
    );

    window
      .within("untracked-question")
      .click("keep-folders", cx);
  })
  .unwrap();
  settle(cx, window);

  assert!(
    !dialog_open(cx, window),
    "the modal stayed after the choice"
  );

  let uninstall = std::iter::from_fn(|| sent.try_recv().ok())
    .find_map(|command| match command {
      Command::Uninstall(uninstall) => Some(uninstall),
      _ => None,
    })
    .expect("an uninstall was sent");

  assert_eq!(uninstall.packages, packages);
  assert!(uninstall.delete.is_empty());
}

#[gpui_kit::test]
fn a_refused_uninstall_shows_in_the_modal_and_dismissing_it_clears_it(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let (commands, _sent) = unbounded::<Command>();
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
      installed: Ok([package("Owner-ModA", &[])].into()),
    },
  ] {
    answers.unbounded_send(msg).unwrap();
  }

  cx.run_until_parked();

  let list = cx
    .update_window(window, |_, window, cx| {
      window.render_frame(cx);
      window.click("tab-installed", cx);
      window.render_frame(cx);

      window.find("installed-packages").bounds()
    })
    .unwrap();

  cx.update_window(window, |_, window, cx| {
    window.click("uninstall-Owner-ModA", cx);
  })
  .unwrap();
  settle(cx, window);

  assert!(
    !dialog_open(cx, window),
    "checking an uninstall nothing depends on opened a modal"
  );

  cx.update_window(window, |_, window, _| {
    assert_eq!(
      window.find("installed-packages").bounds(),
      list,
      "checking the uninstall moved the list"
    );
  })
  .unwrap();
  answers
    .unbounded_send(Msg::UninstallChecked {
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      packages: vec!["Owner-ModA".to_string()],
      untracked: Err("the profile folder is read-only".to_string()),
    })
    .unwrap();
  cx.run_until_parked();
  settle(cx, window);

  assert!(dialog_open(cx, window), "the refusal is not in a modal");

  cx.update_window(window, |_, window, cx| {
    window
      .within("uninstall-refused")
      .click("dismiss-pending", cx);
  })
  .unwrap();
  settle(cx, window);

  assert!(!dialog_open(cx, window), "dismissing kept the refusal open");
}

/// The settings `saves` last saved for valheim, failing when there are none.
fn last_saved(saves: &[Command]) -> GameSettings {
  saves
    .iter()
    .rev()
    .find_map(|command| match command {
      Command::SaveSettings(settings) => settings.games.get("valheim").cloned(),
      _ => None,
    })
    .expect("the settings were not saved")
}

#[gpui_kit::test]
fn settings_change_the_game_folder_and_save_typed_launch_arguments(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let (commands, mut sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let (window, shell) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(State::new(one_game()), commands, inbox, window, cx))
    })
    .expect("open test window")
  });
  let mut drain = move || std::iter::from_fn(|| sent.try_recv().ok()).collect::<Vec<_>>();
  let mut settings = valheim_settings();
  let moved = PathBuf::from("/moved/Valheim");

  if let Some(valheim) = settings.games.get_mut("valheim") {
    valheim.launch_arguments = "-console".to_string();
  }

  answers
    .unbounded_send(Msg::SettingsLoaded(Ok(settings)))
    .unwrap();
  cx.run_until_parked();
  drain();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("open-settings", cx);
    window.render_frame(cx);
    window.click("change-game-folder", cx);
  })
  .unwrap();

  assert_eq!(
    shell.read_with(cx, |shell, _| shell.state().screen.clone()),
    Screen::Settings("valheim".to_string())
  );
  assert!(cx.did_prompt_for_paths());

  cx.simulate_path_prompt_response(|_| Some(vec![moved.clone()]));
  cx.run_until_parked();

  assert_eq!(drain(), vec![Command::ListFolder(moved.clone())]);

  answers
    .unbounded_send(Msg::FolderListed {
      folder: moved.clone(),
      entries: Ok(vec!["valheim.x86_64".to_string()]),
    })
    .unwrap();
  cx.run_until_parked();

  assert_eq!(last_saved(&drain()).folder, moved);

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("launch-arguments", cx);
    window.input(" -windowed", cx);
  })
  .unwrap();
  settle(cx, window);
  cx.update_window(window, |_, window, cx| window.click("close-settings", cx))
    .unwrap();
  cx.run_until_parked();

  assert_eq!(
    shell.read_with(cx, |shell, _| shell.state().screen.clone()),
    Screen::Game("valheim".to_string())
  );
  assert_eq!(last_saved(&drain()).launch_arguments, "-console -windowed");
}

/// A window on the game screen for Valheim, with `host` as the system the app
/// runs on.
fn game_screen_on(cx: &mut TestAppContext, host: HostOs) -> AnyWindowHandle {
  let (commands, _sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let mut state = State::new(one_game());

  state.host = Some(host);

  let (window, _) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(state, commands, inbox, window, cx))
    })
    .expect("open test window")
  });

  answers
    .unbounded_send(Msg::SettingsLoaded(Ok(valheim_settings())))
    .unwrap();
  cx.run_until_parked();

  window
}

#[gpui_kit::test]
fn the_game_screen_keeps_the_game_and_profiles_in_a_sidebar_beside_the_tabs(
  cx: &mut TestAppContext,
) {
  cx.update(gpui_kit::init);

  let window = game_screen_on(cx, HostOs::Linux);

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    let sidebar = window.find("sidebar").bounds();
    let tabs = window.find("tab-browse").bounds();

    assert_eq!(
      sidebar.size.width,
      px(248.),
      "the sidebar is not 248px wide"
    );
    assert!(
      window.try_find("header").is_none(),
      "the header row is back"
    );
    assert!(
      tabs.left() >= sidebar.right(),
      "the tabs are not beside the sidebar: {sidebar:?} then {tabs:?}"
    );
    assert!(
      tabs.top() - sidebar.top() <= px(16.),
      "something sits above the tabs: {sidebar:?} then {tabs:?}"
    );

    let within = window.within("sidebar");

    for id in [
      "game-name",
      "switch-game",
      "launch-modded",
      "launch-vanilla",
      "pick-profile-Default",
      "new-profile",
      "import-profile",
      "profile-actions",
      "open-settings",
    ] {
      assert!(within.try_find(id).is_some(), "{id} is not in the sidebar");
    }

    let row = within.find("open-settings").bounds().size.width;

    for id in ["new-profile", "import-profile", "profile-actions"] {
      assert_eq!(
        within.find(id).bounds().size.width,
        row,
        "{id} does not fill its row"
      );
    }
  })
  .unwrap();
}

#[gpui_kit::test]
fn the_sidebar_says_how_the_game_runs_on_linux_only(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let linux = game_screen_on(cx, HostOs::Linux);
  let windows = game_screen_on(cx, HostOs::Windows);

  let run_with = |window: &mut Window, cx: &mut gpui_kit::App, runtime: &'static str| {
    window.render_frame(cx);
    window.click("open-settings", cx);
    window.render_frame(cx);
    window.click(runtime, cx);
    window.click("close-settings", cx);
    window.render_frame(cx);
  };

  cx.update_window(linux, |_, window, cx| {
    run_with(window, cx, "runtime-native");

    assert!(
      window.within("sidebar").try_find("runs-native").is_some(),
      "the sidebar does not say the game runs natively"
    );
    assert!(window.try_find("runs-proton").is_none());

    run_with(window, cx, "runtime-proton");

    assert!(
      window.within("sidebar").try_find("runs-proton").is_some(),
      "the sidebar does not say the game runs under Proton"
    );
    assert!(window.try_find("runs-native").is_none());
  })
  .unwrap();
  cx.update_window(windows, |_, window, cx| {
    window.render_frame(cx);

    assert!(
      window.try_find("runs-native").is_none() && window.try_find("runs-proton").is_none(),
      "the sidebar says how the game runs on Windows"
    );
  })
  .unwrap();
}

#[gpui_kit::test]
fn the_windows_build_says_it_is_experimental_without_moving_the_tabs(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let linux = game_screen_on(cx, HostOs::Linux);
  let windows = game_screen_on(cx, HostOs::Windows);
  let (linux_tabs, labelled) = cx
    .update_window(linux, |_, window, cx| {
      window.render_frame(cx);

      (
        window.find("tab-browse").bounds(),
        window.try_find("experimental").is_some(),
      )
    })
    .unwrap();

  assert!(!labelled, "the Linux build is labelled experimental");

  cx.update_window(windows, |_, window, cx| {
    window.render_frame(cx);

    assert!(
      window.try_find("experimental").is_some(),
      "the Windows build is not labelled experimental"
    );
    assert!(
      window.within("status-bar").elements().is_empty(),
      "the label sits in the status bar's space for changes"
    );
    assert_eq!(
      window.find("tab-browse").bounds(),
      linux_tabs,
      "the label moved the tabs"
    );
  })
  .unwrap();
}

#[gpui_kit::test]
fn settings_force_a_linux_game_to_run_under_proton(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let (commands, mut sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let mut state = State::new(one_game());

  state.host = Some(HostOs::Linux);

  let (window, shell) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(state, commands, inbox, window, cx))
    })
    .expect("open test window")
  });
  let mut drain = move || std::iter::from_fn(|| sent.try_recv().ok()).collect::<Vec<_>>();

  answers
    .unbounded_send(Msg::SettingsLoaded(Ok(valheim_settings())))
    .unwrap();
  cx.run_until_parked();
  drain();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("open-settings", cx);
    window.render_frame(cx);
    window.click("runtime-proton", cx);
  })
  .unwrap();
  cx.run_until_parked();

  assert_eq!(last_saved(&drain()).runtime, RuntimeChoice::Proton);
  assert_eq!(
    shell.read_with(cx, |shell, _| {
      shell
        .state()
        .launcher
        .as_ref()
        .and_then(|launcher| launcher.runtime)
    }),
    Some(Runtime::Proton)
  );
}

#[gpui_kit::test]
fn settings_mark_the_chosen_runtime_as_pressed(cx: &mut TestAppContext) {
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

  answers
    .unbounded_send(Msg::SettingsLoaded(Ok(valheim_settings())))
    .unwrap();
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("open-settings", cx);
    window.render_frame(cx);
    window.click("runtime-proton", cx);
  })
  .unwrap();
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    let pressed =
      ["runtime-detect", "runtime-native", "runtime-proton"].map(|id| window.find(id).checked());

    assert_eq!(pressed, [Some(false), Some(false), Some(true)]);
  })
  .unwrap();
}

#[gpui_kit::test]
fn settings_remove_the_loader_files_launches_copied_into_the_game_folder(cx: &mut TestAppContext) {
  cx.update(gpui_kit::init);

  let (commands, mut sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let (window, _) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(State::new(one_game()), commands, inbox, window, cx))
    })
    .expect("open test window")
  });
  let mut drain = move || std::iter::from_fn(|| sent.try_recv().ok()).collect::<Vec<_>>();
  let proxy = LoaderFile {
    path: PathBuf::from("/games/Valheim/winhttp.dll"),
    sha256: "aa".to_string(),
  };
  let mut settings = valheim_settings();

  if let Some(valheim) = settings.games.get_mut("valheim") {
    valheim.loader_files = vec![proxy.clone()];
  }

  answers
    .unbounded_send(Msg::SettingsLoaded(Ok(settings)))
    .unwrap();
  cx.run_until_parked();
  drain();

  let place = |window: &mut Window| window.find("steam-folder").bounds();
  let before = cx
    .update_window(window, |_, window, cx| {
      window.render_frame(cx);
      window.click("open-settings", cx);
      window.render_frame(cx);

      let before = place(window);

      window.click("remove-loader-files", cx);

      before
    })
    .unwrap();
  cx.run_until_parked();

  assert_eq!(
    drain(),
    vec![Command::RemoveLoaderFiles {
      game: "valheim".to_string(),
      files: vec![proxy.clone()],
    }]
  );

  answers
    .unbounded_send(Msg::LoaderFilesRemoved {
      game: "valheim".to_string(),
      files: vec![(proxy.path.clone(), FileRemoval::Removed)],
    })
    .unwrap();
  cx.run_until_parked();

  assert_eq!(last_saved(&drain()).loader_files, []);
  assert!(toasted(cx, window, "notice-success"));
  assert_eq!(
    cx.update_window(window, |_, window, cx| {
      window.render_frame(cx);

      place(window)
    })
    .unwrap(),
    before
  );
}
