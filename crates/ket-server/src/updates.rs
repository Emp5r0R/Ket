//! The server's side of the update lifecycle: which app versions it accepts
//! and the update advice it attaches to session responses.

use std::{env, sync::Arc};

use anyhow::{Context, Result, bail};
use axum::http::HeaderMap;
use ket_core::{CLIENT_HEADER, ClientUpdateStatus, UpdateAdvice};
use ket_update::{
    ClientIdentity, ClientPolicy, DEFAULT_DOWNLOAD_URL, Enforcement, ReleaseManifest, Version,
};
use url::Url;

const MAX_DOWNLOAD_URL_CHARS: usize = 2_048;

#[derive(Clone, Debug)]
pub struct ClientUpdates {
    policy: ClientPolicy,
    manifest: Arc<ReleaseManifest>,
}

impl ClientUpdates {
    pub fn new(policy: ClientPolicy, manifest: Arc<ReleaseManifest>) -> Self {
        Self { policy, manifest }
    }

    /// Reads the operator's settings:
    /// - `KET_CLIENT_MINIMUM_VERSION`: refuse apps older than this version.
    /// - `KET_CLIENT_UPDATE_ENFORCEMENT`: `enforce` (default) refuses apps
    ///   whose update is required; `warn` keeps serving them with a warning.
    /// - `KET_CLIENT_DOWNLOAD_URL`: an HTTPS page where people get the apps,
    ///   for operators who mirror the releases.
    pub fn from_env() -> Result<Self> {
        let minimum = optional("KET_CLIENT_MINIMUM_VERSION")
            .map(|value| value.parse::<Version>())
            .transpose()
            .context("KET_CLIENT_MINIMUM_VERSION must be a MAJOR.MINOR.PATCH version")?;
        let enforcement = optional("KET_CLIENT_UPDATE_ENFORCEMENT")
            .map(|value| value.parse::<Enforcement>())
            .transpose()
            .context("KET_CLIENT_UPDATE_ENFORCEMENT must be enforce or warn")?
            .unwrap_or_default();
        let download_url = match optional("KET_CLIENT_DOWNLOAD_URL") {
            Some(value) => validate_download_url(&value)?,
            None => DEFAULT_DOWNLOAD_URL.to_owned(),
        };
        let policy = ClientPolicy::new(server_version(), minimum, enforcement, download_url)
            .context("KET_CLIENT_MINIMUM_VERSION cannot be newer than this server")?;
        Ok(Self::new(
            policy,
            Arc::new(ReleaseManifest::embedded().clone()),
        ))
    }

    /// The advice for the app that sent `headers`.
    pub fn advice(&self, headers: &HeaderMap, now_epoch_seconds: u64) -> UpdateAdvice {
        let identity = ClientIdentity::from_header(
            headers
                .get(CLIENT_HEADER)
                .and_then(|value| value.to_str().ok()),
        );
        self.policy
            .advice(&self.manifest, identity, now_epoch_seconds)
    }
}

impl Default for ClientUpdates {
    /// The release manifest's policy with no operator settings.
    fn default() -> Self {
        let policy = ClientPolicy::new(
            server_version(),
            None,
            Enforcement::Enforce,
            DEFAULT_DOWNLOAD_URL,
        )
        .expect("a policy without a minimum is valid");
        Self::new(policy, Arc::new(ReleaseManifest::embedded().clone()))
    }
}

/// True when the advice says the app may no longer use this server.
pub fn is_required(advice: &UpdateAdvice) -> bool {
    advice.client.status == ClientUpdateStatus::Required
}

/// This build's version; the release manifest's tests keep it in step with
/// the newest manifest entry.
pub fn server_version() -> Version {
    env!("CARGO_PKG_VERSION")
        .parse()
        .expect("the workspace version is MAJOR.MINOR.PATCH")
}

fn optional(name: &str) -> Option<String> {
    env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn validate_download_url(value: &str) -> Result<String> {
    if value.len() > MAX_DOWNLOAD_URL_CHARS {
        bail!("KET_CLIENT_DOWNLOAD_URL is too long");
    }
    let url = Url::parse(value).context("KET_CLIENT_DOWNLOAD_URL must be an absolute URL")?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        bail!("KET_CLIENT_DOWNLOAD_URL must be an https:// URL without credentials or a fragment");
    }
    Ok(url.into())
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    fn updates(minimum: Option<&str>, enforcement: Enforcement) -> ClientUpdates {
        let manifest = ReleaseManifest::parse(
            r#"{"schema":1,"security_grace_days":14,"releases":[
                {"version":"1.0.1","published":"2026-09-02","client":"critical","summary":"Old apps must update."},
                {"version":"1.1.0","published":"2026-10-01","client":"optional","summary":"New features."}]}"#,
        )
        .unwrap();
        let policy = ClientPolicy::new(
            "1.1.0".parse().unwrap(),
            minimum.map(|value| value.parse().unwrap()),
            enforcement,
            DEFAULT_DOWNLOAD_URL,
        )
        .unwrap();
        ClientUpdates::new(policy, Arc::new(manifest))
    }

    fn headers(value: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(value) = value {
            headers.insert(CLIENT_HEADER, HeaderValue::from_str(value).unwrap());
        }
        headers
    }

    #[test]
    fn rates_the_app_named_in_the_header() {
        let updates = updates(None, Enforcement::Enforce);
        let advice = updates.advice(&headers(Some("linux/1.0.1")), 0);
        assert_eq!(advice.server_version, "1.1.0");
        assert_eq!(advice.client.status, ClientUpdateStatus::Optional);
        assert!(!is_required(&advice));
        // Without the header the app is v1.0.1 at most, on an unknown platform.
        let advice = updates.advice(&headers(None), 0);
        assert_eq!(advice.client.status, ClientUpdateStatus::Optional);
        let advice = updates.advice(&headers(Some("windows/1.0.0")), 0);
        assert!(is_required(&advice));
        assert_eq!(
            advice.client.summary.as_deref(),
            Some("Old apps must update.")
        );
    }

    #[test]
    fn operator_settings_apply() {
        let advice =
            updates(Some("1.1.0"), Enforcement::Enforce).advice(&headers(Some("android/1.0.1")), 0);
        assert!(is_required(&advice));
        let advice = updates(None, Enforcement::Warn).advice(&headers(Some("android/1.0.0")), 0);
        assert_eq!(advice.client.status, ClientUpdateStatus::Security);
    }

    #[test]
    fn download_urls_must_be_plain_https() {
        assert_eq!(
            validate_download_url("https://mirror.example.com/ket/").unwrap(),
            "https://mirror.example.com/ket/"
        );
        for invalid in [
            "http://mirror.example.com/",
            "https://user:pass@mirror.example.com/",
            "https://mirror.example.com/#top",
            "mirror.example.com",
            "javascript:alert(1)",
        ] {
            assert!(validate_download_url(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn the_default_policy_uses_this_build_and_the_release_page() {
        let advice = ClientUpdates::default()
            .advice(&headers(Some(&format!("linux/{}", server_version()))), 0);
        assert_eq!(advice.server_version, server_version().to_string());
        assert_eq!(advice.client.status, ClientUpdateStatus::Current);
        assert_eq!(advice.client.download_url, DEFAULT_DOWNLOAD_URL);
    }
}
