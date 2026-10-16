//! The hand-typeable form: `<PREFIX>` then Crockford Base32 of
//! `binary || checksum` in dash-separated groups of five. Decoding ignores
//! case and accepts the Crockford aliases `O` for 0 and `I`/`L` for 1.

use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

use crate::{CapsuleError, MAX_BINARY_INVITE_BYTES, ManualText};

const CHECKSUM_BYTES: usize = 4;
const GROUP: usize = 5;
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

pub(crate) fn encode_manual(form: ManualText, binary: &[u8]) -> Zeroizing<String> {
    let mut checksummed = Zeroizing::new(Vec::with_capacity(binary.len() + CHECKSUM_BYTES));
    checksummed.extend_from_slice(binary);
    checksummed.extend_from_slice(&checksum(form, binary));
    let encoded = Zeroizing::new(crockford_encode(&checksummed));
    let mut output = Zeroizing::new(String::with_capacity(
        form.prefix.len() + encoded.len() + encoded.len() / GROUP,
    ));
    output.push_str(form.prefix);
    for (index, character) in encoded.bytes().enumerate() {
        if index > 0 && index % GROUP == 0 {
            output.push('-');
        }
        output.push(character as char);
    }
    output
}

/// Expects `input` to start with the form's prefix (any case).
pub(crate) fn decode_manual(
    form: ManualText,
    input: &str,
) -> Result<Zeroizing<Vec<u8>>, CapsuleError> {
    let body = input
        .get(form.prefix.len()..)
        .ok_or(CapsuleError::Invalid)?;
    if body.is_empty()
        || body.starts_with('-')
        || body.ends_with('-')
        || body.contains("--")
        || !body
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(CapsuleError::Invalid);
    }
    let compact = Zeroizing::new(
        body.bytes()
            .filter(|byte| *byte != b'-')
            .collect::<Vec<_>>(),
    );
    let mut decoded = Zeroizing::new(crockford_decode(&compact)?);
    if decoded.len() <= CHECKSUM_BYTES || decoded.len() > MAX_BINARY_INVITE_BYTES + CHECKSUM_BYTES {
        return Err(CapsuleError::Invalid);
    }
    let split = decoded.len() - CHECKSUM_BYTES;
    let expected = checksum(form, &decoded[..split]);
    let difference = decoded[split..]
        .iter()
        .zip(expected)
        .fold(0_u8, |difference, (left, right)| {
            difference | (left ^ right)
        });
    if difference != 0 {
        return Err(CapsuleError::InvalidChecksum);
    }
    decoded.truncate(split);
    Ok(decoded)
}

fn checksum(form: ManualText, value: &[u8]) -> [u8; CHECKSUM_BYTES] {
    let mut digest = Sha256::new();
    digest.update(form.checksum_domain);
    digest.update(value);
    let digest = digest.finalize();
    let mut result = [0_u8; CHECKSUM_BYTES];
    result.copy_from_slice(&digest[..CHECKSUM_BYTES]);
    result
}

fn crockford_encode(value: &[u8]) -> String {
    let mut output = String::with_capacity(value.len().saturating_mul(8).div_ceil(5));
    let mut buffer = 0_u16;
    let mut bits = 0_u8;
    for byte in value {
        buffer = (buffer << 8) | u16::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            output.push(CROCKFORD[((buffer >> bits) & 0x1f) as usize] as char);
            buffer &= (1_u16 << bits).saturating_sub(1);
        }
    }
    if bits > 0 {
        output.push(CROCKFORD[((buffer << (5 - bits)) & 0x1f) as usize] as char);
    }
    output
}

fn crockford_decode(value: &[u8]) -> Result<Vec<u8>, CapsuleError> {
    let max_bytes = value.len().saturating_mul(5).div_ceil(8);
    if max_bytes > MAX_BINARY_INVITE_BYTES + CHECKSUM_BYTES {
        return Err(CapsuleError::TooLarge);
    }
    let mut output = Vec::with_capacity(max_bytes);
    let mut buffer = 0_u16;
    let mut bits = 0_u8;
    for byte in value {
        let Some(decoded) = crockford_value(*byte) else {
            output.zeroize();
            return Err(CapsuleError::Invalid);
        };
        buffer = (buffer << 5) | u16::from(decoded);
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
            buffer &= (1_u16 << bits).saturating_sub(1);
        }
    }
    if bits > 0 && buffer != 0 {
        output.zeroize();
        return Err(CapsuleError::Invalid);
    }
    Ok(output)
}

fn crockford_value(byte: u8) -> Option<u8> {
    match byte.to_ascii_uppercase() {
        b'O' => Some(0),
        b'I' | b'L' => Some(1),
        value => CROCKFORD
            .iter()
            .position(|candidate| *candidate == value)
            .map(|index| index as u8),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FORM: ManualText = ManualText {
        prefix: "APP1-",
        checksum_domain: b"APP-INVITE-CHECKSUM-V1",
    };

    #[test]
    fn manual_text_round_trips_in_any_case_and_with_aliases() {
        let binary = (0_u8..=200).collect::<Vec<_>>();
        let text = encode_manual(FORM, &binary);
        assert!(text.starts_with("APP1-"));
        assert!(
            text[FORM.prefix.len()..]
                .split('-')
                .all(|group| (1..=GROUP).contains(&group.len()))
        );
        for variant in [
            text.to_string(),
            text.to_lowercase(),
            format!(
                "{}{}",
                FORM.prefix,
                text[FORM.prefix.len()..]
                    .replace('0', "O")
                    .replace('1', "L")
            ),
        ] {
            assert_eq!(decode_manual(FORM, &variant).unwrap().as_slice(), binary);
        }
    }

    #[test]
    fn typing_mistakes_and_bad_shapes_fail() {
        let text = encode_manual(FORM, b"invite bytes").to_string();
        let index = FORM.prefix.len();
        let mut typo = text.clone().into_bytes();
        typo[index] = if typo[index] == b'Z' { b'Y' } else { b'Z' };
        assert!(matches!(
            decode_manual(FORM, std::str::from_utf8(&typo).unwrap()),
            Err(CapsuleError::InvalidChecksum | CapsuleError::Invalid)
        ));
        for invalid in [
            format!("{text}-"),
            format!("{}-{}", FORM.prefix, &text[index..]),
            text.replacen('-', "--", 2),
            format!("{text}U"),
            format!("{text} "),
            FORM.prefix.to_owned(),
        ] {
            assert!(decode_manual(FORM, &invalid).is_err(), "{invalid:?}");
        }
        let other = ManualText {
            prefix: FORM.prefix,
            checksum_domain: b"OTHER",
        };
        assert_eq!(
            decode_manual(other, &text).map(|_| ()),
            Err(CapsuleError::InvalidChecksum)
        );
    }
}
