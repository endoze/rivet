use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
  AnyWindowHandle, AppContext as _, TestAppContext, Window, WindowOptions, point, px,
};
use rivet_app::Shell;
use rivet_core::browse::Index;
use rivet_core::install::{DEFAULT_PROFILE, InstalledPackage};
use rivet_core::settings::{GameSettings, Settings};
use rivet_core::{Command, Msg, State};
use thunderstore_engine::ecosystem::Ecosystem;
use thunderstore_engine::models::{Package, PackageIndex};

/// The spec's narrowest detail pane.
const PANE_MIN_WIDTH: f32 = 320.;

#[gpui_kit::test]
fn the_pane_shows_the_first_mod_until_another_is_selected_without_moving_the_list(
  cx: &mut TestAppContext,
) {
  let (window, mut sent) = open_valheim(cx, vec![]);

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    let list = window.find("mod-list").bounds();
    let row = window.find("Owner-ModA").bounds();
    let mut fact = |id: &'static str| {
      window
        .within("details")
        .find(id)
        .label()
        .map(str::to_string)
    };

    assert_eq!(fact("details-name").as_deref(), Some("ModA"));
    assert_eq!(fact("fact-version").as_deref(), Some("2.0.0"));
    assert_eq!(fact("fact-downloads").as_deref(), Some("1k"));
    assert_eq!(fact("fact-categories").as_deref(), Some("Tools, Maps"));
    assert!(fact("fact-updated").is_some_and(|updated| updated.ends_with(" ago")));
    assert!(
      window
        .within("details")
        .try_find("details-install")
        .is_some()
    );
    assert!(
      window
        .within("details")
        .try_find("details-version")
        .is_none(),
      "a mod outside the profile offers Choose version beside Install"
    );

    window.click("Owner-ModB", cx);
    window.render_frame(cx);

    assert_eq!(
      window.within("details").find("details-name").label(),
      Some("ModB")
    );
    assert_eq!(window.find("mod-list").bounds(), list, "the list moved");
    assert_eq!(window.find("Owner-ModA").bounds(), row, "the rows moved");
  })
  .unwrap();

  assert_eq!(
    readme_fetches(&mut sent),
    ["Owner-ModA", "Owner-ModB"],
    "readmes other than those shown were fetched"
  );
}

#[gpui_kit::test]
fn the_pane_closes_and_reopens_at_the_width_it_was_given_without_moving_the_toolbar(
  cx: &mut TestAppContext,
) {
  let (window, _) = open_valheim(cx, vec![]);

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    let pane = window.find("details").bounds();
    let list = window.find("mod-list").bounds().size.width;

    drag_pane_edge(window, px(-100.), cx);

    let widened = window.find("details").bounds().size.width;

    assert!(
      widened > pane.size.width + px(90.),
      "dragging 100px widened the pane from {:?} to {widened:?}",
      pane.size.width
    );

    let tools = toolbar(window);

    window.click("close-details", cx);
    window.render_frame(cx);

    assert!(window.try_find("details").is_none(), "the pane stayed open");
    assert!(window.find("mod-list").bounds().size.width > list);
    assert_eq!(toolbar(window), tools, "closing moved the toolbar");

    window.click("open-details", cx);
    window.render_frame(cx);

    assert_eq!(window.find("details").bounds().size.width, widened);
    assert!(window.try_find("open-details").is_none());
    assert_eq!(toolbar(window), tools, "reopening moved the toolbar");
  })
  .unwrap();
}

#[gpui_kit::test]
fn the_pane_is_never_narrower_than_its_minimum(cx: &mut TestAppContext) {
  let (window, _) = open_valheim(cx, vec![]);

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    drag_pane_edge(window, px(600.), cx);

    assert_eq!(
      window.find("details").bounds().size.width,
      px(PANE_MIN_WIDTH)
    );
  })
  .unwrap();
}

#[gpui_kit::test]
fn a_profile_mod_offers_its_update_version_and_uninstall_in_the_pane(cx: &mut TestAppContext) {
  let (window, mut sent) = open_valheim(cx, vec![installed("Owner-ModA")]);

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("tab-installed", cx);
    window.render_frame(cx);

    assert!(
      window.try_find("mod-menu-Owner-ModA").is_none(),
      "a row still has a menu"
    );

    let details = |window: &mut Window, id: &'static str| window.within("details").try_find(id);

    assert_eq!(
      details(window, "details-update").and_then(|button| button.label().map(str::to_string)),
      Some("Update to 2.0.0".to_string())
    );
    assert!(details(window, "details-version").is_some());
    assert!(details(window, "details-install").is_none());
    assert_eq!(
      window.within("details").find("fact-version").label(),
      Some("1.0.0")
    );

    window.within("details").click("details-uninstall", cx);
  })
  .unwrap();

  let checked = std::iter::from_fn(|| sent.try_recv().ok())
    .any(|command| matches!(command, Command::CheckUninstall { packages, .. } if packages == ["Owner-ModA"]));

  assert!(checked, "Uninstall in the pane did not uninstall");
}

#[gpui_kit::test]
fn a_search_shows_its_first_mod_at_once_and_fetches_its_readme_once_typing_pauses(
  cx: &mut TestAppContext,
) {
  let (window, mut sent) = open_valheim(cx, vec![]);

  readme_fetches(&mut sent);
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("package-search", cx);
    window.input("modb", cx);
  })
  .unwrap();
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    assert_eq!(
      window.within("details").find("details-name").label(),
      Some("ModB")
    );
  })
  .unwrap();

  assert_eq!(readme_fetches(&mut sent), Vec::<String>::new());

  cx.executor()
    .advance_clock(rivet_core::readme::SEARCH_PAUSE);
  cx.run_until_parked();

  assert_eq!(readme_fetches(&mut sent), ["Owner-ModB"]);
}

/// Drags the edge between the list and the detail pane `by` to the right.
fn drag_pane_edge(window: &mut Window, by: gpui_kit::Pixels, cx: &mut gpui_kit::App) {
  let pane = window.find("details").bounds();
  let from = point(pane.left() - px(2.), pane.center().y);

  window.drag(from, point(from.x + by, from.y), cx);
  window.render_frame(cx);
}

/// Where the toolbar's search box and its last button sit.
fn toolbar(window: &mut Window) -> [gpui_kit::Bounds<gpui_kit::Pixels>; 2] {
  ["package-search", "refresh"].map(|id| window.find(id).bounds())
}

/// The packages whose readmes the shell asked for, by `Owner-Name`.
fn readme_fetches(sent: &mut UnboundedReceiver<Command>) -> Vec<String> {
  std::iter::from_fn(|| sent.try_recv().ok())
    .filter_map(|command| match command {
      Command::FetchReadme(request) => Some(request.package.full_name()),
      _ => None,
    })
    .collect()
}

/// Opens Valheim on [`index`], with its default profile holding `installed`,
/// and returns the window with the commands the shell sends.
fn open_valheim(
  cx: &mut TestAppContext,
  installed: Vec<(String, InstalledPackage)>,
) -> (AnyWindowHandle, UnboundedReceiver<Command>) {
  cx.update(gpui_kit::init);

  let (commands, sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let (window, _) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(State::new(one_game()), commands, inbox, window, cx))
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
    Msg::CachedIndexRead {
      community: "valheim".to_string(),
      index: Ok(Some(index())),
    },
    Msg::ProfileOpened {
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      installed: Ok(installed.into_iter().collect()),
    },
  ] {
    answers.unbounded_send(msg).unwrap();
  }

  cx.run_until_parked();

  (window, sent)
}

/// `Owner-ModA` at 1.0.0 and 2.0.0, in Tools and Maps, with 1,000
/// downloads, and `Owner-ModB` at 1.0.0, updated before it.
fn index() -> Index {
  let version = |name: &str, number: &str, downloads: u32| {
    serde_json::json!({
      "name": name,
      "full_name": format!("Owner-{name}"),
      "description": "A mod",
      "icon": "icon.png",
      "version_number": number,
      "dependencies": [],
      "download_url": format!("http://mock/dl/{name}-{number}.zip"),
      "downloads": downloads,
      "date_created": format!("2024-01-0{}T12:00:00Z", number.chars().next().unwrap()),
      "website_url": "",
      "is_active": true,
      "uuid4": format!("ver-{name}-{number}"),
      "file_size": 1024,
    })
  };
  let package = |name: &str, updated: &str, categories: &[&str], versions| {
    serde_json::json!({
      "name": name,
      "full_name": format!("Owner-{name}"),
      "owner": "Owner",
      "package_url": format!("https://example.com/{name}"),
      "date_created": "2024-01-01T12:00:00Z",
      "date_updated": updated,
      "uuid4": format!("pkg-{name}"),
      "rating_score": 1,
      "is_pinned": false,
      "is_deprecated": false,
      "has_nsfw_content": false,
      "categories": categories,
      "versions": versions,
    })
  };
  let packages = serde_json::json!([
    package(
      "ModA",
      "2024-03-01T12:00:00Z",
      &["Tools", "Maps"],
      vec![version("ModA", "2.0.0", 600), version("ModA", "1.0.0", 400)],
    ),
    package(
      "ModB",
      "2024-02-01T12:00:00Z",
      &[],
      vec![version("ModB", "1.0.0", 5)],
    ),
  ]);
  let packages: Vec<Package> = serde_json::from_value(packages).unwrap();

  PackageIndex::from(packages).into()
}

/// An enabled 1.0.0 package by `Owner`.
fn installed(full_name: &str) -> (String, InstalledPackage) {
  (
    full_name.to_string(),
    InstalledPackage {
      author: "Owner".to_string(),
      version: "1.0.0".to_string(),
      enabled: true,
      dependencies: vec![],
      installed_at: 0,
      pinned: false,
    },
  )
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
      "modloaderPackages": []
    }"#,
  )
  .unwrap()
}
