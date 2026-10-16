//! The shared rules, exercised through two example applications.

use invite_capsule::{
    Capability, CapsuleError, InviteCapsule, InviteProfile, Locator, LocatorKinds, ManualText,
    NodeSigningKey, SignedInvite,
    cbor::{self, Decoder},
    signature_input,
};
use zeroize::Zeroizing;

/// A 32-byte random capability, as a byte string.
#[derive(Clone, Eq, PartialEq)]
struct Token(Zeroizing<[u8; 32]>);

impl std::fmt::Debug for Token {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Token(<redacted>)")
    }
}

impl Capability for Token {
    fn encode(&self, output: &mut Vec<u8>) {
        cbor::encode_bytes(output, self.0.as_slice());
    }

    fn decode(decoder: &mut Decoder<'_>) -> Result<Self, CapsuleError> {
        let bytes: [u8; 32] = decoder
            .bytes(32)?
            .try_into()
            .map_err(|_| CapsuleError::Invalid)?;
        (bytes != [0; 32])
            .then(|| Self(Zeroizing::new(bytes)))
            .ok_or(CapsuleError::Invalid)
    }
}

enum Clearnet {}

impl InviteProfile for Clearnet {
    const APPLICATION_ID: &'static str = "org.example.clearnet";
    const SIGNATURE_DOMAIN: &'static [u8] = b"EXAMPLE-CLEARNET-INVITE-V1";
    const CAPABILITY_TYPE: u64 = 1;
    const LOCATORS: LocatorKinds = LocatorKinds::CLEARNET;
    const TEXT_PREFIX: &'static str = "clear:invite:";
    type Capability = Token;
}

enum Everywhere {}

impl InviteProfile for Everywhere {
    const APPLICATION_ID: &'static str = "org.example.everywhere";
    const SIGNATURE_DOMAIN: &'static [u8] = b"EXAMPLE-EVERYWHERE-INVITE-V1";
    const CAPABILITY_TYPE: u64 = 1;
    const LOCATORS: LocatorKinds = LocatorKinds::ALL;
    const TEXT_PREFIX: &'static str = "every:invite:";
    const MANUAL_TEXT: Option<ManualText> = Some(ManualText {
        prefix: "EVR1-",
        checksum_domain: b"EXAMPLE-EVERYWHERE-CHECKSUM-V1",
    });
    type Capability = Token;
}

const NOW: u64 = 1_900_000_000;
const EXPIRES: u64 = 2_000_000_000;

fn key() -> NodeSigningKey {
    NodeSigningKey::from_seed(&[0x11; 32])
}

fn token() -> Token {
    Token(Zeroizing::new([0x42; 32]))
}

fn capsule<P: InviteProfile<Capability = Token>>(locators: Vec<Locator>) -> InviteCapsule<P> {
    InviteCapsule::new(key().public_key(), locators, token(), 1, 3, Some(EXPIRES)).unwrap()
}

fn https() -> Locator {
    Locator::https("node.example.com", 443).unwrap()
}

fn signed<P: InviteProfile<Capability = Token>>() -> SignedInvite<P> {
    SignedInvite::sign(capsule::<P>(vec![https()]), &key()).unwrap()
}

#[test]
fn invites_round_trip_through_binary_and_text() {
    let invite = signed::<Clearnet>();
    let binary = invite.canonical_binary().unwrap();
    assert_eq!(
        SignedInvite::<Clearnet>::decode(&binary, Some(NOW)).unwrap(),
        invite
    );
    let text = invite.to_text().unwrap();
    assert!(text.starts_with("clear:invite:"));
    let decoded = SignedInvite::<Clearnet>::from_text(&text, Some(NOW)).unwrap();
    assert_eq!(decoded, invite);
    let capsule = decoded.capsule();
    assert_eq!(capsule.capability(), &token());
    assert_eq!(capsule.locators(), [https()]);
    assert_eq!(capsule.expires_at_epoch_seconds(), Some(EXPIRES));
    assert!(capsule.supports_protocol(2));
    assert!(!capsule.supports_protocol(4));
}

#[test]
fn an_invite_for_one_application_is_wrong_for_another() {
    let clearnet = signed::<Clearnet>().canonical_binary().unwrap();
    assert_eq!(
        SignedInvite::<Everywhere>::decode(&clearnet, None),
        Err(CapsuleError::WrongApplication)
    );
    let everywhere = signed::<Everywhere>().canonical_binary().unwrap();
    assert_eq!(
        SignedInvite::<Clearnet>::decode(&everywhere, None),
        Err(CapsuleError::WrongApplication)
    );
}

#[test]
fn every_bit_of_the_binary_is_authenticated_or_structural() {
    let binary = signed::<Clearnet>().canonical_binary().unwrap();
    for index in 0..binary.len() {
        for bit in 0..8 {
            let mut tampered = binary.to_vec();
            tampered[index] ^= 1 << bit;
            assert!(
                SignedInvite::<Clearnet>::decode(&tampered, None).is_err(),
                "byte {index} bit {bit}"
            );
        }
    }
}

#[test]
fn locator_kinds_follow_the_profile() {
    let i2p = Locator::from_public_url(
        "http://ukeu3k5oycgaauneqgtnvselmt4yemvoilkln7jpvamvfx7dnkdq.b32.i2p",
    )
    .unwrap();
    assert_eq!(
        InviteCapsule::<Clearnet>::new(key().public_key(), vec![i2p.clone()], token(), 1, 1, None),
        Err(CapsuleError::UnsupportedLocator)
    );
    let everywhere =
        SignedInvite::sign(capsule::<Everywhere>(vec![https(), i2p.clone()]), &key()).unwrap();
    let decoded =
        SignedInvite::<Everywhere>::decode(&everywhere.canonical_binary().unwrap(), Some(NOW))
            .unwrap();
    assert_eq!(decoded.capsule().locators(), [https(), i2p]);
}

#[test]
fn signing_requires_the_node_key_named_in_the_capsule() {
    let other = NodeSigningKey::from_seed(&[0x12; 32]);
    assert_eq!(
        SignedInvite::sign(capsule::<Clearnet>(vec![https()]), &other),
        Err(CapsuleError::NodeIdentityMismatch)
    );
}

#[test]
fn capsules_reject_empty_or_inverted_protocol_ranges() {
    for (min, max) in [(0, 1), (2, 1)] {
        assert_eq!(
            InviteCapsule::<Clearnet>::new(
                key().public_key(),
                vec![https()],
                token(),
                min,
                max,
                None
            ),
            Err(CapsuleError::Invalid),
            "{min}..={max}"
        );
    }
}

#[test]
fn expiry_is_checked_after_the_signature_and_only_with_a_clock() {
    let invite = signed::<Clearnet>();
    assert_eq!(invite.verify(Some(EXPIRES)), Err(CapsuleError::Expired));
    assert!(invite.verify(Some(EXPIRES - 1)).is_ok());
    assert!(invite.verify(None).is_ok());

    assert!(invite.capsule().is_expired_at(EXPIRES));
    assert!(!invite.capsule().is_expired_at(EXPIRES - 1));
    let rebuilt =
        SignedInvite::from_parts(invite.capsule().clone(), *invite.signature(), Some(NOW));
    assert_eq!(rebuilt.as_ref(), Ok(&invite));
    let mut bad_signature = *invite.signature();
    bad_signature[0] ^= 1;
    assert_eq!(
        SignedInvite::from_parts(invite.capsule().clone(), bad_signature, Some(u64::MAX)),
        Err(CapsuleError::InvalidSignature)
    );

    let mut forged = invite.canonical_binary().unwrap().to_vec();
    let last = forged.len() - 1;
    forged[last] ^= 1;
    assert_eq!(
        SignedInvite::<Clearnet>::decode(&forged, Some(u64::MAX)),
        Err(CapsuleError::InvalidSignature)
    );
}

/// Signs arbitrary payload bytes so the checks behind a valid signature are
/// reachable.
fn sign_raw<P: InviteProfile>(fields: &[Vec<u8>]) -> Vec<u8> {
    let mut payload = Vec::new();
    cbor::encode_array(&mut payload, fields.len());
    for field in fields {
        payload.extend_from_slice(field);
    }
    let signature = key().sign(&signature_input(
        P::SIGNATURE_DOMAIN,
        P::APPLICATION_ID,
        &payload,
    ));
    let mut output = Vec::new();
    cbor::encode_array(&mut output, 2);
    cbor::encode_bytes(&mut output, &payload);
    cbor::encode_bytes(&mut output, &signature);
    output
}

fn valid_fields() -> Vec<Vec<u8>> {
    let mut fields = vec![Vec::new(); 10];
    cbor::encode_uint(&mut fields[0], 1);
    cbor::encode_text(&mut fields[1], Clearnet::APPLICATION_ID);
    cbor::encode_uint(&mut fields[2], 1);
    cbor::encode_bytes(&mut fields[3], key().public_key().as_bytes());
    cbor::encode_array(&mut fields[4], 1);
    https().encode(&mut fields[4]);
    token().encode(&mut fields[5]);
    cbor::encode_uint(&mut fields[6], 1);
    cbor::encode_uint(&mut fields[7], 1);
    cbor::encode_uint(&mut fields[8], 0);
    cbor::encode_uint(&mut fields[9], EXPIRES);
    fields
}

#[test]
fn signed_but_semantically_invalid_payloads_are_rejected() {
    let decode = |fields: Vec<Vec<u8>>| {
        SignedInvite::<Clearnet>::decode(&sign_raw::<Clearnet>(&fields), None).map(|_| ())
    };
    assert_eq!(decode(valid_fields()), Ok(()));
    let with = |index: usize, encode: &dyn Fn(&mut Vec<u8>)| {
        let mut fields = valid_fields();
        fields[index].clear();
        encode(&mut fields[index]);
        fields
    };
    let cases: Vec<(Vec<Vec<u8>>, CapsuleError)> = vec![
        (
            with(0, &|out| cbor::encode_uint(out, 2)),
            CapsuleError::UnsupportedVersion,
        ),
        (
            with(1, &|out| cbor::encode_text(out, "org.other.app")),
            CapsuleError::WrongApplication,
        ),
        (
            with(2, &|out| cbor::encode_uint(out, 2)),
            CapsuleError::UnsupportedCapability,
        ),
        (
            with(3, &|out| cbor::encode_bytes(out, &[0; 32])),
            CapsuleError::InvalidNodeKey,
        ),
        (
            with(3, &|out| cbor::encode_bytes(out, &[1; 31])),
            CapsuleError::Invalid,
        ),
        (
            with(4, &|out| cbor::encode_array(out, 0)),
            CapsuleError::Invalid,
        ),
        (
            with(4, &|out| {
                cbor::encode_array(out, 2);
                https().encode(out);
                https().encode(out);
            }),
            CapsuleError::Invalid,
        ),
        (
            with(5, &|out| cbor::encode_bytes(out, &[0; 32])),
            CapsuleError::Invalid,
        ),
        (
            with(5, &|out| cbor::encode_bytes(out, &[1; 33])),
            CapsuleError::TooLarge,
        ),
        (
            with(6, &|out| cbor::encode_uint(out, 0)),
            CapsuleError::Invalid,
        ),
        (
            with(7, &|out| cbor::encode_uint(out, 70_000)),
            CapsuleError::Invalid,
        ),
        (
            with(8, &|out| cbor::encode_uint(out, 1)),
            CapsuleError::Invalid,
        ),
        (with(9, &|out| out.push(0xf7)), CapsuleError::Invalid),
        (
            with(0, &|out| out.extend_from_slice(&[0x18, 0x01])),
            CapsuleError::Invalid,
        ),
    ];
    for (index, (fields, expected)) in cases.into_iter().enumerate() {
        assert_eq!(decode(fields), Err(expected), "case {index}");
    }
}

#[test]
fn the_manual_form_round_trips_and_needs_a_profile_that_has_one() {
    let invite = signed::<Everywhere>();
    let manual = invite.to_manual_text().unwrap();
    assert!(manual.starts_with("EVR1-"));
    for text in [manual.to_string(), manual.to_lowercase()] {
        assert_eq!(
            SignedInvite::<Everywhere>::from_text(&text, Some(NOW)).unwrap(),
            invite
        );
    }
    assert_eq!(
        signed::<Clearnet>().to_manual_text().map(|_| ()),
        Err(CapsuleError::Invalid)
    );
}

#[test]
fn text_is_bounded_and_strict() {
    assert_eq!(
        SignedInvite::<Clearnet>::from_text("", None),
        Err(CapsuleError::TooLarge)
    );
    assert_eq!(
        SignedInvite::<Clearnet>::from_text(&"A".repeat(2_049), None),
        Err(CapsuleError::TooLarge)
    );
    let text = signed::<Clearnet>().to_text().unwrap();
    for invalid in [
        format!("{}=", text.as_str()),
        format!(" {}", text.as_str()),
        format!("{}\n", text.as_str()),
        text.to_uppercase(),
    ] {
        assert!(
            SignedInvite::<Clearnet>::from_text(&invalid, None).is_err(),
            "{invalid:?}"
        );
    }
}

#[test]
fn hostile_inputs_never_panic() {
    let binary = signed::<Everywhere>().canonical_binary().unwrap();
    for length in 0..binary.len() {
        let _ = SignedInvite::<Everywhere>::decode(&binary[..length], None);
    }
    for seed in 0_u8..=255 {
        let noise = (0..300_u16)
            .map(|index| (index as u8).wrapping_mul(31).wrapping_add(seed))
            .collect::<Vec<_>>();
        let _ = SignedInvite::<Everywhere>::decode(&noise, None);
        let text = String::from_utf8_lossy(&noise).into_owned();
        let _ = SignedInvite::<Everywhere>::from_text(&text, None);
        let _ = SignedInvite::<Everywhere>::from_text(&format!("EVR1-{text}"), None);
    }
}

#[test]
fn debug_output_never_contains_the_capability() {
    let debug = format!("{:?}", signed::<Clearnet>());
    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("66, 66"));
}
