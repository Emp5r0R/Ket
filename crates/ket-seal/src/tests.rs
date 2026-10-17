use super::*;

fn node() -> NodeOpeningKey {
    NodeOpeningKey::from_ed25519_seed(&[0x42; 32])
}

fn pinned(node: &NodeOpeningKey) -> NodeSealingKey {
    let public = ed25519_dalek::SigningKey::from_bytes(&[0x42; 32])
        .verifying_key()
        .to_bytes();
    let sealing = NodeSealingKey::from_ed25519(&public).unwrap();
    assert_eq!(sealing.ed25519, node.ed25519);
    sealing
}

#[test]
fn a_request_and_its_response_round_trip() {
    let node = node();
    let (sealed, opener) = seal_request(&pinned(&node), b"request").unwrap();
    assert_eq!(sealed[0], SEAL_VERSION);
    assert!(!sealed.windows(7).any(|window| window == b"request"));
    let opened = open_request(&node, &sealed).unwrap();
    assert_eq!(opened.plaintext, b"request");
    let response = opened.sealer.seal(b"response").unwrap();
    assert!(!response.windows(8).any(|window| window == b"response"));
    assert_eq!(opener.open(&response).unwrap(), b"response");
}

#[test]
fn the_converted_public_key_matches_the_node_private_key() {
    let node = node();
    let (sealed, _) = seal_request(&node.sealing_key(), b"x").unwrap();
    open_request(&node, &sealed).unwrap();
    let (sealed, _) = seal_request(&pinned(&node), b"x").unwrap();
    open_request(&node, &sealed).unwrap();
}

#[test]
fn another_node_cannot_open_a_request() {
    let (sealed, _) = seal_request(&pinned(&node()), b"request").unwrap();
    let other = NodeOpeningKey::from_ed25519_seed(&[0x43; 32]);
    assert_eq!(open_request(&other, &sealed).unwrap_err(), SealError);
}

#[test]
fn tampered_or_truncated_requests_are_refused() {
    let node = node();
    let (sealed, _) = seal_request(&pinned(&node), b"request").unwrap();
    for index in [0, 1, 20, 33, sealed.len() - 1] {
        let mut tampered = sealed.clone();
        tampered[index] ^= 1;
        assert!(open_request(&node, &tampered).is_err(), "byte {index}");
    }
    for length in [0, 1, 33, sealed.len() - 1] {
        assert!(
            open_request(&node, &sealed[..length]).is_err(),
            "length {length}"
        );
    }
    assert!(open_request(&node, &vec![SEAL_VERSION; MAX_SEALED_BYTES + 1]).is_err());
}

#[test]
fn responses_open_only_for_their_own_request() {
    let node = node();
    let sealing = pinned(&node);
    let (first, first_opener) = seal_request(&sealing, b"one").unwrap();
    let (_, second_opener) = seal_request(&sealing, b"two").unwrap();
    let response = open_request(&node, &first)
        .unwrap()
        .sealer
        .seal(b"answer")
        .unwrap();
    assert!(second_opener.open(&response).is_err());
    let mut tampered = response.clone();
    *tampered.last_mut().unwrap() ^= 1;
    assert!(first_opener.open(&tampered).is_err());
    assert!(first_opener.open(&response[..31]).is_err());
    assert_eq!(first_opener.open(&response).unwrap(), b"answer");
}

#[test]
fn answering_a_replayed_request_uses_a_fresh_response_nonce() {
    let node = node();
    let (sealed, opener) = seal_request(&pinned(&node), b"request").unwrap();
    let first_open = open_request(&node, &sealed).unwrap();
    let second_open = open_request(&node, &sealed).unwrap();
    assert_eq!(first_open.replay_id(), second_open.replay_id());
    let (other, _) = seal_request(&pinned(&node), b"request").unwrap();
    assert_ne!(
        open_request(&node, &other).unwrap().replay_id(),
        first_open.replay_id()
    );
    let first = first_open.sealer.seal(b"same").unwrap();
    let second = second_open.sealer.seal(b"same").unwrap();
    assert_ne!(first, second);
    assert_eq!(opener.open(&first).unwrap(), b"same");
    assert_eq!(opener.open(&second).unwrap(), b"same");
}

#[test]
fn openers_survive_a_round_trip_through_bytes() {
    let node = node();
    let (sealed, opener) = seal_request(&pinned(&node), b"request").unwrap();
    let restored = ResponseOpener::from_bytes(&opener.to_bytes()).unwrap();
    let response = open_request(&node, &sealed)
        .unwrap()
        .sealer
        .seal(b"answer")
        .unwrap();
    assert_eq!(restored.open(&response).unwrap(), b"answer");
    assert!(ResponseOpener::from_bytes(&[0; 63]).is_err());
}

#[test]
fn invalid_node_keys_are_refused() {
    // The identity point has small order.
    let mut identity = [0_u8; 32];
    identity[0] = 1;
    assert!(NodeSealingKey::from_ed25519(&identity).is_err());
    // y = 2 is not on the curve.
    let mut off_curve = [0_u8; 32];
    off_curve[0] = 2;
    assert!(NodeSealingKey::from_ed25519(&off_curve).is_err());
}

#[test]
fn debug_output_hides_key_material() {
    let node = node();
    let (_, opener) = seal_request(&pinned(&node), b"x").unwrap();
    for text in [
        format!("{node:?}"),
        format!("{:?}", node.sealing_key()),
        format!("{opener:?}"),
    ] {
        assert!(text.ends_with("(..)"), "{text}");
    }
}
