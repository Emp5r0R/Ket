//! Replays the shared server-update vectors that the server's `ket` command
//! also replays with jq, so both implementations rate updates the same way.

use ket_update::{ReleaseManifest, UpdateCategory, Version};
use serde::Deserialize;

#[derive(Deserialize)]
struct Vectors {
    manifest: serde_json::Value,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    installed: Version,
    target: Version,
    category: Option<UpdateCategory>,
}

#[test]
fn server_update_vectors() {
    let vectors: Vectors =
        serde_json::from_str(include_str!("vectors/server-updates.json")).unwrap();
    let manifest = ReleaseManifest::parse(&vectors.manifest.to_string()).unwrap();
    assert!(!vectors.cases.is_empty());
    for case in vectors.cases {
        assert_eq!(
            manifest.server_update_category(case.installed, case.target),
            case.category,
            "{} -> {}",
            case.installed,
            case.target
        );
    }
}
