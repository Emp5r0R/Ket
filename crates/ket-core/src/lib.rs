//! Shared, platform-neutral Ket contracts and security primitives.

mod contract;
mod grant;
mod sealed;
mod secret;
mod secret_value;
mod update;

pub use contract::*;
pub use grant::{
    AccessGrantDetails, ConnectedDevice, GrantActivity, GrantActivityEvent, OfferedProtocol,
    ProtocolAccess, UpdateAccessGrantRequest, UpdateAccessGrantResponse,
};
pub use sealed::{
    SEALED_CLOCK_SKEW_CODE, SEALED_CONTENT_TYPE, SEALED_CONTROL_PATH,
    SEALED_MAX_CLOCK_SKEW_SECONDS, SEALED_REPLAY_CODE, SealedControlRequest, SealedControlResponse,
    SealedMethod,
};
pub use secret::{
    ACCESS_CODE_LENGTH, AccessCodeParts, SESSION_TOKEN_LENGTH, SecretError, SessionTokenParts,
    generate_access_code, generate_scoped_token, generate_session_token, hash_secret,
    hash_token_secret, is_supported_secret_hash, split_access_code, split_session_token,
    verify_secret, verify_token_secret,
};
pub use secret_value::SecretString;
pub use update::{
    CLIENT_HEADER, CLIENT_UPDATE_REQUIRED_CODE, ClientUpdate, ClientUpdateStatus, UpdateAdvice,
};
