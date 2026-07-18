//! Shared, platform-neutral Ket contracts and security primitives.

mod contract;
mod secret;
mod secret_value;

pub use contract::*;
pub use secret::{
    ACCESS_CODE_LENGTH, AccessCodeParts, SESSION_TOKEN_LENGTH, SecretError, SessionTokenParts,
    generate_access_code, generate_scoped_token, generate_session_token, hash_secret,
    hash_token_secret, split_access_code, split_session_token, verify_secret, verify_token_secret,
};
pub use secret_value::SecretString;
