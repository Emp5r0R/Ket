//! Sealed requests: `version || enc || ciphertext`, where the version byte is
//! also the AEAD's associated data.

use hpke::{Deserializable, OpModeR, OpModeS, Serializable};

use crate::{
    Aead, ENCAPSULATED_KEY_BYTES, Kdf, Kem, MAX_SEALED_BYTES, NodeOpeningKey, NodeSealingKey,
    RESPONSE_EXPORT_LABEL, ResponseOpener, ResponseSealer, SEAL_VERSION, SealError, info,
    response::ResponseSecret,
};

/// Seals `plaintext` for `node`. Keep the opener to read the response.
pub fn seal_request(
    node: &NodeSealingKey,
    plaintext: &[u8],
) -> Result<(Vec<u8>, ResponseOpener), SealError> {
    let (encapsulated, mut context) =
        hpke::setup_sender::<Aead, Kdf, Kem>(&OpModeS::Base, &node.x25519, &info(&node.ed25519))
            .map_err(|_| SealError)?;
    let ciphertext = context
        .seal(plaintext, &[SEAL_VERSION])
        .map_err(|_| SealError)?;
    let enc: [u8; ENCAPSULATED_KEY_BYTES] = encapsulated.to_bytes().into();
    let mut sealed = Vec::with_capacity(1 + ENCAPSULATED_KEY_BYTES + ciphertext.len());
    sealed.push(SEAL_VERSION);
    sealed.extend_from_slice(&enc);
    sealed.extend_from_slice(&ciphertext);
    if sealed.len() > MAX_SEALED_BYTES {
        return Err(SealError);
    }
    let mut secret = ResponseSecret::default();
    context
        .export(RESPONSE_EXPORT_LABEL, secret.as_mut())
        .map_err(|_| SealError)?;
    Ok((sealed, ResponseOpener::new(enc, secret)))
}

/// A request the node opened, with the sealer for its one response.
pub struct RequestOpened {
    pub plaintext: Vec<u8>,
    pub sealer: ResponseSealer,
    replay_id: [u8; ENCAPSULATED_KEY_BYTES],
}

impl RequestOpened {
    /// Unique per sealed request (its encapsulated key), so a node can refuse
    /// a request it has already answered.
    pub fn replay_id(&self) -> [u8; 32] {
        self.replay_id
    }
}

impl std::fmt::Debug for RequestOpened {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("RequestOpened(..)")
    }
}

pub fn open_request(key: &NodeOpeningKey, sealed: &[u8]) -> Result<RequestOpened, SealError> {
    if sealed.len() > MAX_SEALED_BYTES || sealed.len() <= 1 + ENCAPSULATED_KEY_BYTES {
        return Err(SealError);
    }
    let (version, rest) = sealed.split_at(1);
    if version != [SEAL_VERSION] {
        return Err(SealError);
    }
    let (enc_bytes, ciphertext) = rest.split_at(ENCAPSULATED_KEY_BYTES);
    let encapsulated =
        <Kem as hpke::Kem>::EncappedKey::from_bytes(enc_bytes).map_err(|_| SealError)?;
    let mut context = hpke::setup_receiver::<Aead, Kdf, Kem>(
        &OpModeR::Base,
        &key.x25519,
        &encapsulated,
        &info(&key.ed25519),
    )
    .map_err(|_| SealError)?;
    let plaintext = context
        .open(ciphertext, &[SEAL_VERSION])
        .map_err(|_| SealError)?;
    let mut secret = ResponseSecret::default();
    context
        .export(RESPONSE_EXPORT_LABEL, secret.as_mut())
        .map_err(|_| SealError)?;
    let enc: [u8; ENCAPSULATED_KEY_BYTES] = enc_bytes.try_into().map_err(|_| SealError)?;
    Ok(RequestOpened {
        plaintext,
        sealer: ResponseSealer::new(enc, secret),
        replay_id: enc,
    })
}
