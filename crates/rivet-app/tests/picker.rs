use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext, WindowOptions, px};
use rivet_app::Shell;
use rivet_core::picker::Step;
use rivet_core::settings::{GameSettings, PickerLayout, Settings};
use rivet_core::{Command, Msg, State};
use tempfile::TempDir;
use thunderstore_engine::ecosystem::Ecosystem;

/// A 1x1 blue PNG.
const IMAGE: &[u8] = &[
  0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
  0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
  0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0x30, 0x4E, 0x9B, 0x09,
  0x00, 0x02, 0x02, 0x01, 0x33, 0x87, 0xA4, 0xFD, 0x0B, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E,
  0x44, 0xAE, 0x42, 0x60, 0x82,
];

const LAYOUTS: [&str; 2] = ["picker-list", "picker-grid"];

#[gpui_kit::test]
fn the_layout_control_switches_the_picker_between_a_list_and_a_grid(cx: &mut TestAppContext) {
  let mut picker = open_picker(cx, PickerLayout::List);

  cx.update_window(picker.window, |_, window, cx| {
    window.render_frame(cx);

    assert_only_selected(window, "picker-list");
    assert!(window.try_find("games-list").is_some());
    assert!(window.try_find("games-grid").is_none());

    window.click("picker-grid", cx);
    window.render_frame(cx);

    assert_only_selected(window, "picker-grid");
    assert!(window.try_find("games-grid").is_some());
    assert!(window.try_find("games-list").is_none());
  })
  .unwrap();

  let saved =
    std::iter::from_fn(|| picker.sent.try_recv().ok()).find_map(|command| match command {
      Command::SaveSettings(settings) => Some(settings.picker_layout),
      _ => None,
    });

  assert_eq!(saved, Some(PickerLayout::Grid));
}

#[gpui_kit::test]
fn the_search_and_layout_control_stay_put_and_centered_in_either_layout(cx: &mut TestAppContext) {
  let picker = open_picker(cx, PickerLayout::List);

  cx.update_window(picker.window, |_, window, cx| {
    window.render_frame(cx);

    let search = window.find("picker-search").bounds();
    let layouts = window.find("picker-layout").bounds();
    let width = window.viewport_size().width;

    assert!(
      (search.left() - (width - layouts.right())).abs() < px(1.),
      "the search and layout control are not centered"
    );

    window.click("picker-grid", cx);
    window.render_frame(cx);

    assert_eq!(window.find("picker-search").bounds(), search);
    assert_eq!(window.find("picker-layout").bounds(), layouts);
  })
  .unwrap();
}

#[gpui_kit::test]
fn the_picker_opens_in_the_saved_layout(cx: &mut TestAppContext) {
  let picker = open_picker(cx, PickerLayout::Grid);

  cx.update_window(picker.window, |_, window, cx| {
    window.render_frame(cx);

    assert_only_selected(window, "picker-grid");
    assert!(window.try_find("games-grid").is_some());
  })
  .unwrap();
}

#[gpui_kit::test]
fn a_list_row_shows_initials_until_its_cover_is_cached(cx: &mut TestAppContext) {
  assert_cover_replaces_initials(cx, PickerLayout::List);
}

#[gpui_kit::test]
fn a_grid_tile_shows_initials_until_its_cover_is_cached(cx: &mut TestAppContext) {
  assert_cover_replaces_initials(cx, PickerLayout::Grid);
}

#[gpui_kit::test]
fn picking_a_game_from_the_list_opens_it(cx: &mut TestAppContext) {
  assert_picking_opens(cx, PickerLayout::List);
}

#[gpui_kit::test]
fn picking_a_game_from_the_grid_opens_it(cx: &mut TestAppContext) {
  assert_picking_opens(cx, PickerLayout::Grid);
}

#[gpui_kit::test]
fn the_list_puts_games_with_a_folder_under_their_own_heading(cx: &mut TestAppContext) {
  let mut settings = Settings::default();

  settings.games.insert(
    "riskofrain2".to_string(),
    GameSettings::new("/games/RiskOfRain2"),
  );

  let picker = open_picker_with(cx, settings);

  cx.update_window(picker.window, |_, window, cx| {
    window.render_frame(cx);

    let top = |window: &mut gpui_kit::Window, id: &'static str| window.find(id).bounds().top();
    let yours = top(window, "your-games");
    let set_up = top(window, "riskofrain2");
    let all = top(window, "all-games");
    let rest = top(window, "valheim");

    assert!(
      yours < set_up && set_up < all && all < rest,
      "the set-up game is not listed first under its own heading"
    );
    assert_eq!(
      window.within("riskofrain2").find("ready").label(),
      Some("Ready")
    );
    assert!(window.within("valheim").try_find("ready").is_none());
  })
  .unwrap();
}

#[gpui_kit::test]
fn the_list_has_no_headings_while_no_game_has_a_folder(cx: &mut TestAppContext) {
  let picker = open_picker(cx, PickerLayout::List);

  cx.update_window(picker.window, |_, window, cx| {
    window.render_frame(cx);

    assert!(window.try_find("your-games").is_none());
    assert!(window.try_find("all-games").is_none());
  })
  .unwrap();
}

#[gpui_kit::test]
fn a_list_row_names_the_stores_its_game_is_sold_on(cx: &mut TestAppContext) {
  let picker = open_picker(cx, PickerLayout::List);

  cx.update_window(picker.window, |_, window, cx| {
    window.render_frame(cx);

    assert_eq!(
      window.within("riskofrain2").find("stores").label(),
      Some("Steam · Epic Games Store")
    );
    assert!(
      window.within("valheim").try_find("stores").is_none(),
      "a game sold nowhere names a store"
    );
  })
  .unwrap();
}

#[gpui_kit::test]
fn a_game_whose_mods_cannot_be_installed_is_marked_and_still_opens(cx: &mut TestAppContext) {
  for layout in [PickerLayout::List, PickerLayout::Grid] {
    let picker = open_picker(cx, layout);

    cx.update_window(picker.window, |_, window, cx| {
      window.render_frame(cx);

      assert_eq!(
        window.within("riskofrain2").find("no-mod-support").label(),
        Some("Mods not supported yet"),
        "{layout:?}"
      );
      assert!(
        window
          .within("valheim")
          .try_find("no-mod-support")
          .is_none(),
        "{layout:?}"
      );

      window.click("riskofrain2", cx);
    })
    .unwrap();

    assert_eq!(
      picker
        .shell
        .read_with(cx, |shell, _| shell.state().picker.step.clone()),
      Step::Choosing("riskofrain2".to_string()),
      "{layout:?}"
    );
  }
}

#[gpui_kit::test]
fn the_list_is_a_centered_column_narrower_than_the_window(cx: &mut TestAppContext) {
  let picker = open_picker(cx, PickerLayout::List);

  cx.update_window(picker.window, |_, window, cx| {
    window.render_frame(cx);

    let row = window.find("valheim").bounds();
    let width = window.viewport_size().width;
    let left = row.left();
    let right = width - row.right();

    assert!(
      row.size.width < width * 0.75,
      "a row spans the window: {row:?}"
    );
    assert!(
      (left - right).abs() < px(1.),
      "the list is not centered: {left:?} left, {right:?} right"
    );
  })
  .unwrap();
}

/// Asserts that in `layout`, Valheim shows its initials and no cover, then
/// its cover and no initials once the cover is cached.
fn assert_cover_replaces_initials(cx: &mut TestAppContext, layout: PickerLayout) {
  let picker = open_picker(cx, layout);
  let dir = TempDir::new().unwrap();
  let cover = dir.path().join("valheim.png");

  std::fs::write(&cover, IMAGE).unwrap();

  cx.update_window(picker.window, |_, window, cx| {
    window.render_frame(cx);

    let valheim = window.within("valheim");

    assert!(
      valheim.try_find("initials").is_some(),
      "{layout:?}: no initials"
    );
    assert!(
      valheim.try_find("cover").is_none(),
      "{layout:?}: a cover before caching"
    );
  })
  .unwrap();

  picker
    .answers
    .unbounded_send(Msg::CoversCached([("valheim".to_string(), cover)].into()))
    .unwrap();
  cx.run_until_parked();
  cx.update_window(picker.window, |_, window, cx| window.render_frame(cx))
    .unwrap();
  cx.run_until_parked();
  cx.update_window(picker.window, |_, window, cx| {
    window.render_frame(cx);

    let valheim = window.within("valheim");

    assert!(valheim.try_find("cover").is_some(), "{layout:?}: no cover");
    assert!(
      valheim.try_find("initials").is_none(),
      "{layout:?}: initials over the cover"
    );
    assert!(
      window.within("riskofrain2").try_find("initials").is_some(),
      "{layout:?}: a game without a cover shows no initials"
    );
  })
  .unwrap();
}

/// Asserts that clicking Valheim in `layout` starts choosing its folder.
fn assert_picking_opens(cx: &mut TestAppContext, layout: PickerLayout) {
  let picker = open_picker(cx, layout);

  cx.update_window(picker.window, |_, window, cx| {
    window.render_frame(cx);
    window.click("valheim", cx);
  })
  .unwrap();

  assert_eq!(
    picker
      .shell
      .read_with(cx, |shell, _| shell.state().picker.step.clone()),
    Step::Choosing("valheim".to_string()),
    "{layout:?}"
  );
}

fn assert_only_selected(window: &mut gpui_kit::Window, chosen: &str) {
  for layout in LAYOUTS {
    assert_eq!(
      window.find(layout).checked(),
      Some(layout == chosen),
      "with {chosen} chosen, {layout} is not marked as it should be"
    );
  }
}

struct Picker {
  window: AnyWindowHandle,
  shell: Entity<Shell>,
  sent: UnboundedReceiver<Command>,
  answers: UnboundedSender<Msg>,
}

/// Opens the picker on a first start, in `layout`.
fn open_picker(cx: &mut TestAppContext, layout: PickerLayout) -> Picker {
  open_picker_with(
    cx,
    Settings {
      picker_layout: layout,
      ..Settings::default()
    },
  )
}

/// Opens the picker with `settings`, which name no last game.
fn open_picker_with(cx: &mut TestAppContext, settings: Settings) -> Picker {
  cx.update(gpui_kit::init);

  let (commands, sent) = unbounded::<Command>();
  let (answers, inbox) = unbounded::<Msg>();
  let (window, shell) = cx.update(|cx| {
    gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
      cx.new(|cx| Shell::new(State::new(two_games()), commands, inbox, window, cx))
    })
    .expect("open test window")
  });

  answers
    .unbounded_send(Msg::SettingsLoaded(Ok(settings)))
    .unwrap();
  cx.run_until_parked();

  Picker {
    window,
    shell,
    sent,
    answers,
  }
}

/// Valheim with a cover and no store, and Risk of Rain 2 without a cover,
/// listed on Steam and the Epic Games Store but with no Steam app id to find
/// its folder by, and no install rules, so Rivet cannot install its mods.
fn two_games() -> Ecosystem {
  Ecosystem::from_json(
    r#"{
      "games": {
        "valheim": {
          "label": "valheim",
          "meta": { "displayName": "Valheim", "iconUrl": "valheim/valheim-cover.webp" },
          "r2modman": [{
            "gameInstanceType": "game",
            "exeNames": ["valheim.x86_64"],
            "packageIndex": "https://thunderstore.io/c/valheim/api/v1/package/",
            "installRules": [{ "route": "BepInEx/plugins", "isDefaultLocation": true }]
          }]
        },
        "riskofrain2": {
          "label": "riskofrain2",
          "meta": { "displayName": "Risk of Rain 2" },
          "distributions": [
            { "platform": "steam", "identifier": "632360" },
            { "platform": "other", "identifier": "" },
            { "platform": "epic-games-store", "identifier": "4abf" }
          ],
          "r2modman": [{
            "gameInstanceType": "game",
            "exeNames": ["Risk of Rain 2.exe"],
            "packageIndex": "https://thunderstore.io/c/riskofrain2/api/v1/package/"
          }]
        }
      }
    }"#,
  )
  .unwrap()
}
