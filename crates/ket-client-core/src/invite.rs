//! Turns invite text into a pinned enrollment target.

use ket_core::SecretString;
use ket_invite::{CONTROL_API_VERSION, InviteError, decode_invite_text};

use crate::{ClientError, ControlEndpoint};

/// Everything a client needs to enroll from an invite. The endpoint carries
/// the node key, so enrollment through it must be signed by that node.
pub struct InviteEnrollment {
    pub endpoint: ControlEndpoint,
    pub access_code: SecretString,
    pub access_expires_at_epoch_seconds: Option<u64>,
    pub node_fingerprint: String,
}

impl InviteEnrollment {
    /// Verifies the invite (signature, structure, and expiry at `now`) and
    /// selects the first locator this client may use.
    pub fn from_text(text: &str, now_epoch_seconds: u64) -> Result<Self, ClientError> {
        let invite = decode_invite_text(text.trim(), Some(now_epoch_seconds))
            .map_err(ClientError::Invite)?;
        let capsule = invite.capsule();
        if !capsule.supports_control_api(CONTROL_API_VERSION) {
            return Err(ClientError::Invite(InviteError::UnsupportedVersion));
        }
        let endpoint = capsule
            .locators
            .iter()
            .find_map(|locator| ControlEndpoint::parse(&locator.base_url()).ok())
            .ok_or(ClientError::Invite(InviteError::UnsupportedLocator))?
            .with_node_key(capsule.node_public_key);
        Ok(Self {
            endpoint,
            access_code: SecretString::new(capsule.access_code()),
            access_expires_at_epoch_seconds: capsule.access_expires_at_epoch_seconds,
            node_fingerprint: capsule.node_public_key.fingerprint(),
        })
    }
}

impl std::fmt::Debug for InviteEnrollment {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InviteEnrollment")
            .field("endpoint", &self.endpoint)
            .field("access_code", &self.access_code)
            .field(
                "access_expires_at_epoch_seconds",
                &self.access_expires_at_epoch_seconds,
            )
            .field("node_fingerprint", &self.node_fingerprint)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use ket_invite::{
        ControlLocator, InviteCapsuleV1, LoopbackHost, NodeSigningKey, SignedInviteCapsule,
        encode_invite_text,
    };

    use super::*;

    const CODE: &str = "Abcdefghijklmnopqrstuvwxyz012345";

    fn invite_text(locators: Vec<ControlLocator>, expiry: Option<u64>) -> String {
        let key = NodeSigningKey::from_seed(&[0x66; 32]);
        let capsule = InviteCapsuleV1::new(key.public_key(), locators, CODE, expiry).unwrap();
        encode_invite_text(&SignedInviteCapsule::sign(capsule, &key).unwrap())
            .unwrap()
            .to_string()
    }

    #[test]
    fn invites_produce_a_pinned_endpoint_and_access_code() {
        let text = invite_text(
            vec![ControlLocator::https("ket.example.com", 8443).unwrap()],
            Some(2_000_000_000),
        );
        let enrollment =
            InviteEnrollment::from_text(&format!("  {text}\n"), 1_900_000_000).unwrap();
        assert_eq!(
            enrollment.endpoint.as_str(),
            "https://ket.example.com:8443/"
        );
        assert_eq!(enrollment.access_code.expose_secret(), CODE);
        assert_eq!(
            enrollment.access_expires_at_epoch_seconds,
            Some(2_000_000_000)
        );
        let key = NodeSigningKey::from_seed(&[0x66; 32]).public_key();
        assert_eq!(enrollment.endpoint.node_key(), Some(&key));
        assert_eq!(enrollment.node_fingerprint, key.fingerprint());
        assert!(!format!("{enrollment:?}").contains(CODE));
    }

    #[test]
    fn desktop_skips_locators_it_cannot_reach_and_rejects_expired_invites() {
        let text = invite_text(
            vec![
                ControlLocator::LoopbackDevelopment {
                    host: LoopbackHost::AndroidEmulator,
                    port: 8787,
                },
                ControlLocator::LoopbackDevelopment {
                    host: LoopbackHost::Ipv4,
                    port: 8787,
                },
            ],
            None,
        );
        let enrollment = InviteEnrollment::from_text(&text, 1).unwrap();
        assert_eq!(enrollment.endpoint.as_str(), "http://127.0.0.1:8787/");

        let emulator_only = invite_text(
            vec![ControlLocator::LoopbackDevelopment {
                host: LoopbackHost::AndroidEmulator,
                port: 8787,
            }],
            None,
        );
        assert!(matches!(
            InviteEnrollment::from_text(&emulator_only, 1),
            Err(ClientError::Invite(InviteError::UnsupportedLocator))
        ));

        let expired = invite_text(
            vec![ControlLocator::https("ket.example.com", 443).unwrap()],
            Some(100),
        );
        let error = InviteEnrollment::from_text(&expired, 100).unwrap_err();
        assert_eq!(error.issue().code, "invite_expired");
        assert!(matches!(
            InviteEnrollment::from_text("ket:invite:AAAA", 1),
            Err(ClientError::Invite(_))
        ));
    }
}
