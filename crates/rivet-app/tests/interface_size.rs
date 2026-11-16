use futures::channel::mpsc::{UnboundedReceiver, unbounded};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
  AnyWindowHandle, AppContext as _, DEFAULT_WINDOW_SIZE, TestAppContext, Window, WindowOptions, px,
};
use rivet_app::Shell;
use rivet_core::browse::Index;
use rivet_core::install::DEFAULT_PROFILE;
use rivet_core::settings::{GameSettings, Settings};
use rivet_core::{Command, Msg, State};
use thunderstore_engine::ecosystem::Ecosystem;
use thunderstore_engine::models::{Package, PackageIndex};
use thunderstore_engine::profile::launch::HostOs;

/// The width and height of the test platform's only display, in pixels.
const TEST_DISPLAY: (f32, f32) = (1920., 1080.);

/// The interface size segments, Auto first.
const SIZES: [&str; 6] = [
  "interface-size-auto",
  "interface-size-90",
  "interface-size-100",
  "interface-size-115",
  "interface-size-130",
  "interface-size-150",
];

#[gpui_kit::test]
fn a_chosen_interface_size_scales_text_and_fixed_sizes_together_and_is_saved(
  cx: &mut TestAppContext,
) {
  let (window, mut sent) = open_valheim(cx, None);

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    assert_eq!(
      measure(window),
      [px(14.), px(248.), px(36.)],
      "the test display, which the OS scales, is not at today's size"
    );

    window.click("open-settings", cx);
    window.render_frame(cx);

    assert_only_selected(window, "interface-size-auto");

    window.click("interface-size-150", cx);
    window.render_frame(cx);

    assert_only_selected(window, "interface-size-150");

    window.click("close-settings", cx);
    window.render_frame(cx);

    assert_eq!(measure(window), [px(21.), px(372.), px(54.)]);
  })
  .unwrap();

  let saved = std::iter::from_fn(|| sent.try_recv().ok()).find_map(|command| match command {
    Command::SaveSettings(settings) => Some(settings.interface_size),
    _ => None,
  });

  assert_eq!(saved, Some(Some(150)));
}

#[gpui_kit::test]
fn a_large_size_grows_the_window_without_leaving_the_display(cx: &mut TestAppContext) {
  let (window, _) = open_valheim(cx, Some(150));

  cx.update_window(window, |_, window, cx| {
    let visible = window.display(cx).unwrap().visible_bounds();
    let bounds = window.bounds();

    assert!(
      bounds.size.width > DEFAULT_WINDOW_SIZE.width,
      "the window did not grow for 150%: {bounds:?}"
    );
    assert!(
      bounds.left() >= visible.left()
        && bounds.top() >= visible.top()
        && bounds.right() <= visible.right()
        && bounds.bottom() <= visible.bottom(),
      "the window at {bounds:?} leaves the display's visible {visible:?}"
    );
  })
  .unwrap();
}

#[gpui_kit::test]
fn the_game_screen_holds_its_layout_at_the_smallest_size(cx: &mut TestAppContext) {
  assert_game_layout_holds(cx, 90);
}

#[gpui_kit::test]
fn the_game_screen_holds_its_layout_at_the_largest_size(cx: &mut TestAppContext) {
  assert_game_layout_holds(cx, 150);
}

#[gpui_kit::test]
fn the_segmented_controls_hold_at_the_smallest_size(cx: &mut TestAppContext) {
  assert_segments_hold(cx, 90);
}

#[gpui_kit::test]
fn the_segmented_controls_hold_at_the_largest_size(cx: &mut TestAppContext) {
  assert_segments_hold(cx, 150);
}

/// Asserts that at `size` percent the fixed slots are their spec size
/// scaled, nothing overlaps, and the list stays put as the pane and the
/// views change.
fn assert_game_layout_holds(cx: &mut TestAppContext, size: u16) {
  let scale = f32::from(size) / 100.;
  let (window, _) = open_valheim(cx, Some(size));

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("tab-browse", cx);
    window.render_frame(cx);

    let viewport = window.viewport_size();
    let corner = window.bounds().origin;
    let room = (
      TEST_DISPLAY.0 - f32::from(corner.x),
      TEST_DISPLAY.1 - f32::from(corner.y),
    );

    assert_near(
      viewport.width,
      (1536. * scale).min(room.0),
      "the window's width",
    );
    assert_near(
      viewport.height,
      (1095. * scale).min(room.1),
      "the window's height",
    );

    let [sidebar, toolbar, list, pane, bar] =
      ["sidebar", "toolbar", "mod-list", "details", "status-bar"]
        .map(|id| window.find(id).bounds());

    assert_near(sidebar.size.width, 248. * scale, "the sidebar's width");
    assert_near(
      bar.size.height + px(1.),
      36. * scale,
      "the status bar's height",
    );
    assert!(
      toolbar.left() >= sidebar.right(),
      "the toolbar overlaps the sidebar: {sidebar:?} {toolbar:?}"
    );
    assert!(
      list.top() >= toolbar.bottom(),
      "the list overlaps the toolbar: {toolbar:?} {list:?}"
    );
    assert!(
      list.right() <= pane.left(),
      "the list overlaps the pane: {list:?} {pane:?}"
    );
    assert!(
      pane.size.width >= px(320. * scale) - px(1.),
      "the pane is narrower than its minimum: {pane:?}"
    );
    assert!(
      pane.right() <= viewport.width && bar.bottom() <= viewport.height,
      "the pane or the status bar runs off the window: {pane:?} {bar:?} in {viewport:?}"
    );

    let first_profile = window.find("pick-profile-Default").bounds();

    assert_apart(window, &["new-profile", "import-profile"]);

    for id in ["new-profile", "import-profile"] {
      let command = window.within("sidebar").find(id).bounds();

      assert!(
        command.bottom() <= first_profile.top()
          && command.left() >= sidebar.left()
          && command.right() <= sidebar.right(),
        "{id} at {command:?} is not above the profiles inside the sidebar at {sidebar:?}"
      );
    }

    for row in ["Owner-ModA", "Owner-ModB"] {
      let outer = window.find(row).bounds();
      let icon = window.within(row).find("icon").bounds().size;

      assert_near(
        outer.size.height,
        56. * scale,
        "a row's height, its gap aside",
      );
      assert_near(icon.width, 40. * scale, "a row icon's width");
      assert_near(icon.height, 40. * scale, "a row icon's height");

      for part in ["name", "byline"] {
        let inner = window.within(row).find(part).bounds();

        assert!(
          inner.top() >= outer.top() && inner.bottom() <= outer.bottom(),
          "{row}'s {part} at {inner:?} spills out of its row at {outer:?}"
        );
      }
    }

    for (step, id) in [
      ("closing the pane", "close-details"),
      ("reopening the pane", "open-details"),
      ("switching views", "tab-installed"),
    ] {
      window.click(id, cx);
      window.render_frame(cx);

      assert_eq!(
        window.find("mod-list").bounds().top(),
        list.top(),
        "{step} moved the list"
      );
      assert_eq!(
        window.find("toolbar").bounds(),
        toolbar,
        "{step} moved the toolbar"
      );
    }
  })
  .unwrap();
}

/// Asserts that at `size` percent each segmented control marks only the
/// chosen segment and lays its segments side by side, apart.
fn assert_segments_hold(cx: &mut TestAppContext, size: u16) {
  let (window, _) = open_valheim(cx, Some(size));
  let chosen = format!("interface-size-{size}");

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    assert_apart(window, &["tab-installed", "tab-browse"]);

    window.click("open-settings", cx);
    window.render_frame(cx);

    assert_only_selected(window, &chosen);
    assert_apart(window, &SIZES);
    assert_apart(window, &["theme-default", "theme-one"]);
    assert_apart(
      window,
      &["appearance-system", "appearance-light", "appearance-dark"],
    );
    assert_apart(
      window,
      &["runtime-detect", "runtime-native", "runtime-proton"],
    );
  })
  .unwrap();
}

/// Asserts that each of `segments` sits wholly left of the next.
fn assert_apart(window: &mut Window, segments: &[&'static str]) {
  let bounds: Vec<_> = segments
    .iter()
    .map(|id| window.find(*id).bounds())
    .collect();

  for (pair, ids) in bounds.windows(2).zip(segments.windows(2)) {
    assert!(
      pair[0].right() <= pair[1].left(),
      "{} overlaps {}: {:?} {:?}",
      ids[0],
      ids[1],
      pair[0],
      pair[1]
    );
  }
}

/// Asserts that `actual` is `expected` pixels, to within the pixel layout
/// rounds to.
fn assert_near(actual: gpui_kit::Pixels, expected: f32, what: &str) {
  assert!(
    (f32::from(actual) - expected).abs() <= 1.,
    "{what} is {actual:?}, not {expected}px"
  );
}

/// The body text size, the sidebar's width, and the status bar's height,
/// counting the 1px border over the part the bar's id marks.
fn measure(window: &mut Window) -> [gpui_kit::Pixels; 3] {
  [
    window.rem_size(),
    window.find("sidebar").bounds().size.width,
    window.find("status-bar").bounds().size.height + px(1.),
  ]
}

/// Asserts that of the interface sizes, `chosen` alone is marked selected.
fn assert_only_selected(window: &mut Window, chosen: &str) {
  for size in SIZES {
    assert_eq!(
      window.find(size).checked(),
      Some(size == chosen),
      "with {chosen} chosen, {size} is not marked as it should be"
    );
  }
}

/// Opens Valheim's default profile on Linux with `size` as the saved
/// interface size, in the window the app opens, and returns it with the
/// commands the shell sends.
fn open_valheim(
  cx: &mut TestAppContext,
  size: Option<u16>,
) -> (AnyWindowHandle, UnboundedReceiver<Command>) {
  cx.update(gpui_kit::init);

  let (commands, sent) = unbounded::<Command>();
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
    interface_size: size,
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
      installed: Ok([].into()),
    },
  ] {
    answers.unbounded_send(msg).unwrap();
  }

  cx.run_until_parked();
  // The test platform resizes a window without telling it, as a real one
  // does once the resize lands.
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.bounds_changed(cx);
  })
  .unwrap();

  (window, sent)
}

/// `Owner-ModA` and `Owner-ModB`, one version each.
fn index() -> Index {
  let package = |name: &str| {
    serde_json::json!({
      "name": name,
      "full_name": format!("Owner-{name}"),
      "owner": "Owner",
      "package_url": format!("https://example.com/{name}"),
      "date_created": "2024-01-01T12:00:00Z",
      "date_updated": "2024-01-02T12:00:00Z",
      "uuid4": format!("pkg-{name}"),
      "rating_score": 1,
      "is_pinned": false,
      "is_deprecated": false,
      "has_nsfw_content": false,
      "categories": [],
      "versions": [{
        "name": name,
        "full_name": format!("Owner-{name}"),
        "description": "A mod",
        "icon": "icon.png",
        "version_number": "1.0.0",
        "dependencies": [],
        "download_url": format!("http://mock/dl/{name}.zip"),
        "downloads": 1,
        "date_created": "2024-01-01T12:00:00Z",
        "website_url": "",
        "is_active": true,
        "uuid4": format!("ver-{name}"),
        "file_size": 1024,
      }],
    })
  };
  let packages: Vec<Package> =
    serde_json::from_value(serde_json::json!([package("ModA"), package("ModB")])).unwrap();

  PackageIndex::from(packages).into()
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
            "packageIndex": "https://thunderstore.io/c/valheim/api/v1/package/",
            "installRules": [{ "route": "BepInEx/plugins", "isDefaultLocation": true }]
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
