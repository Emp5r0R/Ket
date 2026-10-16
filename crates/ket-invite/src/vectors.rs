//! Cross-implementation conformance vectors.
//!
//! `tests/vectors/invite-v1.json` is the single source of truth shared with the
//! Android client. This test regenerates it deterministically and fails on
//! drift; set `KET_UPDATE_INVITE_VECTORS=1` to rewrite it after an intended
//! format change.

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};

use invite_capsule::cbor as codec;

use crate::{
    ControlLocator, EnrollmentNonce, EnrollmentProof, INVITE_TEXT_PREFIX, InviteError, KetInvite,
    LoopbackHost, NodeSigningKey, SignedInviteCapsule, decode_invite_text, encode_invite_text,
    node_fingerprint,
    test_support::{CODE, RawPayload, sign_raw_payload},
};

const VECTOR_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/vectors/invite-v1.json");
const NODE_SEED: [u8; 32] = [0x5a; 32];
const DECODE_AT: u64 = 1_900_000_000;
const EXPIRES_AT: u64 = 2_000_000_000;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn text_of(binary: &[u8]) -> String {
    format!("{INVITE_TEXT_PREFIX}{}", URL_SAFE_NO_PAD.encode(binary))
}

fn locator_json(locator: &ControlLocator) -> Value {
    match locator {
        ControlLocator::Https { host, port } => {
            json!({"type": "https", "host": host, "port": port, "base_url": locator.base_url()})
        }
        ControlLocator::LoopbackDevelopment { host, port } => json!({
            "type": "loopback_development",
            "host": host.as_str(),
            "port": port,
            "base_url": locator.base_url(),
        }),
        _ => unreachable!("Ket invites carry only HTTPS and loopback locators"),
    }
}

fn valid_case(
    name: &str,
    key: &NodeSigningKey,
    locators: Vec<ControlLocator>,
    expires_at: Option<u64>,
) -> Value {
    let capsule = KetInvite::capsule(key.public_key(), locators.clone(), CODE, expires_at).unwrap();
    let invite = SignedInviteCapsule::sign(capsule, key).unwrap();
    let binary = invite.canonical_binary().unwrap();
    let text = encode_invite_text(&invite).unwrap();
    assert_eq!(decode_invite_text(&text, Some(DECODE_AT)).unwrap(), invite);
    json!({
        "name": name,
        "text": text.as_str(),
        "binary_hex": hex(&binary),
        "decode_at_epoch_seconds": DECODE_AT,
        "node_public_key_hex": hex(key.public_key().as_bytes()),
        "node_fingerprint": node_fingerprint(&key.public_key()),
        "locators": locators.iter().map(locator_json).collect::<Vec<_>>(),
        "access_code": CODE,
        "control_api_min": 1,
        "control_api_max": 1,
        "access_expires_at_epoch_seconds": expires_at,
    })
}

fn invalid_case(name: &str, text: String, now: Option<u64>, expected: InviteError) -> Value {
    assert_eq!(
        decode_invite_text(&text, now).map(|_| ()),
        Err(expected),
        "vector {name}"
    );
    json!({"name": name, "text": text, "now_epoch_seconds": now, "error": expected.code()})
}

fn signed_payload(key: &NodeSigningKey, payload: RawPayload) -> String {
    text_of(&sign_raw_payload(&payload.bytes(), key))
}

fn invalid_cases(key: &NodeSigningKey, valid_binary: &[u8]) -> Vec<Value> {
    let public = key.public_key();
    let valid = || RawPayload::valid(&public);
    let mut bad_signature = valid_binary.to_vec();
    let last = bad_signature.len() - 1;
    bad_signature[last] ^= 0x01;
    let mut trailing = valid_binary.to_vec();
    trailing.push(0x00);
    let valid_text = text_of(valid_binary);
    assert!(
        valid_text[INVITE_TEXT_PREFIX.len()..].contains(['-', '_']),
        "the standard-alphabet vector needs a URL-safe character to differ"
    );

    vec![
        invalid_case(
            "unsupported_version",
            signed_payload(key, valid().with(0, |out| codec::encode_uint(out, 2))),
            None,
            InviteError::UnsupportedVersion,
        ),
        invalid_case(
            "wrong_application",
            signed_payload(
                key,
                valid().with(1, |out| codec::encode_text(out, "org.other.app")),
            ),
            None,
            InviteError::WrongApplication,
        ),
        invalid_case(
            "unsupported_capability",
            signed_payload(key, valid().with(2, |out| codec::encode_uint(out, 2))),
            None,
            InviteError::UnsupportedCapability,
        ),
        invalid_case(
            "zero_node_key",
            signed_payload(
                key,
                valid().with(3, |out| codec::encode_bytes(out, &[0; 32])),
            ),
            None,
            InviteError::InvalidNodeKey,
        ),
        invalid_case(
            "empty_locators",
            signed_payload(key, valid().with(4, |out| codec::encode_array(out, 0))),
            None,
            InviteError::Invalid,
        ),
        invalid_case(
            "ip_literal_locator",
            signed_payload(
                key,
                valid().with(4, |out| {
                    codec::encode_array(out, 1);
                    codec::encode_array(out, 3);
                    codec::encode_uint(out, 1);
                    codec::encode_text(out, "203.0.113.9");
                    codec::encode_uint(out, 443);
                }),
            ),
            None,
            InviteError::UnsafeLocator,
        ),
        invalid_case(
            "unknown_locator_tag",
            signed_payload(
                key,
                valid().with(4, |out| {
                    codec::encode_array(out, 1);
                    codec::encode_array(out, 3);
                    codec::encode_uint(out, 9);
                    codec::encode_uint(out, 0);
                    codec::encode_uint(out, 443);
                }),
            ),
            None,
            InviteError::UnsupportedLocator,
        ),
        invalid_case(
            "duplicate_locators",
            signed_payload(
                key,
                valid().with(4, |out| {
                    codec::encode_array(out, 2);
                    for _ in 0..2 {
                        ControlLocator::https("ket.example.com", 443)
                            .unwrap()
                            .encode(out);
                    }
                }),
            ),
            None,
            InviteError::Invalid,
        ),
        invalid_case(
            "short_access_code",
            signed_payload(key, valid().with(5, |out| codec::encode_text(out, "short"))),
            None,
            InviteError::Invalid,
        ),
        invalid_case(
            "inverted_control_api_range",
            signed_payload(key, valid().with(7, |out| codec::encode_uint(out, 0))),
            None,
            InviteError::Invalid,
        ),
        invalid_case(
            "nonzero_flags",
            signed_payload(key, valid().with(8, |out| codec::encode_uint(out, 1))),
            None,
            InviteError::Invalid,
        ),
        invalid_case(
            "non_shortest_integer",
            signed_payload(
                key,
                valid().with(0, |out| out.extend_from_slice(&[0x18, 0x01])),
            ),
            None,
            InviteError::Invalid,
        ),
        invalid_case(
            "bad_signature",
            text_of(&bad_signature),
            None,
            InviteError::InvalidSignature,
        ),
        invalid_case(
            "expired",
            valid_text.clone(),
            Some(EXPIRES_AT),
            InviteError::Expired,
        ),
        invalid_case(
            "trailing_bytes",
            text_of(&trailing),
            None,
            InviteError::Invalid,
        ),
        invalid_case(
            "padded_text",
            format!("{valid_text}="),
            None,
            InviteError::Invalid,
        ),
        invalid_case(
            "standard_base64_alphabet",
            format!(
                "{INVITE_TEXT_PREFIX}{}",
                base64::engine::general_purpose::STANDARD_NO_PAD.encode(valid_binary)
            ),
            None,
            InviteError::Invalid,
        ),
        invalid_case(
            "surrounding_whitespace",
            format!(" {valid_text}"),
            None,
            InviteError::Invalid,
        ),
    ]
}

fn enrollment_vectors(key: &NodeSigningKey) -> Value {
    let nonce = EnrollmentNonce::from_bytes([0x6b; 32]);
    let body = r#"{"session_token":"vector","transports":[]}"#;
    let signature = EnrollmentProof::sign(key, &nonce, body.as_bytes());
    EnrollmentProof::verify(&key.public_key(), &nonce, body.as_bytes(), Some(&signature)).unwrap();
    let other_nonce = EnrollmentNonce::from_bytes([0x6c; 32]);
    let other_signature = EnrollmentProof::sign(
        &NodeSigningKey::from_seed(&[0x5b; 32]),
        &nonce,
        body.as_bytes(),
    );
    json!({
        "node_public_key_hex": hex(key.public_key().as_bytes()),
        "nonce": nonce.header_value(),
        "body": body,
        "signature": signature,
        "rejected": [
            {"name": "other_nonce", "nonce": other_nonce.header_value(), "body": body, "signature": signature},
            {"name": "modified_body", "nonce": nonce.header_value(), "body": body.replace("vector", "vectos"), "signature": signature},
            {"name": "other_node", "nonce": nonce.header_value(), "body": body, "signature": other_signature},
            {"name": "padded_signature", "nonce": nonce.header_value(), "body": body, "signature": format!("{signature}=")},
            {"name": "missing_signature", "nonce": nonce.header_value(), "body": body, "signature": null},
        ],
    })
}

fn generate() -> Value {
    let key = NodeSigningKey::from_seed(&NODE_SEED);
    let primary = ControlLocator::https("ket.example.com", 443).unwrap();
    let valid = vec![
        valid_case(
            "single_https",
            &key,
            vec![primary.clone()],
            Some(EXPIRES_AT),
        ),
        valid_case(
            "multiple_locators_without_expiry",
            &key,
            vec![
                ControlLocator::https("edge.example.net", 8443).unwrap(),
                primary.clone(),
                ControlLocator::LoopbackDevelopment {
                    host: LoopbackHost::AndroidEmulator,
                    port: 8787,
                },
            ],
            None,
        ),
    ];
    let primary_binary =
        KetInvite::capsule(key.public_key(), vec![primary], CODE, Some(EXPIRES_AT))
            .and_then(|capsule| SignedInviteCapsule::sign(capsule, &key))
            .and_then(|invite| invite.canonical_binary())
            .unwrap();
    json!({
        "format": "ket-invite-capsule-v1-conformance",
        "node_seed_hex": hex(&NODE_SEED),
        "valid": valid,
        "invalid": invalid_cases(&key, &primary_binary),
        "enrollment": enrollment_vectors(&key),
    })
}

#[test]
fn committed_vectors_match_the_reference_implementation() {
    let generated = serde_json::to_string_pretty(&generate()).unwrap() + "\n";
    if std::env::var_os("KET_UPDATE_INVITE_VECTORS").is_some() {
        std::fs::write(VECTOR_PATH, &generated).unwrap();
    }
    let committed = std::fs::read_to_string(VECTOR_PATH)
        .expect("run with KET_UPDATE_INVITE_VECTORS=1 to create the vector file");
    assert_eq!(
        committed, generated,
        "invite vectors drifted; review the format change and regenerate"
    );
}
