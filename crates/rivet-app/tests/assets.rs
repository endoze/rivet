use gpui_kit::AssetSource as _;
use gpui_kit::assets::IconName;
use rivet_app::assets::Assets;

#[test]
fn the_redesigns_icons_load_beside_the_component_defaults() {
  let icons = [
    IconName::Search,
    IconName::X,
    IconName::Gamepad2,
    IconName::RefreshCw,
    IconName::Settings,
    IconName::Ellipsis,
    IconName::ChevronDown,
    IconName::ChevronRight,
    IconName::ChevronsUpDown,
    IconName::Check,
    IconName::List,
    IconName::LayoutGrid,
  ];

  for icon in icons {
    let svg = Assets.load(&icon.path()).ok().flatten();

    assert!(svg.is_some(), "{icon:?} is not bundled");
  }
}

#[test]
fn the_rest_of_lucide_stays_out_of_the_binary() {
  let unused = Assets.load(&IconName::Anchor.path()).ok().flatten();
  let bundled = Assets.list("icons/").expect("the icons list");

  assert!(unused.is_none(), "an icon Rivet never draws is bundled");
  assert!(
    bundled.len() < IconName::ALL.len() / 2,
    "{} of {} Lucide icons are bundled",
    bundled.len(),
    IconName::ALL.len()
  );
}
