//! Node identity for invite issuance and node-authenticated enrollment.
//!
//! This module is the only holder of the node signing key. The access service
//! and data plane stay unaware of invites; the API layer asks the issuer to
//! wrap a freshly created access code and to sign enrollment responses.

use std::env;

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ket_core::SecretString;
use ket_invite::{
    ControlLocator, EnrollmentNonce, EnrollmentProof, KetInvite, NodeSigningKey,
    SignedInviteCapsule, encode_invite_text, node_fingerprint,
};
use ket_seal::NodeOpeningKey;
use zeroize::Zeroizing;

pub const NODE_SIGNING_KEY_ENV: &str = "KET_NODE_SIGNING_KEY";

pub struct InviteIssuer {
    signing_key: NodeSigningKey,
    opening_key: NodeOpeningKey,
    locator: ControlLocator,
}

impl InviteIssuer {
    /// Reads `KET_NODE_SIGNING_KEY`. Absent means invites are disabled; a
    /// present key requires a public URL that maps to an invite locator.
    pub fn from_env(public_url: &str) -> Result<Option<Self>> {
        Self::from_configured_key(env::var(NODE_SIGNING_KEY_ENV).ok().as_deref(), public_url)
    }

    fn from_configured_key(value: Option<&str>, public_url: &str) -> Result<Option<Self>> {
        let Some(value) = value.filter(|value| !value.is_empty()) else {
            return Ok(None);
        };
        let signing_key = NodeSigningKey::from_base64url(value).with_context(|| {
            format!("{NODE_SIGNING_KEY_ENV} must be a 32-byte seed in unpadded Base64URL")
        })?;
        Self::new(signing_key, public_url).map(Some)
    }

    pub fn new(signing_key: NodeSigningKey, public_url: &str) -> Result<Self> {
        let locator = match KetInvite::locator_from_public_url(public_url) {
            Ok(locator) => locator,
            Err(_) => bail!(
                "{NODE_SIGNING_KEY_ENV} requires KET_PUBLIC_URL to be an https:// public DNS name \
                 without a path, or a loopback development URL"
            ),
        };
        let seed = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(signing_key.to_base64url().as_bytes())
                .context("the node signing key has no seed")?,
        );
        let seed: &[u8; 32] = seed
            .as_slice()
            .try_into()
            .context("the node signing seed is not 32 bytes")?;
        Ok(Self {
            opening_key: NodeOpeningKey::from_ed25519_seed(seed),
            signing_key,
            locator,
        })
    }

    /// Opens control requests sealed to this node's pinned key.
    pub fn opening_key(&self) -> &NodeOpeningKey {
        &self.opening_key
    }

    pub fn fingerprint(&self) -> String {
        node_fingerprint(&self.signing_key.public_key())
    }

    /// Returns `ket:invite:...` for a newly created access grant.
    pub fn issue(
        &self,
        access_code: &SecretString,
        expires_at_epoch_seconds: u64,
    ) -> Result<SecretString> {
        let capsule = KetInvite::capsule(
            self.signing_key.public_key(),
            vec![self.locator.clone()],
            access_code.expose_secret(),
            Some(expires_at_epoch_seconds),
        )
        .context("the access grant cannot be wrapped in an invite")?;
        let invite = SignedInviteCapsule::sign(capsule, &self.signing_key)
            .context("failed to sign the invite")?;
        let text = encode_invite_text(&invite).context("failed to encode the invite")?;
        Ok(SecretString::new(text.as_str()))
    }

    /// The `Ket-Node-Signature` value for an enrollment response body.
    pub fn sign_enrollment(&self, nonce: &EnrollmentNonce, body: &[u8]) -> String {
        EnrollmentProof::sign(&self.signing_key, nonce, body)
    }
}

impl std::fmt::Debug for InviteIssuer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("InviteIssuer")
            .field("fingerprint", &self.fingerprint())
            .field("locator", &self.locator)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use ket_invite::{EnrollmentProof, decode_invite_text};

    use super::*;

    const CODE: &str = "Abcdefghijklmnopqrstuvwxyz012345";

    fn issuer() -> InviteIssuer {
        InviteIssuer::new(
            NodeSigningKey::from_seed(&[0x44; 32]),
            "https://ket.example.com",
        )
        .unwrap()
    }

    #[test]
    fn issued_invites_carry_the_grant_and_verify_against_the_node_key() {
        let issuer = issuer();
        let text = issuer
            .issue(&SecretString::new(CODE), 2_000_000_000)
            .unwrap();
        let invite = decode_invite_text(text.expose_secret(), Some(1_900_000_000)).unwrap();
        let capsule = invite.capsule();
        assert_eq!(capsule.capability().expose(), CODE);
        assert_eq!(capsule.expires_at_epoch_seconds(), Some(2_000_000_000));
        assert_eq!(capsule.locators()[0].base_url(), "https://ket.example.com/");
        assert_eq!(
            node_fingerprint(capsule.node_public_key()),
            issuer.fingerprint()
        );
    }

    #[test]
    fn enrollment_signatures_verify_with_the_invite_node_key() {
        let issuer = issuer();
        let nonce = EnrollmentNonce::generate();
        let header = issuer.sign_enrollment(&nonce, b"{}");
        let public = NodeSigningKey::from_seed(&[0x44; 32]).public_key();
        EnrollmentProof::verify(&public, &nonce, b"{}", Some(&header)).unwrap();
    }

    #[test]
    fn configured_keys_are_optional_but_must_be_well_formed() {
        let url = "https://ket.example.com";
        assert!(
            InviteIssuer::from_configured_key(None, url)
                .unwrap()
                .is_none()
        );
        assert!(
            InviteIssuer::from_configured_key(Some(""), url)
                .unwrap()
                .is_none()
        );
        let seed = "REREREREREREREREREREREREREREREREREREREREREQ";
        let configured = InviteIssuer::from_configured_key(Some(seed), url)
            .unwrap()
            .expect("a configured key enables invites");
        assert_eq!(configured.fingerprint(), issuer().fingerprint());
        for invalid in [&format!("{seed}="), "short", "not base64!"] {
            let error = InviteIssuer::from_configured_key(Some(invalid), url).unwrap_err();
            assert!(
                error.to_string().contains(NODE_SIGNING_KEY_ENV),
                "{invalid}"
            );
        }
        assert!(
            InviteIssuer::from_configured_key(Some(seed), "https://ket.example.com/sub").is_err()
        );
    }

    #[test]
    fn signing_keys_require_an_invite_compatible_public_url() {
        for url in [
            "https://ket.example.com/prefix",
            "https://203.0.113.9",
            "https://nl.example.test",
        ] {
            assert!(
                InviteIssuer::new(NodeSigningKey::from_seed(&[1; 32]), url).is_err(),
                "{url}"
            );
        }
        assert!(
            InviteIssuer::new(NodeSigningKey::from_seed(&[1; 32]), "http://127.0.0.1:8787").is_ok()
        );
        let debug = format!("{:?}", issuer());
        assert!(debug.contains("fingerprint"));
    }
}
