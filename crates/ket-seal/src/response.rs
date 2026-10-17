//! Sealed responses (RFC 9458, section 4.4): `response_nonce || ciphertext`.
//! The AEAD key and nonce come from HKDF over the request's exported secret,
//! salted with `enc || response_nonce`, so answering a replayed request never
//! reuses a key and nonce pair.

use chacha20poly1305::{
    ChaCha20Poly1305, KeyInit,
    aead::{Aead as _, Payload},
};
use hkdf::Hkdf;
use rand::{RngCore, rngs::OsRng};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::{ENCAPSULATED_KEY_BYTES, MAX_SEALED_BYTES, SealError};

pub const RESPONSE_SECRET_BYTES: usize = 32;
const RESPONSE_NONCE_BYTES: usize = 32;
const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;

#[derive(Default)]
pub(crate) struct ResponseSecret(Zeroizing<[u8; RESPONSE_SECRET_BYTES]>);

impl ResponseSecret {
    pub(crate) fn as_mut(&mut self) -> &mut [u8] {
        self.0.as_mut_slice()
    }
}

/// The node's half: seals exactly one response, then is gone.
pub struct ResponseSealer {
    enc: [u8; ENCAPSULATED_KEY_BYTES],
    secret: ResponseSecret,
}

impl ResponseSealer {
    pub(crate) fn new(enc: [u8; ENCAPSULATED_KEY_BYTES], secret: ResponseSecret) -> Self {
        Self { enc, secret }
    }

    pub fn seal(self, plaintext: &[u8]) -> Result<Vec<u8>, SealError> {
        let mut response_nonce = [0_u8; RESPONSE_NONCE_BYTES];
        OsRng.fill_bytes(&mut response_nonce);
        let (cipher, nonce) = cipher(&self.enc, &self.secret, &response_nonce)?;
        let ciphertext = cipher
            .encrypt(&nonce.into(), plaintext)
            .map_err(|_| SealError)?;
        let mut sealed = Vec::with_capacity(RESPONSE_NONCE_BYTES + ciphertext.len());
        sealed.extend_from_slice(&response_nonce);
        sealed.extend_from_slice(&ciphertext);
        if sealed.len() > MAX_SEALED_BYTES {
            return Err(SealError);
        }
        Ok(sealed)
    }
}

impl std::fmt::Debug for ResponseSealer {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ResponseSealer(..)")
    }
}

/// The app's half: opens the response to the request it sealed.
pub struct ResponseOpener {
    enc: [u8; ENCAPSULATED_KEY_BYTES],
    secret: ResponseSecret,
}

impl ResponseOpener {
    pub(crate) fn new(enc: [u8; ENCAPSULATED_KEY_BYTES], secret: ResponseSecret) -> Self {
        Self { enc, secret }
    }

    /// `enc || secret`, for apps that keep the opener outside Rust between
    /// sealing and opening (Android over JNI). Treat it as a key.
    pub fn to_bytes(&self) -> Zeroizing<Vec<u8>> {
        let mut bytes = Zeroizing::new(Vec::with_capacity(
            ENCAPSULATED_KEY_BYTES + RESPONSE_SECRET_BYTES,
        ));
        bytes.extend_from_slice(&self.enc);
        bytes.extend_from_slice(self.secret.0.as_slice());
        bytes
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SealError> {
        if bytes.len() != ENCAPSULATED_KEY_BYTES + RESPONSE_SECRET_BYTES {
            return Err(SealError);
        }
        let (enc, secret_bytes) = bytes.split_at(ENCAPSULATED_KEY_BYTES);
        let mut secret = ResponseSecret::default();
        secret.as_mut().copy_from_slice(secret_bytes);
        Ok(Self {
            enc: enc.try_into().map_err(|_| SealError)?,
            secret,
        })
    }

    pub fn open(&self, sealed: &[u8]) -> Result<Vec<u8>, SealError> {
        if sealed.len() > MAX_SEALED_BYTES || sealed.len() < RESPONSE_NONCE_BYTES {
            return Err(SealError);
        }
        let (response_nonce, ciphertext) = sealed.split_at(RESPONSE_NONCE_BYTES);
        let (cipher, nonce) = cipher(&self.enc, &self.secret, response_nonce)?;
        cipher
            .decrypt(
                &nonce.into(),
                Payload {
                    msg: ciphertext,
                    aad: &[],
                },
            )
            .map_err(|_| SealError)
    }
}

impl std::fmt::Debug for ResponseOpener {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ResponseOpener(..)")
    }
}

fn cipher(
    enc: &[u8; ENCAPSULATED_KEY_BYTES],
    secret: &ResponseSecret,
    response_nonce: &[u8],
) -> Result<(ChaCha20Poly1305, [u8; NONCE_BYTES]), SealError> {
    let mut salt = Vec::with_capacity(ENCAPSULATED_KEY_BYTES + RESPONSE_NONCE_BYTES);
    salt.extend_from_slice(enc);
    salt.extend_from_slice(response_nonce);
    let hkdf = Hkdf::<Sha256>::new(Some(&salt), secret.0.as_slice());
    let mut key = Zeroizing::new([0_u8; KEY_BYTES]);
    let mut nonce = [0_u8; NONCE_BYTES];
    hkdf.expand(b"key", key.as_mut_slice())
        .and_then(|()| hkdf.expand(b"nonce", &mut nonce))
        .map_err(|_| SealError)?;
    let cipher = ChaCha20Poly1305::new_from_slice(key.as_slice()).map_err(|_| SealError)?;
    Ok((cipher, nonce))
}
