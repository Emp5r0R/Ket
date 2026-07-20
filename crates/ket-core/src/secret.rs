use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use blake2::{Blake2s256, Digest};
use rand::{Rng, distributions::Alphanumeric};
use subtle::ConstantTimeEq;
use thiserror::Error;

pub const ACCESS_CODE_LENGTH: usize = 32;
pub const SESSION_TOKEN_LENGTH: usize = 44;
const ACCESS_ID_LENGTH: usize = 10;
const SESSION_ID_LENGTH: usize = 12;
const SESSION_SECRET_LENGTH: usize = SESSION_TOKEN_LENGTH - SESSION_ID_LENGTH;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccessCodeParts {
    pub id: String,
    pub secret: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionTokenParts {
    pub id: String,
    pub secret: String,
}

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("secret has an invalid format")]
    InvalidFormat,
    #[error("secret hashing failed")]
    Hash,
}

pub fn generate_access_code() -> String {
    random_alphanumeric(ACCESS_CODE_LENGTH)
}

pub fn generate_session_token() -> String {
    random_alphanumeric(SESSION_TOKEN_LENGTH)
}

pub fn generate_scoped_token(session_id: &str) -> Result<String, SecretError> {
    validate_alphanumeric(session_id, SESSION_ID_LENGTH)?;
    Ok(format!(
        "{session_id}{}",
        random_alphanumeric(SESSION_SECRET_LENGTH)
    ))
}

pub fn split_access_code(value: &str) -> Result<AccessCodeParts, SecretError> {
    validate_alphanumeric(value, ACCESS_CODE_LENGTH)?;
    Ok(AccessCodeParts {
        id: value[..ACCESS_ID_LENGTH].to_owned(),
        secret: value[ACCESS_ID_LENGTH..].to_owned(),
    })
}

pub fn split_session_token(value: &str) -> Result<SessionTokenParts, SecretError> {
    validate_alphanumeric(value, SESSION_TOKEN_LENGTH)?;
    Ok(SessionTokenParts {
        id: value[..SESSION_ID_LENGTH].to_owned(),
        secret: value[SESSION_ID_LENGTH..].to_owned(),
    })
}

pub fn hash_secret(secret: &str) -> Result<String, SecretError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(secret.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| SecretError::Hash)
}

pub fn verify_secret(secret: &str, encoded_hash: &str) -> bool {
    let Ok(hash) = PasswordHash::new(encoded_hash) else {
        return false;
    };
    Argon2::default()
        .verify_password(secret.as_bytes(), &hash)
        .is_ok()
}

/// Accepts only the exact Argon2id profile emitted by [`hash_secret`].
pub fn is_supported_secret_hash(encoded_hash: &str) -> bool {
    let Ok(hash) = PasswordHash::new(encoded_hash) else {
        return false;
    };
    hash.algorithm.as_str() == "argon2id"
        && hash.version == Some(19)
        && hash.params.iter().count() == 3
        && hash.params.get_decimal("m") == Some(19 * 1024)
        && hash.params.get_decimal("t") == Some(2)
        && hash.params.get_decimal("p") == Some(1)
        && hash.salt.is_some()
        && hash.hash.is_some_and(|output| output.len() == 32)
}

/// Fast hashing for machine-generated tokens with approximately 190 bits of entropy.
pub fn hash_token_secret(secret: &str) -> [u8; 32] {
    Blake2s256::digest(secret.as_bytes()).into()
}

pub fn verify_token_secret(secret: &str, expected_hash: &[u8; 32]) -> bool {
    bool::from(hash_token_secret(secret).ct_eq(expected_hash))
}

fn random_alphanumeric(length: usize) -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(length)
        .map(char::from)
        .collect()
}

fn validate_alphanumeric(value: &str, expected_length: usize) -> Result<(), SecretError> {
    if value.len() != expected_length || !value.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return Err(SecretError::InvalidFormat);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn access_codes_have_the_required_shape() {
        let code = generate_access_code();
        let parts = split_access_code(&code).expect("generated code must parse");

        assert_eq!(code.len(), ACCESS_CODE_LENGTH);
        assert_eq!(parts.id.len(), ACCESS_ID_LENGTH);
        assert_eq!(format!("{}{}", parts.id, parts.secret), code);
    }

    #[test]
    fn malformed_codes_are_rejected() {
        assert!(split_access_code("short").is_err());
        assert!(split_access_code("!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!").is_err());
    }

    #[test]
    fn hashes_verify_without_retaining_plaintext() {
        let hash = hash_secret("high-entropy-secret").expect("hashing should work");

        assert!(is_supported_secret_hash(&hash));
        assert!(verify_secret("high-entropy-secret", &hash));
        assert!(!verify_secret("different-secret", &hash));
        assert!(!hash.contains("high-entropy-secret"));
    }

    #[test]
    fn secret_hash_policy_rejects_unbounded_or_downgraded_profiles() {
        let hash = hash_secret("high-entropy-secret").expect("hashing should work");

        assert!(!is_supported_secret_hash(&hash.replacen(
            "m=19456",
            "m=4294967295",
            1
        )));
        assert!(!is_supported_secret_hash(
            &hash.replacen("argon2id", "argon2i", 1)
        ));
        assert!(!is_supported_secret_hash("not-a-password-hash"));
    }

    #[test]
    fn scoped_tokens_share_only_the_public_session_id() {
        let session = split_session_token(&generate_session_token()).expect("session token");
        let scoped = generate_scoped_token(&session.id).expect("scoped token");
        let scoped = split_session_token(&scoped).expect("scoped token should parse");

        assert_eq!(scoped.id, session.id);
        assert_ne!(scoped.secret, session.secret);
        let hash = hash_token_secret(&scoped.secret);
        assert!(verify_token_secret(&scoped.secret, &hash));
        assert!(!verify_token_secret(&session.secret, &hash));
    }
}
