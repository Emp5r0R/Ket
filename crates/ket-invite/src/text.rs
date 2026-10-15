//! `ket:invite:<base64url>` text form.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use zeroize::Zeroizing;

use crate::{
    InviteError, MAX_BINARY_INVITE_BYTES, MAX_ENCODED_INVITE_TEXT_BYTES, SignedInviteCapsule,
};

pub const INVITE_TEXT_PREFIX: &str = "ket:invite:";

pub fn encode_invite_text(invite: &SignedInviteCapsule) -> Result<Zeroizing<String>, InviteError> {
    let binary = invite.canonical_binary()?;
    Ok(Zeroizing::new(format!(
        "{INVITE_TEXT_PREFIX}{}",
        URL_SAFE_NO_PAD.encode(binary.as_slice())
    )))
}

/// Decodes and fully verifies invite text. The prefix is optional; anything
/// other than exact canonical Base64URL fails.
pub fn decode_invite_text(
    input: &str,
    now_epoch_seconds: Option<u64>,
) -> Result<SignedInviteCapsule, InviteError> {
    if input.is_empty() || input.len() > MAX_ENCODED_INVITE_TEXT_BYTES {
        return Err(InviteError::TooLarge);
    }
    let encoded = input.strip_prefix(INVITE_TEXT_PREFIX).unwrap_or(input);
    if encoded.is_empty()
        || !encoded
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(InviteError::Invalid);
    }
    let binary = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| InviteError::Invalid)?,
    );
    if binary.len() > MAX_BINARY_INVITE_BYTES {
        return Err(InviteError::TooLarge);
    }
    if URL_SAFE_NO_PAD.encode(binary.as_slice()) != encoded {
        return Err(InviteError::Invalid);
    }
    SignedInviteCapsule::decode(&binary, now_epoch_seconds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ControlLocator, InviteCapsuleV1, NodeSigningKey};

    fn invite() -> SignedInviteCapsule {
        let key = NodeSigningKey::from_seed(&[0x41; 32]);
        let capsule = InviteCapsuleV1::new(
            key.public_key(),
            vec![ControlLocator::https("ket.example.com", 443).unwrap()],
            "ABCDEFGHIJKLMNOPQRSTUVWXYZ012345",
            Some(2_000_000_000),
        )
        .unwrap();
        SignedInviteCapsule::sign(capsule, &key).unwrap()
    }

    #[test]
    fn prefixed_and_bare_text_decode_to_the_same_invite() {
        let invite = invite();
        let text = encode_invite_text(&invite).unwrap();
        assert!(text.starts_with(INVITE_TEXT_PREFIX));
        let bare = &text[INVITE_TEXT_PREFIX.len()..];
        assert_eq!(
            decode_invite_text(&text, Some(1_900_000_000)).unwrap(),
            invite
        );
        assert_eq!(
            decode_invite_text(bare, Some(1_900_000_000)).unwrap(),
            invite
        );
    }

    #[test]
    fn non_canonical_or_decorated_text_is_rejected() {
        let text = encode_invite_text(&invite()).unwrap();
        let bare = &text[INVITE_TEXT_PREFIX.len()..];
        for invalid in [
            format!("{}=", text.as_str()),
            format!(" {}", text.as_str()),
            format!("{}\n", text.as_str()),
            format!("KET:INVITE:{bare}"),
            format!("ket:invite:{}", bare.replace('-', "+").replace('_', "/")),
            INVITE_TEXT_PREFIX.to_owned(),
            "ket:invite:%41".to_owned(),
        ] {
            assert!(decode_invite_text(&invalid, None).is_err(), "{invalid:?}");
        }
        assert_eq!(
            decode_invite_text(&"A".repeat(MAX_ENCODED_INVITE_TEXT_BYTES + 1), None),
            Err(InviteError::TooLarge)
        );
        assert_eq!(decode_invite_text("", None), Err(InviteError::TooLarge));
    }

    #[test]
    fn expired_invites_are_rejected_when_a_clock_is_supplied() {
        let text = encode_invite_text(&invite()).unwrap();
        assert_eq!(
            decode_invite_text(&text, Some(2_000_000_000)),
            Err(InviteError::Expired)
        );
    }
}
