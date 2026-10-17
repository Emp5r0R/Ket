use std::{collections::BTreeMap, sync::OnceLock};

use serde::Deserialize;

use crate::{ClientPlatform, UpdateCategory, UpdateError, Version};

const SCHEMA: u32 = 1;
const MAX_SUMMARY_CHARS: usize = 200;
const SECONDS_PER_DAY: u64 = 86_400;

/// The manifest shipped with this build. Every release adds its entry here,
/// so a server always knows every release up to its own version.
const EMBEDDED: &str = include_str!("../releases.json");

/// One Ket release and how urgent it is for apps and for servers.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Release {
    pub version: Version,
    /// Release day, `YYYY-MM-DD` (UTC).
    published: String,
    /// Urgency for apps; `None` when the release changes nothing in the apps.
    /// With `apps`, the most urgent of those ratings: servers, and `ket`
    /// commands that predate `apps`, rate their own update from this field.
    #[serde(default)]
    pub client: Option<UpdateCategory>,
    /// Urgency for servers; `None` when the release changes nothing on servers.
    #[serde(default)]
    pub server: Option<UpdateCategory>,
    /// Urgency for each app, when the apps are not all rated alike. Apps not
    /// listed are unchanged by the release. Empty means `client` applies to
    /// every app.
    #[serde(default)]
    pub apps: BTreeMap<ClientPlatform, UpdateCategory>,
    pub summary: String,
    #[serde(skip)]
    published_epoch_seconds: u64,
}

impl Release {
    pub fn published_epoch_seconds(&self) -> u64 {
        self.published_epoch_seconds
    }

    /// How urgent this release is for an app on `platform`. Apps that do not
    /// name their platform get `client`, the most urgent app rating.
    pub fn client_rating(&self, platform: Option<ClientPlatform>) -> Option<UpdateCategory> {
        match platform {
            Some(platform) if !self.apps.is_empty() => self.apps.get(&platform).copied(),
            _ => self.client,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ReleaseManifest {
    schema: u32,
    /// Days after a security release's publication before apps must update.
    pub security_grace_days: u32,
    /// Oldest first.
    pub releases: Vec<Release>,
}

impl ReleaseManifest {
    pub fn parse(json: &str) -> Result<Self, UpdateError> {
        let mut manifest: Self = serde_json::from_str(json)
            .map_err(|error| UpdateError::InvalidManifest(error.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// The manifest compiled into this build. Its validity is checked by the
    /// crate's tests, so a malformed manifest never ships.
    pub fn embedded() -> &'static Self {
        static MANIFEST: OnceLock<ReleaseManifest> = OnceLock::new();
        MANIFEST.get_or_init(|| {
            Self::parse(EMBEDDED).expect("the embedded release manifest is validated by tests")
        })
    }

    pub fn latest(&self) -> Option<&Release> {
        self.releases.last()
    }

    pub fn security_grace_seconds(&self) -> u64 {
        u64::from(self.security_grace_days) * SECONDS_PER_DAY
    }

    /// How urgently a server at `installed` should move to `target`: the
    /// most urgent rating, for servers or for apps, among the releases after
    /// `installed` up to and including `target`. App ratings count because
    /// apps only learn about a fix from servers that know the release.
    /// `None` when no release in the range is rated.
    pub fn server_update_category(
        &self,
        installed: Version,
        target: Version,
    ) -> Option<UpdateCategory> {
        self.releases
            .iter()
            .filter(|release| release.version > installed && release.version <= target)
            .flat_map(|release| [release.server, release.client])
            .flatten()
            .max()
    }

    /// What apps on `platform` are told once a server moves from `installed`
    /// to `target`: the most urgent app rating for that platform among the
    /// releases in between. `None` when the update changes nothing there.
    pub fn app_update_category(
        &self,
        installed: Version,
        target: Version,
        platform: ClientPlatform,
    ) -> Option<UpdateCategory> {
        self.releases
            .iter()
            .filter(|release| release.version > installed && release.version <= target)
            .filter_map(|release| release.client_rating(Some(platform)))
            .max()
    }

    fn validate(&mut self) -> Result<(), UpdateError> {
        let invalid = |message: String| Err(UpdateError::InvalidManifest(message));
        if self.schema != SCHEMA {
            return invalid(format!("unsupported schema {}", self.schema));
        }
        if !(1..=365).contains(&self.security_grace_days) {
            return invalid("security_grace_days must be 1-365".to_owned());
        }
        if self.releases.is_empty() {
            return invalid("there are no releases".to_owned());
        }
        for index in 0..self.releases.len() {
            if index > 0 && self.releases[index - 1].version >= self.releases[index].version {
                return invalid(format!(
                    "{} is not newer than the release before it",
                    self.releases[index].version
                ));
            }
            let release = &mut self.releases[index];
            release.published_epoch_seconds = parse_date(&release.published).ok_or_else(|| {
                UpdateError::InvalidManifest(format!(
                    "{} has an invalid published date",
                    release.version
                ))
            })? * SECONDS_PER_DAY;
            let summary = release.summary.as_str();
            if summary.trim().is_empty()
                || summary != summary.trim()
                || summary.chars().count() > MAX_SUMMARY_CHARS
                || summary.chars().any(char::is_control)
            {
                return invalid(format!("{} has an invalid summary", release.version));
            }
            if !release.apps.is_empty() && release.client != release.apps.values().max().copied() {
                return invalid(format!(
                    "{} needs a client rating equal to its most urgent app rating",
                    release.version
                ));
            }
        }
        Ok(())
    }
}

/// Days since 1970-01-01 for a strict `YYYY-MM-DD` date.
fn parse_date(value: &str) -> Option<u64> {
    let bytes = value.as_bytes();
    if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let number = |range: std::ops::Range<usize>| -> Option<u32> {
        let part = &value[range];
        part.bytes()
            .all(|byte| byte.is_ascii_digit())
            .then(|| part.parse().ok())
            .flatten()
    };
    let (year, month, day) = (number(0..4)?, number(5..7)?, number(8..10)?);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if year < 1970 || day == 0 || day > month_days {
        return None;
    }
    // Howard Hinnant's days_from_civil.
    let year = i64::from(year) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i64::from(month);
    let day_of_year =
        (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    u64::try_from(era * 146_097 + day_of_era - 719_468).ok()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn manifest(releases: &str) -> ReleaseManifest {
        ReleaseManifest::parse(&format!(
            r#"{{"schema":1,"security_grace_days":14,"releases":[{releases}]}}"#
        ))
        .unwrap()
    }

    #[test]
    fn embedded_manifest_is_valid_and_ends_at_this_build() {
        let manifest = ReleaseManifest::embedded();
        let latest = manifest.latest().unwrap();
        assert_eq!(
            latest.version,
            env!("CARGO_PKG_VERSION").parse::<Version>().unwrap(),
            "add a releases.json entry for this version"
        );
    }

    #[test]
    fn dates_convert_to_unix_days() {
        assert_eq!(parse_date("1970-01-01"), Some(0));
        assert_eq!(parse_date("2000-03-01"), Some(11_017));
        assert_eq!(parse_date("2024-02-29"), Some(19_782));
        assert_eq!(parse_date("2026-09-02"), Some(20_698));
        for invalid in [
            "2026-02-29",
            "2026-13-01",
            "2026-00-10",
            "2026-04-31",
            "1969-12-31",
            "2026-9-02",
            "2026/09/02",
            "+026-09-02",
        ] {
            assert_eq!(parse_date(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn server_updates_take_the_most_urgent_server_or_app_rating_in_range() {
        let manifest = manifest(
            r#"{"version":"1.0.0","published":"2026-01-01","server":"critical","summary":"a"},
               {"version":"1.1.0","published":"2026-02-01","server":"recommended","summary":"b"},
               {"version":"1.2.0","published":"2026-03-01","client":"security","summary":"c"},
               {"version":"1.3.0","published":"2026-04-01","server":"security","summary":"d"}"#,
        );
        let v = |value: &str| value.parse::<Version>().unwrap();
        assert_eq!(
            manifest.server_update_category(v("1.0.0"), v("1.3.0")),
            Some(UpdateCategory::Security)
        );
        assert_eq!(
            manifest.server_update_category(v("1.0.0"), v("1.1.0")),
            Some(UpdateCategory::Recommended)
        );
        // An app-only security fix still makes the server update urgent.
        assert_eq!(
            manifest.server_update_category(v("1.1.0"), v("1.2.0")),
            Some(UpdateCategory::Security)
        );
        assert_eq!(
            manifest.server_update_category(v("1.3.0"), v("1.3.0")),
            None
        );
        assert_eq!(
            manifest.server_update_category(v("0.9.0"), v("1.0.0")),
            Some(UpdateCategory::Critical)
        );
    }

    #[test]
    fn apps_are_rated_per_platform_and_unnamed_apps_get_the_most_urgent_rating() {
        let manifest = manifest(
            r#"{"version":"1.0.0","published":"2026-01-01","client":"security","apps":{"android":"optional","windows":"security"},"summary":"a"},
               {"version":"1.1.0","published":"2026-02-01","client":"recommended","summary":"b"}"#,
        );
        let release = &manifest.releases[0];
        assert_eq!(
            release.client_rating(Some(ClientPlatform::Android)),
            Some(UpdateCategory::Optional)
        );
        assert_eq!(
            release.client_rating(Some(ClientPlatform::Windows)),
            Some(UpdateCategory::Security)
        );
        assert_eq!(release.client_rating(Some(ClientPlatform::Linux)), None);
        assert_eq!(release.client_rating(None), Some(UpdateCategory::Security));
        let rated = &manifest.releases[1];
        assert_eq!(
            rated.client_rating(Some(ClientPlatform::Linux)),
            Some(UpdateCategory::Recommended)
        );
        let v = |value: &str| value.parse::<Version>().unwrap();
        assert_eq!(
            manifest.app_update_category(v("0.9.0"), v("1.1.0"), ClientPlatform::Android),
            Some(UpdateCategory::Recommended)
        );
        assert_eq!(
            manifest.app_update_category(v("0.9.0"), v("1.0.0"), ClientPlatform::Linux),
            None
        );
    }

    #[test]
    fn rejects_malformed_manifests() {
        let entry = |fields: &str| {
            format!(r#"{{"schema":1,"security_grace_days":14,"releases":[{fields}]}}"#)
        };
        let cases = [
            r#"{"schema":2,"security_grace_days":14,"releases":[{"version":"1.0.0","published":"2026-01-01","summary":"a"}]}"#.to_owned(),
            r#"{"schema":1,"security_grace_days":0,"releases":[{"version":"1.0.0","published":"2026-01-01","summary":"a"}]}"#.to_owned(),
            r#"{"schema":1,"security_grace_days":14,"releases":[]}"#.to_owned(),
            entry(r#"{"version":"1.1.0","published":"2026-01-01","summary":"a"},{"version":"1.0.0","published":"2026-01-01","summary":"b"}"#),
            entry(r#"{"version":"1.0.0","published":"2026-01-01","summary":"a"},{"version":"1.0.0","published":"2026-01-02","summary":"b"}"#),
            entry(r#"{"version":"1.0.0","published":"2026-02-30","summary":"a"}"#),
            entry(r#"{"version":"1.0.0","published":"2026-01-01","summary":" padded"}"#),
            entry(r#"{"version":"1.0.0","published":"2026-01-01","summary":""}"#),
            entry(r#"{"version":"1.0.0","published":"2026-01-01","summary":"a","client":"urgent"}"#),
            entry(r#"{"version":"1.0.0","published":"2026-01-01","summary":"a","extra":true}"#),
            entry(r#"{"version":"1.0.0","published":"2026-01-01","summary":"a","apps":{"linux":"optional"}}"#),
            entry(r#"{"version":"1.0.0","published":"2026-01-01","summary":"a","client":"optional","apps":{"linux":"security"}}"#),
            entry(r#"{"version":"1.0.0","published":"2026-01-01","summary":"a","client":"critical","apps":{"linux":"security"}}"#),
            entry(r#"{"version":"1.0.0","published":"2026-01-01","summary":"a","client":"optional","apps":{"ios":"optional"}}"#),
            entry(r#"{"version":"1.0.0","published":"2026-01-01","summary":"a","client":"optional","platforms":["linux"]}"#),
            entry(&format!(
                r#"{{"version":"1.0.0","published":"2026-01-01","summary":"{}"}}"#,
                "x".repeat(201)
            )),
        ];
        for case in cases {
            assert!(ReleaseManifest::parse(&case).is_err(), "{case}");
        }
    }
}
