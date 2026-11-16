//! Exporting and importing profiles in the formats r2modman shares them in,
//! with the engine's portability functions.

use crate::Config;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use futures::channel::mpsc::UnboundedSender;
use rivet_core::Msg;
use rivet_core::install::{FailedPackage, Outcome};
use rivet_core::portability::{
  Destination, Export, Exported, Import, ReadFailure, Shared, SharedPackage, Source,
};
use rivet_core::r2modman::Found;
use std::path::Path;
use thunderstore_engine::profile::modlist;
use thunderstore_engine::profile::{self, layout, portability};

/// Exports `export.profile` to where `export.to` says.
pub(crate) async fn export(config: &Config, export: &Export) -> Result<Exported, String> {
  let folder = layout::profile_dir(&config.data, &export.game, &export.profile);

  match &export.to {
    Destination::Code => {
      let client = crate::client(config, &export.community).map_err(|error| error.to_string())?;

      portability::export_code_in(&folder, &export.profile, &client)
        .await
        .map(Exported::Code)
        .map_err(|error| error.to_string())
    }
    Destination::File(path) => {
      let (path, name) = (path.clone(), export.profile.clone());

      tokio::task::spawn_blocking(move || {
        let bytes = portability::build_r2z_in(&folder, &name).map_err(|error| error.to_string())?;

        std::fs::write(&path, bytes).map_err(|error| crate::failure(&path, error))?;

        Ok(Exported::File(path))
      })
      .await
      .map_err(|error| error.to_string())?
    }
  }
}

/// Reads what `source` shares, fetching a code through `community`.
pub(crate) async fn read_shared(
  config: &Config,
  community: &str,
  source: &Source,
) -> Result<Shared, ReadFailure> {
  let archive = match source {
    Source::File(path) => tokio::fs::read(path)
      .await
      .map_err(|error| ReadFailure::Unreadable(crate::failure(path, error)))?,
    Source::Code(code) => fetch_code(config, community, code).await?,
    Source::R2modman(folder) => r2modman_export(folder).await?,
  };
  let list =
    portability::read_export(&archive).map_err(|error| ReadFailure::Damaged(error.to_string()))?;
  let packages = list
    .mods
    .into_iter()
    .map(|entry| SharedPackage {
      full_name: entry.name,
      version: entry.version.to_string(),
      enabled: entry.enabled,
    })
    .collect();

  Ok(Shared {
    name: list.profile_name,
    packages,
    archive,
  })
}

/// The profiles r2modman holds in each of its data folders for the game it
/// files under each of `folders`.
pub(crate) async fn find_r2modman_profiles(config: &Config, folders: Vec<String>) -> Vec<Found> {
  let data = config.r2modman.clone();

  tokio::task::spawn_blocking(move || rivet_platform::r2modman::profiles(&data, &folders))
    .await
    .unwrap_or_default()
    .into_iter()
    .map(|folder| Found {
      name: profile_name(&folder),
      folder,
    })
    .collect()
}

/// The name of the r2modman profile in `folder`: the folder's own name.
fn profile_name(folder: &Path) -> String {
  folder
    .file_name()
    .map(|name| name.to_string_lossy().into_owned())
    .unwrap_or_default()
}

/// An export of the r2modman profile in `folder`, built from its mod list
/// and config files without writing anything there.
async fn r2modman_export(folder: &Path) -> Result<Vec<u8>, ReadFailure> {
  let folder = folder.to_path_buf();

  tokio::task::spawn_blocking(move || portability::build_r2z_in(&folder, &profile_name(&folder)))
    .await
    .map_err(|error| ReadFailure::Unreadable(error.to_string()))?
    .map_err(|error| ReadFailure::Unreadable(error.to_string()))
}

/// The text a profile code's body starts with, before the base64 of the
/// export.
const CODE_PREFIX: &str = "#r2modman";

/// The export Thunderstore holds under `code`.
async fn fetch_code(config: &Config, community: &str, code: &str) -> Result<Vec<u8>, ReadFailure> {
  let client = crate::client(config, community)
    .map_err(|error| ReadFailure::Unreachable(error.to_string()))?;
  let body = client
    .fetch_profile_code(code)
    .await
    .map_err(|error| match error {
      thunderstore_engine::Error::Config(_) => ReadFailure::UnknownCode,
      thunderstore_engine::Error::Http(http)
        if http.status().is_some_and(|status| status.as_u16() == 404) =>
      {
        ReadFailure::UnknownCode
      }
      other => ReadFailure::Unreachable(other.to_string()),
    })?;
  let encoded = std::str::from_utf8(&body)
    .ok()
    .and_then(|text| text.strip_prefix(CODE_PREFIX))
    .ok_or_else(|| ReadFailure::Damaged("Thunderstore's answer is not a profile".to_string()))?;

  STANDARD
    .decode(encoded.trim())
    .map_err(|error| ReadFailure::Damaged(error.to_string()))
}

/// Creates `import.profile`, installs `import.packages` into it at their
/// versions and enabled states, sending its progress to `progress` and
/// stopping when the operation is cancelled, then restores the config files
/// the export carries. An error means no profile was created.
///
/// Config files are restored only when every package installed, as the
/// engine's own import does. A failure after the profile was created names
/// the profile in the outcome.
pub(crate) async fn import(
  config: &Config,
  import: Import,
  progress: UnboundedSender<Msg>,
) -> Result<Outcome, String> {
  let Import {
    operation: id,
    game,
    community,
    profile: name,
    packages,
    archive,
    index,
  } = import;
  let client = crate::client(config, &community).map_err(|error| error.to_string())?;
  let (data, created_game, created) = (config.data.clone(), game.clone(), name.clone());
  let folder = tokio::task::spawn_blocking(move || layout::create(&data, &created_game, &created))
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| error.to_string())?;
  let names_where = |enabled: bool| {
    packages
      .iter()
      .filter(|package| package.enabled == enabled)
      .map(|package| package.full_name.clone())
      .collect()
  };
  let desired = packages
    .iter()
    .map(|package| format!("{}-{}", package.full_name, package.version))
    .collect();
  let mut batch = profile::InstallBatch::new(desired, names_where(false));

  batch.enable_requested = names_where(true);

  let operation = crate::operation(config, id, progress);
  let installed = profile::install_batch_with(
    &folder,
    &config.data,
    &config.ecosystem,
    &index.0,
    &client,
    &game,
    &batch,
    modlist::now_millis(),
    &operation,
  )
  .await;
  let installed = match installed {
    Ok(installed) => installed,
    Err(error) => {
      return Ok(Outcome {
        failed: vec![FailedPackage {
          package: name,
          reason: error.to_string(),
        }],
        ..crate::batch_outcome(&folder, profile::BatchOutcome::default())
      });
    }
  };
  let complete = installed.is_ok();
  let mut outcome = crate::batch_outcome(&folder, installed);

  if complete {
    let restored = tokio::task::spawn_blocking(move || restore_configs(&folder, &archive))
      .await
      .map_err(|error| error.to_string())
      .and_then(|restored| restored);

    if let Err(reason) = restored {
      outcome.failed.push(FailedPackage {
        package: name,
        reason,
      });
    }
  }

  Ok(outcome)
}

/// Restores the config files `archive` carries into the profile an import
/// just created in `folder`, which starts with none of the app's own files,
/// such as pins.
fn restore_configs(folder: &Path, archive: &[u8]) -> Result<(), String> {
  portability::extract_configs_in(folder, archive).map_err(|error| error.to_string())?;

  let own = crate::app_folder(folder);

  match std::fs::remove_dir_all(&own) {
    Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(crate::failure(&own, error)),
    _ => Ok(()),
  }
}

#[cfg(test)]
mod tests {
  use super::STANDARD;
  use crate::Config;
  use crate::tests::{
    config_for, config_in, install_at, mock_index, mock_thunderstore, names, packages,
    profile_folder, run_any, run_now, run_reporting, set_enabled, with_mod_c,
  };
  use base64::Engine as _;
  use rivet_core::install::{DEFAULT_PROFILE, Outcome, Stage};
  use rivet_core::portability::{
    Destination, Export, Exported, Import, ReadFailure, Shared, SharedPackage, Source,
  };
  use rivet_core::r2modman::Found;
  use rivet_core::{Command, Msg};
  use std::collections::BTreeMap;
  use std::io::Write as _;
  use std::path::{Path, PathBuf};
  use tempfile::TempDir;
  use thunderstore_engine::profile::portability::read_export;
  use thunderstore_engine::test_support::MockThunderstore;
  use thunderstore_engine::test_support::mockito::Matcher;

  /// The `export.r2x` r2modman writes for a profile named `Shared` holding
  /// `Owner-ModA` 1.0.0 enabled and `Owner-ModD` 1.0.0 disabled.
  const R2MODMAN_LIST: &str = "profileName: Shared
mods:
  - name: Owner-ModA
    version:
      major: 1
      minor: 0
      patch: 0
    enabled: true
  - name: Owner-ModD
    version:
      major: 1
      minor: 0
      patch: 0
    enabled: false
";

  /// A zip holding `entries`, by name and content.
  fn zip(entries: &[(&str, &str)]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();

    for (name, body) in entries {
      zip.start_file(*name, options).unwrap();
      zip.write_all(body.as_bytes()).unwrap();
    }

    zip.finish().unwrap().into_inner()
  }

  /// A `.r2z` as r2modman exports it: the list, and a config file.
  fn r2modman_export() -> Vec<u8> {
    zip(&[
      ("export.r2x", R2MODMAN_LIST),
      ("config/Owner.ModA.cfg", "[General]\nSpeed = 2\n"),
    ])
  }

  /// The packages [`R2MODMAN_LIST`] lists.
  fn listed() -> Vec<SharedPackage> {
    vec![
      SharedPackage {
        full_name: "Owner-ModA".to_string(),
        version: "1.0.0".to_string(),
        enabled: true,
      },
      SharedPackage {
        full_name: "Owner-ModD".to_string(),
        version: "1.0.0".to_string(),
        enabled: false,
      },
    ]
  }

  fn export(to: Destination) -> Command {
    Command::Export(Export {
      game: "valheim".to_string(),
      community: "valheim".to_string(),
      profile: DEFAULT_PROFILE.to_string(),
      to,
    })
  }

  fn exported(answer: Msg) -> Result<Exported, String> {
    match answer {
      Msg::ProfileExported { result, .. } => result,
      other => panic!("unexpected answer {other:?}"),
    }
  }

  #[test]
  fn an_exported_file_lists_each_package_at_its_version_and_state_with_the_configs() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = with_mod_c(&dir, &mut mock);
    let configs = profile_folder(&config, DEFAULT_PROFILE).join("BepInEx/config");
    let path = dir.path().join("Default.r2z");

    run_now(set_enabled(&["Owner-ModC"], false), &config);
    std::fs::create_dir_all(&configs).unwrap();
    std::fs::write(configs.join("Owner.ModC.cfg"), "Speed = 3").unwrap();

    let answer = exported(run_now(export(Destination::File(path.clone())), &config));
    let bytes = std::fs::read(&path).unwrap();
    let list = read_export(&bytes).unwrap();
    let mods: Vec<(String, String, bool)> = list
      .mods
      .iter()
      .map(|entry| (entry.name.clone(), entry.version.to_string(), entry.enabled))
      .collect();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();

    assert_eq!(answer, Ok(Exported::File(path)));
    assert_eq!(list.profile_name, DEFAULT_PROFILE);
    assert_eq!(
      mods,
      [
        ("Owner-ModA".to_string(), "1.0.0".to_string(), true),
        ("Owner-ModC".to_string(), "1.0.0".to_string(), false),
      ]
    );
    assert!(archive.by_name("config/Owner.ModC.cfg").is_ok());
  }

  #[test]
  fn pins_do_not_travel_in_an_export() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = with_mod_c(&dir, &mut mock);
    let index = mock_index(&mut mock);
    let path = dir.path().join("Default.r2z");

    run_now(install_at("Owner-ModD", Some("1.0.0"), &index), &config);
    exported(run_now(export(Destination::File(path.clone())), &config)).unwrap();

    let archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
    let names: Vec<&str> = archive.file_names().collect();

    assert!(names.iter().all(|name| !name.contains("pins")), "{names:?}");
  }

  #[test]
  fn an_export_that_cannot_be_written_says_why() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = with_mod_c(&dir, &mut mock);

    let answer = exported(run_now(
      export(Destination::File(dir.path().join("gone/Default.r2z"))),
      &config,
    ));

    assert!(answer.is_err(), "{answer:?}");
  }

  const CREATE_PATH: &str = "/api/experimental/legacyprofile/create/";

  fn code_path(code: &str) -> String {
    format!("/api/experimental/legacyprofile/get/{code}/")
  }

  #[test]
  fn an_exported_code_is_the_key_thunderstore_gives_an_r2modman_upload() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = with_mod_c(&dir, &mut mock);
    let upload = mock
      .server_mut()
      .mock("POST", CREATE_PATH)
      .match_header("Content-Type", "application/octet-stream")
      .match_body(Matcher::Regex("^#r2modman\n[A-Za-z0-9+/=]+$".to_string()))
      .with_status(200)
      .with_body(r#"{"key":"0193e2b5-shared"}"#)
      .create();

    let answer = exported(run_now(export(Destination::Code), &config));

    upload.assert();
    assert_eq!(answer, Ok(Exported::Code("0193e2b5-shared".to_string())));
  }

  fn read(source: Source) -> Command {
    Command::ReadShared {
      community: "valheim".to_string(),
      source,
    }
  }

  fn shared(answer: Msg) -> Result<Shared, ReadFailure> {
    match answer {
      Msg::SharedRead { shared, .. } => shared,
      other => panic!("unexpected answer {other:?}"),
    }
  }

  #[test]
  fn reading_an_r2modman_file_lists_its_packages_and_keeps_the_archive() {
    let dir = TempDir::new().unwrap();
    let mock = mock_thunderstore();
    let config = config_for(&dir, &mock);
    let path = dir.path().join("Shared_1700000000000.r2z");

    std::fs::write(&path, r2modman_export()).unwrap();

    let answer = shared(run_now(read(Source::File(path)), &config));

    assert_eq!(
      answer,
      Ok(Shared {
        name: "Shared".to_string(),
        packages: listed(),
        archive: r2modman_export(),
      })
    );
  }

  #[test]
  fn reading_an_r2modman_code_fetches_the_profile_it_shares() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = config_for(&dir, &mock);
    let body = format!("#r2modman\n{}", STANDARD.encode(r2modman_export()));

    mock
      .server_mut()
      .mock("GET", code_path("0193e2b5-shared").as_str())
      .with_status(200)
      .with_body(body)
      .create();

    let answer = shared(run_now(
      read(Source::Code("0193e2b5-shared".to_string())),
      &config,
    ));

    assert_eq!(answer.map(|shared| shared.packages), Ok(listed()));
  }

  #[test]
  fn a_code_thunderstore_does_not_know_or_could_not_be_is_unknown() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = config_for(&dir, &mock);

    mock
      .server_mut()
      .mock("GET", code_path("expired").as_str())
      .with_status(404)
      .create();

    for code in ["expired", "not a code"] {
      let answer = shared(run_now(read(Source::Code(code.to_string())), &config));

      assert_eq!(answer, Err(ReadFailure::UnknownCode), "{code}");
    }
  }

  #[test]
  fn a_damaged_file_or_code_says_so_and_a_missing_file_cannot_be_read() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = config_for(&dir, &mock);
    let damaged = dir.path().join("damaged.r2z");

    std::fs::write(&damaged, b"PK\x03\x04 not really a zip").unwrap();
    mock
      .server_mut()
      .mock("GET", code_path("garbled").as_str())
      .with_status(200)
      .with_body("not a profile")
      .create();

    let file = shared(run_now(read(Source::File(damaged)), &config));
    let code = shared(run_now(read(Source::Code("garbled".to_string())), &config));
    let missing = shared(run_now(
      read(Source::File(dir.path().join("missing.r2z"))),
      &config,
    ));

    assert!(matches!(file, Err(ReadFailure::Damaged(_))), "{file:?}");
    assert!(matches!(code, Err(ReadFailure::Damaged(_))), "{code:?}");
    assert!(
      matches!(missing, Err(ReadFailure::Unreadable(_))),
      "{missing:?}"
    );
  }

  fn import(mock: &mut MockThunderstore) -> Command {
    Command::Import(Import {
      operation: 3,
      game: "valheim".to_string(),
      community: "valheim".to_string(),
      profile: "Shared".to_string(),
      packages: listed(),
      archive: r2modman_export(),
      index: mock_index(mock),
    })
  }

  #[test]
  fn an_import_creates_the_profile_with_each_package_at_its_version_and_state_and_the_configs() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = config_for(&dir, &mock);
    let command = import(&mut mock);

    let (progress, answer) = run_reporting(command, &config);

    let Msg::OperationFinished {
      operation: 3,
      outcome: Ok(outcome),
    } = answer
    else {
      panic!("unexpected answer {answer:?}");
    };
    let folder = profile_folder(&config, "Shared");

    assert_eq!(outcome.changed, names(&["Owner-ModA", "Owner-ModD"]));
    assert_eq!(outcome.failed, vec![]);
    assert_eq!(
      outcome.profile.as_ref().map(packages),
      Ok(vec![
        ("Owner-ModA".to_string(), "1.0.0".to_string(), true),
        ("Owner-ModD".to_string(), "1.0.0".to_string(), false),
      ])
    );
    assert_eq!(
      std::fs::read_to_string(folder.join("BepInEx/config/Owner.ModA.cfg")).unwrap(),
      "[General]\nSpeed = 2\n"
    );
    assert!(
      progress.iter().any(|msg| matches!(
        msg,
        Msg::OperationProgress { operation: 3, progress } if progress.stage == Stage::Installing
      )),
      "{progress:?}"
    );
  }

  #[test]
  fn a_cancelled_import_installs_nothing_more_and_restores_no_configs() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = config_for(&dir, &mock);
    let command = import(&mut mock);

    run_any(Command::Cancel(3), &config);

    let answer = run_now(command, &config);

    let Msg::OperationFinished {
      operation: 3,
      outcome: Ok(outcome),
    } = answer
    else {
      panic!("unexpected answer {answer:?}");
    };

    assert_eq!(outcome.changed, Vec::<String>::new());
    assert_eq!(
      outcome.cancelled,
      names(&["Owner-ModA-1.0.0", "Owner-ModD-1.0.0"])
    );
    assert!(
      !profile_folder(&config, "Shared")
        .join("BepInEx/config/Owner.ModA.cfg")
        .exists()
    );
  }

  #[test]
  fn an_import_onto_a_profile_that_exists_is_refused_and_changes_nothing() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = config_for(&dir, &mock);
    let command = import(&mut mock);

    run_now(
      Command::ChangeProfile {
        game: "valheim".to_string(),
        change: rivet_core::switcher::ProfileChange::Create("Shared".to_string()),
      },
      &config,
    );

    let answer = run_now(command, &config);

    assert!(
      matches!(&answer, Msg::OperationFinished { operation: 3, outcome: Err(reason) } if reason.contains("Shared")),
      "{answer:?}"
    );
    assert!(
      !profile_folder(&config, "Shared")
        .join("BepInEx/config/Owner.ModA.cfg")
        .exists()
    );
  }

  /// Runs `import` as operation 3 and returns its outcome.
  fn imported_outcome(import: Import, config: &Config) -> Outcome {
    let answer = run_now(Command::Import(import), config);

    let Msg::OperationFinished {
      operation: 3,
      outcome: Ok(outcome),
    } = answer
    else {
      panic!("unexpected answer {answer:?}");
    };

    outcome
  }

  /// A pin file holding `Owner-ModA` at 1.0.0.
  const PINS: &str = "[pins]\n\"Owner-ModA\" = \"1.0.0\"\n";

  #[test]
  fn an_import_restores_no_pins_whatever_its_archive_holds() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let config = config_for(&dir, &mock);
    let archive = zip(&[("export.r2x", R2MODMAN_LIST), (".rivet/pins.toml", PINS)]);

    let outcome = imported_outcome(
      Import {
        operation: 3,
        game: "valheim".to_string(),
        community: "valheim".to_string(),
        profile: "Shared".to_string(),
        packages: listed(),
        archive,
        index: mock_index(&mut mock),
      },
      &config,
    );

    assert!(!profile_folder(&config, "Shared").join(".rivet").exists());
    assert!(
      outcome
        .profile
        .unwrap()
        .values()
        .all(|package| !package.pinned)
    );
  }

  /// One entry of a `mods.yml` as r2modman writes it.
  fn r2modman_entry(name: &str, enabled: bool) -> String {
    format!(
      "- manifestVersion: 1
  name: {name}
  authorName: Owner
  websiteUrl: ''
  displayName: {name}
  description: ''
  gameVersion: '0'
  networkMode: both
  packageType: other
  installMode: managed
  installedAtTime: 1794578613520
  loaders: []
  dependencies: []
  incompatibilities: []
  optionalDependencies: []
  versionNumber:
    major: 1
    minor: 0
    patch: 0
  enabled: {enabled}
  onlineSource: true
"
    )
  }

  /// Writes r2modman's valheim profile `name` in its data folder `data`,
  /// holding `Owner-ModA` 1.0.0 enabled and `Owner-ModD` 1.0.0 disabled, with
  /// a config file and the files r2modman installed.
  fn r2modman_profile(data: &Path, name: &str) -> PathBuf {
    let folder = data.join("Valheim").join("profiles").join(name);
    let files = [
      (
        "mods.yml",
        r2modman_entry("Owner-ModA", true) + &r2modman_entry("Owner-ModD", false),
      ),
      (
        "BepInEx/config/Owner.ModA.cfg",
        "[General]\nSpeed = 2\n".to_string(),
      ),
      ("BepInEx/plugins/Owner-ModA/ModA.dll", "MZ".to_string()),
      ("BepInEx/plugins/Owner-ModD/ModD.dll.old", "MZ".to_string()),
    ];

    for (file, body) in files {
      let path = folder.join(file);

      std::fs::create_dir_all(path.parent().unwrap()).unwrap();
      std::fs::write(path, body).unwrap();
    }

    folder
  }

  /// Every file under `folder` with its bytes.
  fn snapshot(folder: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();

    for entry in std::fs::read_dir(folder).unwrap() {
      let path = entry.unwrap().path();

      if path.is_dir() {
        files.extend(snapshot(&path));
      } else {
        files.insert(path.clone(), std::fs::read(&path).unwrap());
      }
    }

    files
  }

  #[test]
  fn r2modmans_profiles_of_a_game_are_found_in_its_data_folders() {
    let dir = TempDir::new().unwrap();
    let data = dir.path().join("r2modmanPlus-local");
    let modded = r2modman_profile(&data, "Modded");
    let config = Config {
      r2modman: vec![dir.path().join("missing"), data],
      ..config_in(&dir)
    };

    let answer = run_now(
      Command::FindR2modmanProfiles {
        game: "valheim".to_string(),
        folders: vec!["Valheim".to_string()],
      },
      &config,
    );

    assert_eq!(
      answer,
      Msg::R2modmanFound {
        game: "valheim".to_string(),
        profiles: vec![Found {
          name: "Modded".to_string(),
          folder: modded,
        }],
      }
    );
  }

  #[test]
  fn an_r2modman_profile_imports_with_its_packages_and_configs_and_is_left_unchanged() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let data = dir.path().join("r2modmanPlus-local");
    let config = Config {
      r2modman: vec![data.clone()],
      ..config_for(&dir, &mock)
    };
    let source = r2modman_profile(&data, "Modded");
    let before = snapshot(&data);

    let shared = shared(run_now(read(Source::R2modman(source)), &config)).unwrap();

    assert_eq!(shared.name, "Modded");
    assert_eq!(shared.packages, listed());

    let answer = run_now(
      Command::Import(Import {
        operation: 3,
        game: "valheim".to_string(),
        community: "valheim".to_string(),
        profile: "Modded".to_string(),
        packages: shared.packages,
        archive: shared.archive,
        index: mock_index(&mut mock),
      }),
      &config,
    );

    let Msg::OperationFinished {
      operation: 3,
      outcome: Ok(outcome),
    } = answer
    else {
      panic!("unexpected answer {answer:?}");
    };

    assert_eq!(outcome.failed, vec![]);
    assert_eq!(
      outcome.profile.as_ref().map(packages),
      Ok(vec![
        ("Owner-ModA".to_string(), "1.0.0".to_string(), true),
        ("Owner-ModD".to_string(), "1.0.0".to_string(), false),
      ])
    );
    assert_eq!(
      std::fs::read_to_string(
        profile_folder(&config, "Modded").join("BepInEx/config/Owner.ModA.cfg")
      )
      .unwrap(),
      "[General]\nSpeed = 2\n"
    );
    assert_eq!(snapshot(&data), before);
  }

  #[test]
  fn an_r2modman_profile_brings_nothing_from_a_rivet_folder_in_it() {
    let dir = TempDir::new().unwrap();
    let mut mock = mock_thunderstore();
    let data = dir.path().join("r2modmanPlus-local");
    let config = config_for(&dir, &mock);
    let source = r2modman_profile(&data, "Modded");

    std::fs::create_dir_all(source.join(".rivet")).unwrap();
    std::fs::write(source.join(".rivet/pins.toml"), PINS).unwrap();
    std::fs::write(source.join(".rivet/notes.json"), "{}").unwrap();

    let shared = shared(run_now(read(Source::R2modman(source)), &config)).unwrap();
    let outcome = imported_outcome(
      Import {
        operation: 3,
        game: "valheim".to_string(),
        community: "valheim".to_string(),
        profile: "Modded".to_string(),
        packages: shared.packages,
        archive: shared.archive,
        index: mock_index(&mut mock),
      },
      &config,
    );

    assert!(!profile_folder(&config, "Modded").join(".rivet").exists());
    assert!(
      outcome
        .profile
        .unwrap()
        .values()
        .all(|package| !package.pinned)
    );
  }

  #[test]
  fn an_r2modman_profile_whose_mod_list_is_damaged_cannot_be_read() {
    let dir = TempDir::new().unwrap();
    let config = config_in(&dir);
    let source = r2modman_profile(dir.path(), "Broken");

    std::fs::write(source.join("mods.yml"), "{ not: [a mod list").unwrap();

    let answer = shared(run_now(read(Source::R2modman(source)), &config));

    assert!(
      matches!(answer, Err(ReadFailure::Unreadable(_))),
      "{answer:?}"
    );
  }
}
