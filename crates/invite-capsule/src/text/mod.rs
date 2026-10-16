//! Text forms of a canonical binary invite. These functions only transcode;
//! [`crate::SignedInvite::from_text`] verifies what they return.

mod manual;

use data_encoding::BASE64URL_NOPAD;
use zeroize::Zeroizing;

use crate::{CapsuleError, MAX_BINARY_INVITE_BYTES};

pub(crate) use manual::{decode_manual, encode_manual};

pub(crate) fn encode_base64url(prefix: &str, binary: &[u8]) -> Zeroizing<String> {
    Zeroizing::new(format!("{prefix}{}", BASE64URL_NOPAD.encode(binary)))
}

/// Strict unpadded Base64URL with an optional exact `prefix`.
pub(crate) fn decode_base64url(
    prefix: &str,
    input: &str,
) -> Result<Zeroizing<Vec<u8>>, CapsuleError> {
    let encoded = input.strip_prefix(prefix).unwrap_or(input);
    if encoded.is_empty()
        || !encoded
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(CapsuleError::Invalid);
    }
    let binary = Zeroizing::new(
        BASE64URL_NOPAD
            .decode(encoded.as_bytes())
            .map_err(|_| CapsuleError::Invalid)?,
    );
    if binary.len() > MAX_BINARY_INVITE_BYTES {
        return Err(CapsuleError::TooLarge);
    }
    if *Zeroizing::new(BASE64URL_NOPAD.encode(&binary)) != encoded {
        return Err(CapsuleError::Invalid);
    }
    Ok(binary)
}

/// Case-insensitive prefix test for the manual form.
pub(crate) fn has_prefix(input: &str, prefix: &str) -> bool {
    input
        .get(..prefix.len())
        .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_is_strict_and_the_prefix_optional() {
        let text = encode_base64url("app:invite:", b"\x01\x02\x03\xff");
        assert_eq!(text.as_str(), "app:invite:AQID_w");
        for input in [text.as_str(), "AQID_w"] {
            assert_eq!(
                decode_base64url("app:invite:", input).unwrap().as_slice(),
                b"\x01\x02\x03\xff"
            );
        }
        for invalid in [
            "AQID_w=",
            " AQID_w",
            "AQID_w\n",
            "AQID/w",
            "APP:INVITE:AQID_w",
            "app:invite:",
            "AQID_x",
            "%41",
        ] {
            assert_eq!(
                decode_base64url("app:invite:", invalid).map(|_| ()),
                Err(CapsuleError::Invalid),
                "{invalid:?}"
            );
        }
        let large = BASE64URL_NOPAD.encode(&[7_u8; MAX_BINARY_INVITE_BYTES + 1]);
        assert_eq!(
            decode_base64url("", &large).map(|_| ()),
            Err(CapsuleError::TooLarge)
        );
    }
}
