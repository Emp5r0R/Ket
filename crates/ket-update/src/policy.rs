use std::str::FromStr;

use ket_core::{ClientUpdate, ClientUpdateStatus, UpdateAdvice};

use crate::{ClientIdentity, ReleaseManifest, UpdateCategory, UpdateError, Version};

/// What a server does with apps whose update has become required.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Enforcement {
    /// Refuse them (`426 Upgrade Required`).
    #[default]
    Enforce,
    /// Keep serving them with an urgent warning, for operators whose users
    /// cannot reach the download page.
    Warn,
}

impl FromStr for Enforcement {
    type Err = UpdateError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "enforce" => Ok(Self::Enforce),
            "warn" => Ok(Self::Warn),
            _ => Err(UpdateError::InvalidPolicy(
                "enforcement must be enforce or warn".to_owned(),
            )),
        }
    }
}

/// A server's rules for the apps that connect to it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ClientPolicy {
    server_version: Version,
    minimum: Option<Version>,
    enforcement: Enforcement,
    download_url: String,
}

impl ClientPolicy {
    /// `minimum` is an operator-chosen floor on top of the release manifest;
    /// it cannot be newer than the server itself.
    pub fn new(
        server_version: Version,
        minimum: Option<Version>,
        enforcement: Enforcement,
        download_url: impl Into<String>,
    ) -> Result<Self, UpdateError> {
        if minimum.is_some_and(|minimum| minimum > server_version) {
            return Err(UpdateError::InvalidPolicy(format!(
                "the minimum app version cannot be newer than this server ({server_version})"
            )));
        }
        Ok(Self {
            server_version,
            minimum,
            enforcement,
            download_url: download_url.into(),
        })
    }

    pub fn server_version(&self) -> Version {
        self.server_version
    }

    pub fn advice(
        &self,
        manifest: &ReleaseManifest,
        client: ClientIdentity,
        now_epoch_seconds: u64,
    ) -> UpdateAdvice {
        UpdateAdvice {
            server_version: self.server_version.to_string(),
            client: self.evaluate(manifest, client, now_epoch_seconds),
        }
    }

    /// Rates `client` against every release after its version, up to this
    /// server's. A critical release, an overdue security release, or the
    /// operator's minimum makes the update required.
    pub fn evaluate(
        &self,
        manifest: &ReleaseManifest,
        client: ClientIdentity,
        now_epoch_seconds: u64,
    ) -> ClientUpdate {
        let mut status = ClientUpdateStatus::Current;
        let mut summary = None;
        let mut required_after = None;
        if let Some(minimum) = self.minimum.filter(|minimum| client.version < *minimum) {
            status = ClientUpdateStatus::Required;
            summary = Some(format!("This server requires Ket {minimum} or newer."));
        } else {
            let grace = manifest.security_grace_seconds();
            let newer = manifest.releases.iter().filter(|release| {
                release.version > client.version && release.version <= self.server_version
            });
            for release in newer {
                let Some(category) = release.client_rating(client.platform) else {
                    continue;
                };
                let rated = match category {
                    UpdateCategory::Optional => ClientUpdateStatus::Optional,
                    UpdateCategory::Recommended => ClientUpdateStatus::Recommended,
                    UpdateCategory::Critical => ClientUpdateStatus::Required,
                    UpdateCategory::Security => {
                        let deadline = release.published_epoch_seconds().saturating_add(grace);
                        if now_epoch_seconds >= deadline {
                            ClientUpdateStatus::Required
                        } else {
                            required_after =
                                Some(required_after.map_or(deadline, |due: u64| due.min(deadline)));
                            ClientUpdateStatus::Security
                        }
                    }
                };
                // Releases are oldest first, so ties keep the newest summary.
                if rated >= status {
                    status = rated;
                    summary = Some(release.summary.clone());
                }
            }
        }
        if status == ClientUpdateStatus::Required && self.enforcement == Enforcement::Warn {
            status = ClientUpdateStatus::Security;
            required_after = None;
        }
        if status != ClientUpdateStatus::Security {
            required_after = None;
        }
        ClientUpdate {
            status,
            latest_version: self.server_version.to_string(),
            required_after_epoch_seconds: required_after,
            summary,
            download_url: self.download_url.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ClientPlatform, manifest::tests::manifest};

    const DAY: u64 = 86_400;
    /// 2026-03-01, when the security release below was published.
    const SECURITY_PUBLISHED: u64 = 20_513 * DAY;

    fn v(value: &str) -> Version {
        value.parse().unwrap()
    }

    fn releases() -> ReleaseManifest {
        manifest(
            r#"{"version":"1.0.1","published":"2026-01-01","client":"optional","summary":"base"},
               {"version":"1.1.0","published":"2026-02-01","client":"recommended","summary":"fixes"},
               {"version":"1.2.0","published":"2026-03-01","client":"security","summary":"security fix"},
               {"version":"1.3.0","published":"2026-04-01","client":"optional","summary":"features"},
               {"version":"1.4.0","published":"2026-05-01","client":"critical","apps":{"android":"critical","linux":"optional"},"summary":"android critical"},
               {"version":"1.5.0","published":"2026-06-01","server":"critical","summary":"server only"}"#,
        )
    }

    fn policy(server: &str) -> ClientPolicy {
        ClientPolicy::new(
            v(server),
            None,
            Enforcement::Enforce,
            "https://example.test/ket",
        )
        .unwrap()
    }

    fn linux(version: &str) -> ClientIdentity {
        ClientIdentity::new(ClientPlatform::Linux, v(version))
    }

    #[test]
    fn current_apps_and_apps_newer_than_the_server_need_nothing() {
        let manifest = releases();
        for client in [linux("1.5.0"), linux("1.3.0"), linux("2.0.0")] {
            let update = policy("1.3.0").evaluate(&manifest, client, SECURITY_PUBLISHED);
            assert_eq!(update.status, ClientUpdateStatus::Current, "{client:?}");
            assert_eq!(update.summary, None);
            assert_eq!(update.latest_version, "1.3.0");
        }
    }

    #[test]
    fn optional_and_recommended_releases_are_rated_as_published() {
        let manifest = releases();
        let update = policy("1.1.0").evaluate(&manifest, linux("1.0.1"), 0);
        assert_eq!(update.status, ClientUpdateStatus::Recommended);
        assert_eq!(update.summary.as_deref(), Some("fixes"));
        let update = policy("1.3.0").evaluate(&manifest, linux("1.2.0"), 0);
        assert_eq!(update.status, ClientUpdateStatus::Optional);
        assert_eq!(update.download_url, "https://example.test/ket");
    }

    #[test]
    fn security_releases_become_required_after_the_grace_period() {
        let manifest = releases();
        let deadline = SECURITY_PUBLISHED + 14 * DAY;
        let update = policy("1.3.0").evaluate(&manifest, linux("1.1.0"), deadline - 1);
        assert_eq!(update.status, ClientUpdateStatus::Security);
        assert_eq!(update.required_after_epoch_seconds, Some(deadline));
        assert_eq!(update.summary.as_deref(), Some("security fix"));
        let update = policy("1.3.0").evaluate(&manifest, linux("1.1.0"), deadline);
        assert_eq!(update.status, ClientUpdateStatus::Required);
        assert_eq!(update.required_after_epoch_seconds, None);
    }

    #[test]
    fn per_app_ratings_apply_only_to_their_platforms() {
        let manifest = releases();
        let android = ClientIdentity::new(ClientPlatform::Android, v("1.3.0"));
        assert_eq!(
            policy("1.5.0").evaluate(&manifest, android, 0).status,
            ClientUpdateStatus::Required
        );
        assert_eq!(
            policy("1.5.0")
                .evaluate(&manifest, linux("1.3.0"), 0)
                .status,
            ClientUpdateStatus::Optional
        );
        let windows = ClientIdentity::new(ClientPlatform::Windows, v("1.3.0"));
        assert_eq!(
            policy("1.5.0").evaluate(&manifest, windows, 0).status,
            ClientUpdateStatus::Current
        );
        // Legacy apps do not say which platform they are, so every rating counts.
        assert_eq!(
            policy("1.5.0")
                .evaluate(&manifest, ClientIdentity::legacy(), 0)
                .status,
            ClientUpdateStatus::Required
        );
    }

    #[test]
    fn the_operator_minimum_overrides_the_manifest() {
        let manifest = releases();
        let policy = ClientPolicy::new(
            v("1.3.0"),
            Some(v("1.3.0")),
            Enforcement::Enforce,
            "https://example.test/ket",
        )
        .unwrap();
        let update = policy.evaluate(&manifest, linux("1.2.0"), 0);
        assert_eq!(update.status, ClientUpdateStatus::Required);
        assert_eq!(
            update.summary.as_deref(),
            Some("This server requires Ket 1.3.0 or newer.")
        );
        assert!(
            ClientPolicy::new(v("1.3.0"), Some(v("1.4.0")), Enforcement::Enforce, "x").is_err()
        );
    }

    #[test]
    fn warn_mode_downgrades_required_to_an_undated_security_warning() {
        let manifest = releases();
        let policy = ClientPolicy::new(
            v("1.5.0"),
            None,
            Enforcement::Warn,
            "https://example.test/ket",
        )
        .unwrap();
        let android = ClientIdentity::new(ClientPlatform::Android, v("1.3.0"));
        let update = policy.evaluate(&manifest, android, 0);
        assert_eq!(update.status, ClientUpdateStatus::Security);
        assert_eq!(update.required_after_epoch_seconds, None);
        assert_eq!("warn".parse::<Enforcement>(), Ok(Enforcement::Warn));
        assert!("off".parse::<Enforcement>().is_err());
    }

    #[test]
    fn advice_names_the_server_version() {
        let advice = policy("1.3.0").advice(&releases(), linux("1.3.0"), 0);
        assert_eq!(advice.server_version, "1.3.0");
        assert_eq!(advice.client.status, ClientUpdateStatus::Current);
    }
}
