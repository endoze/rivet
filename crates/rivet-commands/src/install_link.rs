//! Asking Thunderstore about an install link's package, and registering
//! Rivet as the app that opens install links.

use crate::{Config, USER_AGENT};
use rivet_core::install_link::{InstallLink, ListingFailure};
use serde::Deserialize;

/// How long asking about a package may take before it is given up.
const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// The part of Thunderstore's experimental package API answer read here.
#[derive(Deserialize)]
struct Package {
  community_listings: Vec<Listing>,
}

#[derive(Deserialize)]
struct Listing {
  community: String,
  review_status: String,
}

/// The communities that list `link`'s package, leaving out listings
/// Thunderstore rejected, which it hides from the community.
pub(crate) async fn find_listings(
  config: &Config,
  link: &InstallLink,
) -> Result<Vec<String>, ListingFailure> {
  let unreachable = |error: reqwest::Error| ListingFailure::Unreachable(error.to_string());
  let url = format!(
    "{}/api/experimental/package/{}/{}/",
    config.thunderstore.trim_end_matches('/'),
    link.owner,
    link.name
  );
  let client = reqwest::Client::builder()
    .user_agent(USER_AGENT)
    .timeout(TIMEOUT)
    .build()
    .map_err(unreachable)?;
  let response = client.get(url).send().await.map_err(unreachable)?;

  if response.status() == reqwest::StatusCode::NOT_FOUND {
    return Err(ListingFailure::Unknown);
  }

  let package: Package = response
    .error_for_status()
    .map_err(unreachable)?
    .json()
    .await
    .map_err(unreachable)?;

  Ok(
    package
      .community_listings
      .into_iter()
      .filter(|listing| listing.review_status != "rejected")
      .map(|listing| listing.community)
      .collect(),
  )
}

#[cfg(test)]
mod tests {
  use crate::tests::{config_for, run_now};
  use rivet_core::install_link::{InstallLink, ListingFailure};
  use rivet_core::{Command, Msg};
  use tempfile::TempDir;
  use thunderstore_engine::test_support::MockThunderstore;

  fn link() -> InstallLink {
    InstallLink::parse("ror2mm://v1/install/thunderstore.io/BepInEx/BepInExPack/5.4.2305/").unwrap()
  }

  const PACKAGE: &str = "/api/experimental/package/BepInEx/BepInExPack/";

  fn find(mock: &MockThunderstore) -> Result<Vec<String>, ListingFailure> {
    let dir = TempDir::new().unwrap();

    match run_now(Command::FindListings(link()), &config_for(&dir, mock)) {
      Msg::ListingsFound {
        link: found,
        communities,
      } if found == link() => communities,
      other => panic!("answered {other:?}"),
    }
  }

  #[test]
  fn the_communities_listing_a_package_leave_out_rejected_listings() {
    let mut mock = MockThunderstore::start();

    mock
      .server_mut()
      .mock("GET", PACKAGE)
      .with_status(200)
      .with_header("Content-Type", "application/json")
      .with_body(
        r#"{
          "namespace": "BepInEx",
          "name": "BepInExPack",
          "full_name": "BepInEx-BepInExPack",
          "community_listings": [
            { "has_nsfw_content": false, "categories": ["Libraries"], "community": "lethal-company", "review_status": "approved" },
            { "has_nsfw_content": false, "categories": [], "community": "valheim", "review_status": "unreviewed" },
            { "has_nsfw_content": false, "categories": [], "community": "riskofrain2", "review_status": "rejected" }
          ],
          "latest": { "version_number": "5.4.2305" }
        }"#,
      )
      .create();

    assert_eq!(
      find(&mock),
      Ok(vec!["lethal-company".to_string(), "valheim".to_string()])
    );
  }

  #[test]
  fn a_package_thunderstore_does_not_have_is_unknown() {
    let mut mock = MockThunderstore::start();

    mock
      .server_mut()
      .mock("GET", PACKAGE)
      .with_status(404)
      .with_body(r#"{"detail":"No Package matches the given query."}"#)
      .create();

    assert_eq!(find(&mock), Err(ListingFailure::Unknown));
  }

  #[test]
  fn a_failing_thunderstore_is_unreachable() {
    let mut mock = MockThunderstore::start();

    mock
      .server_mut()
      .mock("GET", PACKAGE)
      .with_status(503)
      .create();

    assert!(
      matches!(find(&mock), Err(ListingFailure::Unreachable(_))),
      "{:?}",
      find(&mock)
    );
  }
}
