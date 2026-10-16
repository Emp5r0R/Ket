//! What the Android app asks of an invite, in plain Rust so it is tested
//! without a JVM.

use ket_invite::{
    CONTROL_API_VERSION, EnrollmentNonce, EnrollmentProof, InviteError, NodePublicKey,
    decode_invite_text, node_fingerprint,
};
use zeroize::Zeroizing;

/// A verified invite, reduced to what the app stores and shows.
pub struct InviteFields {
    pub node_key: String,
    pub node_fingerprint: String,
    pub access_code: Zeroizing<String>,
    pub access_expires_at_epoch_seconds: Option<u64>,
    /// Control API origins (`https://host[:port]`), in the invite's order.
    pub origins: Vec<String>,
}

/// Verifies `text` (signature, structure, and expiry at `now`) for this
/// client's control API version.
pub fn read_invite(text: &str, now_epoch_seconds: i64) -> Result<InviteFields, InviteError> {
    let now = u64::try_from(now_epoch_seconds).map_err(|_| InviteError::Invalid)?;
    let invite = decode_invite_text(text, Some(now))?;
    let capsule = invite.capsule();
    if !capsule.supports_protocol(CONTROL_API_VERSION) {
        return Err(InviteError::UnsupportedVersion);
    }
    Ok(InviteFields {
        node_key: capsule.node_public_key().to_base64url(),
        node_fingerprint: node_fingerprint(capsule.node_public_key()),
        access_code: Zeroizing::new(capsule.capability().expose().to_owned()),
        access_expires_at_epoch_seconds: capsule.expires_at_epoch_seconds(),
        origins: capsule
            .locators()
            .iter()
            .map(|locator| locator.origin())
            .collect(),
    })
}

/// The fingerprint of a stored node key, or `None` if it is not a valid key.
pub fn node_key_fingerprint(node_key: &str) -> Option<String> {
    NodePublicKey::from_base64url(node_key)
        .ok()
        .map(|key| node_fingerprint(&key))
}

/// Checks `Ket-Node-Signature` over an enrollment response body.
pub fn verify_enrollment(
    node_key: &str,
    nonce: &str,
    body: &[u8],
    signature: Option<&str>,
) -> Result<(), InviteError> {
    let node_key = NodePublicKey::from_base64url(node_key)?;
    let nonce = EnrollmentNonce::from_header_value(nonce)?;
    EnrollmentProof::verify(&node_key, &nonce, body, signature)
}

#[cfg(test)]
mod tests {
    use ket_invite::{
        ControlLocator, KetInvite, LoopbackHost, NodeSigningKey, SignedInviteCapsule,
        encode_invite_text,
    };

    use super::*;

    const CODE: &str = "Abcdefghijklmnopqrstuvwxyz012345";

    fn key() -> NodeSigningKey {
        NodeSigningKey::from_seed(&[0x24; 32])
    }

    fn invite(locators: Vec<ControlLocator>, expiry: Option<u64>) -> String {
        let capsule = KetInvite::capsule(key().public_key(), locators, CODE, expiry).unwrap();
        encode_invite_text(&SignedInviteCapsule::sign(capsule, &key()).unwrap())
            .unwrap()
            .to_string()
    }

    #[test]
    fn invites_reduce_to_the_fields_the_app_keeps() {
        let text = invite(
            vec![
                ControlLocator::https("ket.example.com", 8443).unwrap(),
                ControlLocator::LoopbackDevelopment {
                    host: LoopbackHost::AndroidEmulator,
                    port: 8787,
                },
            ],
            Some(2_000_000_000),
        );
        let fields = read_invite(&text, 1_900_000_000).unwrap();
        assert_eq!(fields.node_key, key().public_key().to_base64url());
        assert_eq!(
            fields.node_fingerprint,
            node_fingerprint(&key().public_key())
        );
        assert_eq!(fields.access_code.as_str(), CODE);
        assert_eq!(fields.access_expires_at_epoch_seconds, Some(2_000_000_000));
        assert_eq!(
            fields.origins,
            ["https://ket.example.com:8443", "http://10.0.2.2:8787"]
        );
    }

    #[test]
    fn expired_tampered_and_clockless_invites_are_rejected() {
        let text = invite(
            vec![ControlLocator::https("ket.example.com", 443).unwrap()],
            Some(100),
        );
        assert_eq!(
            read_invite(&text, 100).map(|_| ()),
            Err(InviteError::Expired)
        );
        assert_eq!(
            read_invite(&text, -1).map(|_| ()),
            Err(InviteError::Invalid)
        );
        assert!(read_invite(&format!("{text}A"), 1).is_err());
    }

    #[test]
    fn stored_node_keys_are_validated_before_they_are_shown() {
        let encoded = key().public_key().to_base64url();
        assert_eq!(
            node_key_fingerprint(&encoded),
            Some(node_fingerprint(&key().public_key()))
        );
        assert_eq!(node_key_fingerprint(&format!("{encoded}=")), None);
        assert_eq!(node_key_fingerprint(""), None);
    }

    #[test]
    fn enrollment_proofs_bind_the_node_nonce_and_body() {
        let nonce = EnrollmentNonce::from_bytes([0x5c; 32]);
        let signature = EnrollmentProof::sign(&key(), &nonce, b"{}");
        let node = key().public_key().to_base64url();
        let header = nonce.header_value();
        assert_eq!(
            verify_enrollment(&node, &header, b"{}", Some(&signature)),
            Ok(())
        );
        assert_eq!(
            verify_enrollment(&node, &header, b"{ }", Some(&signature)),
            Err(InviteError::NodeIdentityMismatch)
        );
        assert_eq!(
            verify_enrollment(&node, &header, b"{}", None),
            Err(InviteError::NodeIdentityMismatch)
        );
        assert_eq!(
            verify_enrollment("bad", &header, b"{}", Some(&signature)),
            Err(InviteError::InvalidNodeKey)
        );
        assert_eq!(
            verify_enrollment(&node, "bad", b"{}", Some(&signature)),
            Err(InviteError::Invalid)
        );
    }
}
