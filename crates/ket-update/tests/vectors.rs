//! Replays the shared update vectors that the server's `ket` command also
//! replays with jq, so both implementations rate updates the same way.

use ket_update::{ClientPlatform, ReleaseManifest, UpdateCategory, Version};
use serde::Deserialize;

#[derive(Deserialize)]
struct Vectors<C> {
    manifest: serde_json::Value,
    cases: Vec<C>,
}

#[derive(Deserialize)]
struct ServerCase {
    installed: Version,
    target: Version,
    category: Option<UpdateCategory>,
}

#[derive(Deserialize)]
struct AppCase {
    installed: Version,
    target: Version,
    platform: ClientPlatform,
    category: Option<UpdateCategory>,
}

fn load<C: serde::de::DeserializeOwned>(json: &str) -> (ReleaseManifest, Vec<C>) {
    let vectors: Vectors<C> = serde_json::from_str(json).unwrap();
    let manifest = ReleaseManifest::parse(&vectors.manifest.to_string()).unwrap();
    assert!(!vectors.cases.is_empty());
    (manifest, vectors.cases)
}

#[test]
fn server_update_vectors() {
    let (manifest, cases) = load::<ServerCase>(include_str!("vectors/server-updates.json"));
    for case in cases {
        assert_eq!(
            manifest.server_update_category(case.installed, case.target),
            case.category,
            "{} -> {}",
            case.installed,
            case.target
        );
    }
}

#[test]
fn app_update_vectors() {
    let (manifest, cases) = load::<AppCase>(include_str!("vectors/app-updates.json"));
    for case in cases {
        assert_eq!(
            manifest.app_update_category(case.installed, case.target, case.platform),
            case.category,
            "{} -> {} on {}",
            case.installed,
            case.target,
            case.platform
        );
    }
}
