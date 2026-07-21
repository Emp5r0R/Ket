use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OpenVpnSessionStatus {
    pub username: String,
    pub virtual_address: String,
    pub connected_since_epoch_seconds: u64,
    pub bytes_received: u64,
    pub bytes_sent: u64,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ReconcileOpenVpnSessions {
    pub usernames: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct RemoveOpenVpnSessions {
    pub usernames: Vec<String>,
}

pub(crate) fn valid_session_username(username: &str) -> bool {
    username.len() == 12 && username.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_usernames_have_the_control_plane_id_shape() {
        assert!(valid_session_username("AbCdEf123456"));
        assert!(!valid_session_username("short"));
        assert!(!valid_session_username("AbCdEf12345-"));
    }
}
