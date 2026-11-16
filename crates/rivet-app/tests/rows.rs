use futures::channel::mpsc::unbounded;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
  AnyWindowHandle, AppContext as _, Bounds, Pixels, TestAppContext, Window, WindowOptions, px,
};
use rivet_app::Shell;
use rivet_core::browse::Index;
use rivet_core::install::{DEFAULT_PROFILE, InstalledPackage};
use rivet_core::settings::{GameSettings, Settings};
use rivet_core::{Command, Msg, State};
use thunderstore_engine::ecosystem::Ecosystem;
use thunderstore_engine::models::{Package, PackageIndex};

/// Many lines' worth of words, broken by newlines, with a URL too long to
/// fit on any line.
const LONG_DESCRIPTION: &str = "Adds a great many things to the game.\n\nFeatures:\n- \
  more items\n- more enemies\n- more biomes\r\n\tSee https://example.com/a/really/long/\
  path/that/never/breaks/anywhere/because/it/has/no/spaces/at/all/and/keeps/going/and/going \
  for the full list of everything this mod changes, adds, removes, and rebalances, which \
  is far longer than a single row could ever show.";
const LONG_OWNER: &str = "SomeoneWithAnAuthorNameFarTooLongForAnyRowToShowInFull";
const LONG_NAME: &str = "AModWhoseNameKeepsGoingWellPastTheEdgeOfTheRowItSitsIn";
/// The game's mod loader, which [`one_game`] names.
const LOADER: &str = "denikson-BepInExPack_Valheim";
/// The spec's icon size for a row.
const ICON_SIZE: f32 = 40.;

#[gpui_kit::test]
fn a_package_row_keeps_long_text_inside_one_uniform_row(cx: &mut TestAppContext) {
  let window = open_valheim(cx, vec![]);
  let rows = [
    "Owner-Plain".to_string(),
    "Owner-Wordy".to_string(),
    format!("{LONG_OWNER}-{LONG_NAME}"),
  ];

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("tab-browse", cx);
    window.render_frame(cx);

    assert_rows_hold_their_text(window, &rows);

    let description = |window: &mut Window, row: &str| {
      window
        .within(row.to_string())
        .find("description")
        .bounds()
        .size
        .height
    };
    let one_line = description(window, &rows[0]);
    let wordy = description(window, &rows[1]);

    assert_eq!(wordy, one_line, "a description is more than one line");

    for row in &rows {
      assert_icon(window, row);
      assert!(
        window.within(row.clone()).try_find("downloads").is_some(),
        "{row} does not show its downloads"
      );
    }
  })
  .unwrap();
}

#[gpui_kit::test]
fn a_profile_row_shows_an_icon_its_tags_and_a_switch(cx: &mut TestAppContext) {
  let (unlisted, mut off) = installed("Owner-Unlisted", "Owner");

  off.enabled = false;

  let window = open_valheim(
    cx,
    vec![
      installed("Owner-Plain", "Owner"),
      (unlisted, off),
      installed(LOADER, "denikson"),
    ],
  );

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("tab-installed", cx);
    window.render_frame(cx);

    for name in ["Owner-Plain", "Owner-Unlisted", LOADER] {
      assert_icon(window, &format!("profile-{name}"));
    }

    assert_eq!(window.find("switch-Owner-Plain").checked(), Some(true));
    assert_eq!(window.find("switch-Owner-Unlisted").checked(), Some(false));
    assert!(
      window.try_find(format!("switch-{LOADER}")).is_none(),
      "the mod loader can be switched off"
    );
    assert!(
      window
        .within(format!("profile-{LOADER}"))
        .try_find(format!("loader-badge-{LOADER}"))
        .is_some(),
      "the mod loader is not tagged as one"
    );
  })
  .unwrap();
}

#[gpui_kit::test]
fn a_row_shows_initials_when_its_icon_fails_to_load(cx: &mut TestAppContext) {
  let window = open_valheim(cx, vec![]);

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("tab-browse", cx);
    window.render_frame(cx);
  })
  .unwrap();
  cx.run_until_parked();
  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);

    assert!(
      window.within("Owner-Plain").try_find("initials").is_some(),
      "a row whose icon failed to load shows no initials"
    );
  })
  .unwrap();
}

#[gpui_kit::test]
fn a_row_shows_initials_when_it_has_no_icon(cx: &mut TestAppContext) {
  let window = open_valheim(cx, vec![installed("Owner-Unlisted", "Owner")]);

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("tab-installed", cx);
    window.render_frame(cx);

    assert!(
      window
        .within("profile-Owner-Unlisted")
        .try_find("initials")
        .is_some(),
      "a row without an icon shows no initials"
    );
  })
  .unwrap();
}

#[gpui_kit::test]
fn a_rows_main_action_shows_on_hover_and_on_the_selected_row_without_resizing_it(
  cx: &mut TestAppContext,
) {
  let window = open_valheim(cx, vec![]);

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("tab-browse", cx);
    window.render_frame(cx);

    let shown = |window: &mut Window, row: &str| window.find(format!("actions-{row}")).visible();
    let row = window.find("Owner-Wordy").bounds();
    let actions = window.find("actions-Owner-Wordy").bounds();

    assert!(
      shown(window, "Owner-Plain"),
      "the row shown by default shows no action"
    );
    assert!(!shown(window, "Owner-Wordy"), "an action shows at rest");

    window.hover("Owner-Wordy", cx);

    assert!(shown(window, "Owner-Wordy"), "hovering shows no action");
    assert!(window.try_find("install-Owner-Wordy").is_some());
    assert_eq!(
      window.find("Owner-Wordy").bounds(),
      row,
      "hovering resized the row"
    );
    assert_eq!(
      window.find("actions-Owner-Wordy").bounds(),
      actions,
      "hovering moved the action"
    );

    window.click("Owner-Wordy", cx);
    window.hover("toolbar", cx);

    assert!(
      shown(window, "Owner-Wordy"),
      "the selected row shows no action"
    );
    assert!(!shown(window, "Owner-Plain"));
  })
  .unwrap();
}

#[gpui_kit::test]
fn a_long_profile_list_builds_only_the_rows_in_view(cx: &mut TestAppContext) {
  let mods = (0..300)
    .map(|n| installed(&format!("Owner-Mod{n:03}"), "Owner"))
    .collect();
  let window = open_valheim(cx, mods);

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("tab-installed", cx);
    window.render_frame(cx);

    assert!(window.try_find("profile-Owner-Mod000").is_some());
    assert!(
      window.try_find("profile-Owner-Mod299").is_none(),
      "rows out of view were built"
    );
  })
  .unwrap();
}

/// Asserts that `row` shows a square icon [`ICON_SIZE`] wide.
fn assert_icon(window: &mut Window, row: &str) {
  let icon = window.within(row.to_string()).find("icon").bounds().size;

  assert_eq!(
    icon.width,
    px(ICON_SIZE),
    "{row}'s icon is not {ICON_SIZE}px"
  );
  assert_eq!(icon.height, px(ICON_SIZE), "{row}'s icon is not square");
}

#[gpui_kit::test]
fn an_installed_row_keeps_long_text_inside_one_uniform_row(cx: &mut TestAppContext) {
  let long = format!("{LONG_OWNER}-{LONG_NAME}");
  let window = open_valheim(
    cx,
    vec![
      installed("Owner-Plain", "Owner"),
      installed("Owner-Other", "Owner"),
      installed(&long, LONG_OWNER),
    ],
  );

  cx.update_window(window, |_, window, cx| {
    window.render_frame(cx);
    window.click("tab-installed", cx);
    window.render_frame(cx);

    let rows = ["Owner-Plain", "Owner-Other", long.as_str()].map(|name| format!("profile-{name}"));

    assert_rows_hold_their_text(window, &rows);
  })
  .unwrap();
}

/// Asserts that each of `rows` is as tall as the first, is apart from the
/// next, and holds its name, byline, and any description inside its bounds,
/// its name and byline as tall as the first row's, so on one line.
fn assert_rows_hold_their_text(window: &mut Window, rows: &[String]) {
  let plain = &rows[0];
  let height = window.find(plain.clone()).bounds().size.height;
  let mut part_height = |row: &str, part: &'static str| {
    window
      .within(row.to_string())
      .find(part)
      .bounds()
      .size
      .height
  };
  let name = part_height(plain, "name");
  let byline = part_height(plain, "byline");
  let mut bounds: Vec<Bounds<Pixels>> = Vec::new();

  for row in rows {
    let outer = window.find(row.clone()).bounds();

    assert_eq!(outer.size.height, height, "{row} is not as tall as {plain}");

    for part in ["name", "byline", "description"] {
      let Some(inner) = window.within(row.clone()).try_find(part) else {
        continue;
      };
      let inner = inner.bounds();

      assert!(
        inside(inner, outer),
        "{row}'s {part} at {inner:?} spills out of its row at {outer:?}"
      );
    }

    let mut part_height =
      |part: &'static str| window.within(row.clone()).find(part).bounds().size.height;

    assert_eq!(part_height("name"), name, "{row}'s name is not one line");
    assert_eq!(
      part_height("byline"),
      byline,
      "{row}'s byline is not one line"
    );

    bounds.push(outer);
  }

  bounds.sort_by(|a, b| a.origin.y.partial_cmp(&b.origin.y).unwrap());

  for pair in bounds.windows(2) {
    let gap = pair[1].origin.y - pair[0].bottom_left().y;

    assert!(gap >= px(2.), "rows {pair:?} are {gap:?} apart, too close");
  }
}

fn inside(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
  let (top_left, bottom_right) = (inner.origin, inner.bottom_right());

  top_left.x >= outer.origin.x
    && top_left.y >= outer.origin.y
    && bottom_right.x <= outer.bottom_right().x
    && bottom_right.y <= outer.bottom_right().y
}

/// Opens Valheim on [`index`], with its default profile holding `installed`.
fn open_valheim(
  cx: &mut TestAppContext,
  installed: Vec<(String, InstalledPackage)>,
) -> AnyWindowHandle {
  cx.update(gpui_kit::init);

  let (commands, _sent) = unbounded::<Command>();
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

  window
}

/// A plain package, one with [`LONG_DESCRIPTION`], and one with
/// [`LONG_OWNER`] and [`LONG_NAME`].
fn index() -> Index {
  let package = |owner: &str, name: &str, description: &str| {
    serde_json::json!({
      "name": name,
      "full_name": format!("{owner}-{name}"),
      "owner": owner,
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
        "full_name": format!("{owner}-{name}"),
        "description": description,
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
  let packages = serde_json::json!([
    package("Owner", "Plain", "A mod"),
    package("Owner", "Wordy", LONG_DESCRIPTION),
    package(LONG_OWNER, LONG_NAME, "A mod"),
  ]);
  let packages: Vec<Package> = serde_json::from_value(packages).unwrap();

  PackageIndex::from(packages).into()
}

/// An enabled 1.0.0 package by `author`.
fn installed(full_name: &str, author: &str) -> (String, InstalledPackage) {
  (
    full_name.to_string(),
    InstalledPackage {
      author: author.to_string(),
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
      "modloaderPackages": [
        { "packageId": "denikson-BepInExPack_Valheim", "rootFolder": "BepInExPack_Valheim", "loader": "bepinex" }
      ]
    }"#,
  )
  .unwrap()
}
