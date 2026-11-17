use futures::channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, TestAppContext, WindowOptions};
use rivet_app::Shell;
use rivet_core::browse::Index;
use rivet_core::handoff::Request;
use rivet_core::install::DEFAULT_PROFILE;
use rivet_core::install_link::{Claim, InstallLink};
use rivet_core::settings::{GameSettings, Settings};
use rivet_core::{Command, Msg, State};
use thunderstore_engine::ecosystem::Ecosystem;
use thunderstore_engine::models::{Package, PackageIndex};
use thunderstore_engine::test_support::index_json;

const LINK: &str = "ror2mm://v1/install/thunderstore.io/Owner/ModA/1.0.0/";

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
      }
    }"#,
  )
  .unwrap()
}

/// The mock Thunderstore's five packages, `Owner-ModA` first.
fn mock_index() -> Index {
  let packages: Vec<Package> = serde_json::from_str(&index_json("http://mock")).unwrap();

  PackageIndex::from(packages).into()
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

/// A window with Valheim set up and open, its list and Default profile
/// loaded, and the commands sent so far drained.
fn valheim_open(
  cx: &mut TestAppContext,
) -> (
  AnyWindowHandle,
  UnboundedSender<Msg>,
  UnboundedReceiver<Command>,
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
  let settings = Settings {
    last_game: Some("valheim".to_string()),
    games: [("valheim".to_string(), GameSettings::new("/games/Valheim"))].into(),
    ..Settings::default()
  };

  for msg in [
    Msg::SettingsLoaded(Ok(settings)),
    Msg::CachedIndexRead {
      community: "valheim".to_string(),
      index: Ok(Some(mock_index())),
    },
    Msg::ProfileOpened {
      game: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      installed: Ok(Default::default()),
    },
  ] {
    answers.unbounded_send(msg).unwrap();
    cx.run_until_parked();
  }

  while sent.try_recv().is_ok() {}

  (window, answers, sent)
}

fn drain(sent: &mut UnboundedReceiver<Command>) -> Vec<Command> {
  std::iter::from_fn(|| sent.try_recv().ok()).collect()
}

#[gpui_kit::test]
fn a_link_handed_over_asks_where_to_install_and_installs_there(cx: &mut TestAppContext) {
  let (window, answers, mut sent) = valheim_open(cx);
  let link = InstallLink::parse(LINK).unwrap();

  answers
    .unbounded_send(Msg::HandedOff(Request {
      open: Some(LINK.to_string()),
      ..Request::default()
    }))
    .unwrap();
  cx.run_until_parked();

  assert_eq!(drain(&mut sent), vec![Command::FindListings(link.clone())]);

  answers
    .unbounded_send(Msg::ListingsFound {
      link,
      communities: Ok(vec!["valheim".to_string()]),
    })
    .unwrap();
  cx.run_until_parked();

  assert!(
    shows(cx, window, "install-link-dialog"),
    "the link asked nothing"
  );

  click(cx, window, "confirm-install-link");

  assert!(
    drain(&mut sent).iter().any(|command| matches!(
      command,
      Command::Install(install)
        if install.package == "Owner-ModA" && install.profile == DEFAULT_PROFILE
    )),
    "confirming installed nothing"
  );
  assert!(
    !shows(cx, window, "install-link-dialog"),
    "the dialog stayed open"
  );
}

#[gpui_kit::test]
fn settings_turn_opening_install_links_in_rivet_on(cx: &mut TestAppContext) {
  let (window, answers, mut sent) = valheim_open(cx);

  answers
    .unbounded_send(Msg::InstallLinksRegistered(Ok(false)))
    .unwrap();
  cx.run_until_parked();
  click(cx, window, "open-settings");

  assert_eq!(
    cx.update_window(window, |_, window, _| window
      .find("install-links")
      .checked())
      .unwrap(),
    Some(false)
  );

  click(cx, window, "install-links");

  let commands = drain(&mut sent);

  assert!(
    commands.iter().any(
      |command| matches!(command, Command::SaveSettings(saved) if saved.install_links == Some(true))
    ),
    "turning install links on was not saved: {commands:?}"
  );
  assert!(
    commands.contains(&Command::RegisterInstallLinks(Claim::Always)),
    "turning install links on did not claim them: {commands:?}"
  );
}
