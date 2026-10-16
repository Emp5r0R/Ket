//! The exact bytes a node signs.

/// `domain || u16be(len(application_id)) || application_id || payload`.
///
/// The domain separates object kinds (an invite from, say, a node
/// descriptor) and the application ID separates applications, so a signature
/// made for one never verifies as another.
pub fn signature_input(domain: &[u8], application_id: &str, payload: &[u8]) -> Vec<u8> {
    let length = u16::try_from(application_id.len()).unwrap_or(u16::MAX);
    let mut input = Vec::with_capacity(domain.len() + 2 + application_id.len() + payload.len());
    input.extend_from_slice(domain);
    input.extend_from_slice(&length.to_be_bytes());
    input.extend_from_slice(application_id.as_bytes());
    input.extend_from_slice(payload);
    input
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_is_length_prefixed_so_fields_cannot_shift() {
        assert_eq!(
            signature_input(b"D", "ab", b"P"),
            [b'D', 0, 2, b'a', b'b', b'P']
        );
        assert_ne!(
            signature_input(b"D", "ab", b"cP"),
            signature_input(b"D", "abc", b"P")
        );
    }
}
