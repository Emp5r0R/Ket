//! What the diagnostic report says about this computer: the operating
//! system and the kind of network it is on, never which network.

use ket_client_core::NetworkKind;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct DiagnosticContext {
    pub network: NetworkKind,
    /// The operating system's name and version and the processor family,
    /// such as `Ubuntu 24.04 x86_64`.
    pub system: String,
}

pub(crate) fn context() -> DiagnosticContext {
    DiagnosticContext {
        network: ket_client_core::network_kind(),
        system: system_description(),
    }
}

fn system_description() -> String {
    let name = os_release_name().unwrap_or_else(|| std::env::consts::OS.to_owned());
    format!("{name} {}", std::env::consts::ARCH)
}

#[cfg(target_os = "linux")]
fn os_release_name() -> Option<String> {
    let release = std::fs::read_to_string("/etc/os-release").ok()?;
    os_release_field(&release)
}

#[cfg(not(target_os = "linux"))]
fn os_release_name() -> Option<String> {
    None
}

/// `NAME VERSION_ID` from an os-release file, limited to plain characters.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn os_release_field(release: &str) -> Option<String> {
    let value = |key: &str| {
        release.lines().find_map(|line| {
            line.strip_prefix(key)
                .and_then(|rest| rest.strip_prefix('='))
                .map(|value| value.trim().trim_matches('"').to_owned())
        })
    };
    let name = value("NAME")?;
    let described = match value("VERSION_ID") {
        Some(version) => format!("{name} {version}"),
        None => name,
    };
    let plain = described
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || " ._-/()".contains(*character))
        .take(48)
        .collect::<String>();
    (!plain.trim().is_empty()).then(|| plain.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn os_release_names_the_distribution_plainly() {
        let release = "PRETTY_NAME=\"Ubuntu 24.04.1 LTS\"\nNAME=\"Ubuntu\"\nVERSION_ID=\"24.04\"\n";
        assert_eq!(os_release_field(release).as_deref(), Some("Ubuntu 24.04"));
        assert_eq!(
            os_release_field("NAME=Arch Linux\n").as_deref(),
            Some("Arch Linux")
        );
        assert_eq!(
            os_release_field("NAME=\"$(evil)\u{1b}[31m\"\n").as_deref(),
            Some("(evil)31m")
        );
        assert_eq!(os_release_field("ID=debian\n"), None);
    }
}
