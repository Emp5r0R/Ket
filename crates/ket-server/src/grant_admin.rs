//! Applies an admin's changes to one access grant inside the persisted state
//! and decides which of its sessions the change ends.
//!
//! A session ends when the grant no longer admits it as it is: the grant was
//! paused, its protocols changed (the session was issued credentials for the
//! old set), its allowance is now used up, or it has more devices connected
//! than the new limit (the newest go first). A shorter lifetime ends no
//! session; each one is cut to the new end instead. An allowance with a
//! different period, or none, starts the count again. The caller has
//! validated the values themselves.

use std::collections::HashSet;

use ket_core::UpdateAccessGrantRequest;

use crate::{
    allowance,
    model::PersistedState,
    service::{ServiceError, SessionAllocation, session_allocation},
};

pub(crate) fn apply(
    state: &mut PersistedState,
    id: &str,
    changes: &UpdateAccessGrantRequest,
    now: u64,
) -> Result<Vec<SessionAllocation>, ServiceError> {
    let grant = state
        .grants
        .iter_mut()
        .find(|grant| grant.id == id)
        .ok_or(ServiceError::NotFound)?;
    let mut end_all = false;
    if let Some(label) = &changes.label {
        grant.label = label.clone();
    }
    if let Some(paused) = changes.paused {
        end_all |= paused && !grant.paused;
        grant.paused = paused;
    }
    if let Some(protocols) = &changes.protocols {
        end_all |= *protocols != grant.protocols;
        grant.protocols = protocols.clone();
    }
    if let Some(max_connections) = changes.max_connections {
        grant.max_connections = max_connections;
    }
    if let Some(expires_at) = changes.expires_at_epoch_seconds {
        grant.expires_at_epoch_seconds = Some(expires_at);
    }
    if let Some(new) = changes.allowance {
        if new.map(|new| new.period) != grant.allowance.map(|old| old.period) {
            grant.usage = None;
            grant.allowance_since_epoch_seconds = new.map(|_| now);
        }
        grant.allowance = new;
    }
    if changes.reset_usage {
        grant.usage = None;
    }
    end_all |= allowance::used_up_until(grant, now).is_some();
    let max_connections = grant.max_connections as usize;
    let expires_at = grant.expires_at_epoch_seconds;

    let mut sessions = state
        .sessions
        .iter_mut()
        .filter(|session| session.grant_id == id)
        .collect::<Vec<_>>();
    // Oldest first, so a lower device limit keeps the devices that were
    // connected before the others.
    sessions.sort_by_key(|session| (session.issued_at_epoch_seconds, session.id.clone()));
    let mut ending = HashSet::new();
    let mut active = 0;
    for session in sessions {
        if let Some(expires_at) = expires_at {
            session.expires_at_epoch_seconds = session.expires_at_epoch_seconds.min(expires_at);
        }
        let live = session.expires_at_epoch_seconds > now;
        if end_all || (live && active >= max_connections) {
            ending.insert(session.id.clone());
        } else if live {
            active += 1;
        }
    }
    let ended = state
        .sessions
        .iter()
        .filter(|session| ending.contains(&session.id))
        .map(session_allocation)
        .collect::<Result<Vec<_>, _>>()?;
    state
        .sessions
        .retain(|session| !ending.contains(&session.id));
    Ok(ended)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use ket_core::{AllowancePeriod, DataAllowance, ProtocolAccess, TransportProtocol};

    use super::*;
    use crate::model::{AccessGrantRecord, CURRENT_STATE_SCHEMA_VERSION, SessionRecord};

    const NOW: u64 = 1_000;
    const VALID_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHQxMjM0NTY3OA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    #[test]
    fn pausing_ends_every_session_and_resuming_ends_none() {
        let mut state = state_with_sessions(3);
        let ended = apply(&mut state, "Grant00001", &paused(true), NOW).unwrap();
        assert_eq!(ended.len(), 3);
        assert!(
            state
                .sessions
                .iter()
                .all(|session| session.grant_id != "Grant00001")
        );
        assert!(state.grants[0].paused);
        assert_eq!(state.sessions.len(), 1, "another grant's session stays");

        assert!(
            apply(&mut state, "Grant00001", &paused(true), NOW)
                .unwrap()
                .is_empty()
        );
        assert!(
            apply(&mut state, "Grant00001", &paused(false), NOW)
                .unwrap()
                .is_empty()
        );
        assert!(!state.grants[0].paused);
        assert_eq!(state.validate(), Ok(()));
    }

    #[test]
    fn changing_protocols_ends_the_grants_sessions_and_keeping_them_does_not() {
        let mut state = state_with_sessions(2);
        let same = UpdateAccessGrantRequest {
            protocols: Some(ProtocolAccess::All),
            ..Default::default()
        };
        assert!(
            apply(&mut state, "Grant00001", &same, NOW)
                .unwrap()
                .is_empty()
        );

        let only = ProtocolAccess::Only(BTreeSet::from([TransportProtocol::Stealth]));
        let narrow = UpdateAccessGrantRequest {
            protocols: Some(only.clone()),
            ..Default::default()
        };
        assert_eq!(
            apply(&mut state, "Grant00001", &narrow, NOW).unwrap().len(),
            2
        );
        assert_eq!(state.grants[0].protocols, only);
        assert_eq!(state.validate(), Ok(()));
    }

    #[test]
    fn a_lower_device_limit_disconnects_the_newest_devices() {
        let mut state = state_with_sessions(3);
        let fewer = UpdateAccessGrantRequest {
            max_connections: Some(1),
            ..Default::default()
        };
        let ended = apply(&mut state, "Grant00001", &fewer, NOW).unwrap();
        let ended = ended
            .into_iter()
            .map(|session| session.id)
            .collect::<Vec<_>>();
        assert_eq!(ended, ["Session00002", "Session00003"]);
        assert!(
            state
                .sessions
                .iter()
                .any(|session| session.id == "Session00001")
        );
        assert_eq!(state.grants[0].max_connections, 1);

        let more = UpdateAccessGrantRequest {
            max_connections: Some(5),
            ..Default::default()
        };
        assert!(
            apply(&mut state, "Grant00001", &more, NOW)
                .unwrap()
                .is_empty()
        );
        assert_eq!(state.validate(), Ok(()));
    }

    #[test]
    fn a_shorter_lifetime_cuts_sessions_to_the_new_end_and_a_longer_one_keeps_them() {
        let mut state = state_with_sessions(2);
        let shorter = UpdateAccessGrantRequest {
            expires_at_epoch_seconds: Some(NOW + 50),
            ..Default::default()
        };
        assert!(
            apply(&mut state, "Grant00001", &shorter, NOW)
                .unwrap()
                .is_empty()
        );
        assert!(
            state
                .sessions
                .iter()
                .filter(|session| session.grant_id == "Grant00001")
                .all(|session| session.expires_at_epoch_seconds == NOW + 50)
        );
        assert_eq!(state.validate(), Ok(()));

        let longer = UpdateAccessGrantRequest {
            expires_at_epoch_seconds: Some(NOW + 90_000),
            label: Some("Renamed".to_owned()),
            ..Default::default()
        };
        assert!(
            apply(&mut state, "Grant00001", &longer, NOW)
                .unwrap()
                .is_empty()
        );
        assert_eq!(state.grants[0].expires_at_epoch_seconds, Some(NOW + 90_000));
        assert_eq!(state.grants[0].label, "Renamed");
        assert_eq!(state.validate(), Ok(()));
    }

    #[test]
    fn an_allowance_below_what_is_used_ends_the_sessions_and_a_reset_lets_them_back() {
        let mut state = state_with_sessions(2);
        let daily = |bytes| DataAllowance {
            bytes,
            period: AllowancePeriod::Day,
        };
        let set = |allowance| UpdateAccessGrantRequest {
            allowance: Some(allowance),
            ..Default::default()
        };
        assert!(
            apply(&mut state, "Grant00001", &set(Some(daily(4 << 20))), NOW)
                .unwrap()
                .is_empty()
        );
        assert_eq!(state.grants[0].allowance_since_epoch_seconds, Some(NOW));
        crate::allowance::add(&mut state.grants[0], 3 << 20, NOW);

        // A larger amount keeps the count; a smaller one than is used ends
        // every session.
        assert!(
            apply(&mut state, "Grant00001", &set(Some(daily(8 << 20))), NOW)
                .unwrap()
                .is_empty()
        );
        assert_eq!(crate::allowance::used(&state.grants[0], NOW), 3 << 20);
        // Its counting started when the allowance was first set.
        assert_eq!(state.grants[0].allowance_since_epoch_seconds, Some(NOW));
        assert_eq!(
            apply(&mut state, "Grant00001", &set(Some(daily(2 << 20))), NOW)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(state.validate(), Ok(()));

        let reset = UpdateAccessGrantRequest {
            reset_usage: true,
            ..Default::default()
        };
        apply(&mut state, "Grant00001", &reset, NOW).unwrap();
        assert_eq!(crate::allowance::used_up_until(&state.grants[0], NOW), None);

        // A new period, or no allowance, starts again from nothing.
        crate::allowance::add(&mut state.grants[0], 3 << 20, NOW);
        let weekly = DataAllowance {
            period: AllowancePeriod::Week,
            ..daily(2 << 20)
        };
        apply(&mut state, "Grant00001", &set(Some(weekly)), NOW).unwrap();
        assert_eq!(state.grants[0].usage, None);
        crate::allowance::add(&mut state.grants[0], 1 << 20, NOW);
        apply(&mut state, "Grant00001", &set(None), NOW).unwrap();
        assert_eq!(
            (
                state.grants[0].allowance,
                state.grants[0].allowance_since_epoch_seconds,
                state.grants[0].usage
            ),
            (None, None, None)
        );
        assert_eq!(state.validate(), Ok(()));
    }

    #[test]
    fn an_unknown_grant_changes_nothing() {
        let mut state = state_with_sessions(1);
        assert!(matches!(
            apply(&mut state, "Missing001", &paused(true), NOW),
            Err(ServiceError::NotFound)
        ));
        assert_eq!(state.sessions.len(), 2);
    }

    fn paused(paused: bool) -> UpdateAccessGrantRequest {
        UpdateAccessGrantRequest {
            paused: Some(paused),
            ..Default::default()
        }
    }

    /// `count` sessions on Grant00001, connected one after another, and one
    /// on Grant00002.
    fn state_with_sessions(count: u32) -> PersistedState {
        let grant = |id: &str| AccessGrantRecord {
            id: id.to_owned(),
            secret_hash: VALID_HASH.to_owned(),
            label: "Family".to_owned(),
            max_connections: 4,
            expires_at_epoch_seconds: Some(NOW + 10_000),
            created_at_epoch_seconds: 100,
            paused: false,
            protocols: ProtocolAccess::All,
            allowance: None,
            allowance_since_epoch_seconds: None,
            usage: None,
        };
        let session = |index: u32, grant_id: &str| SessionRecord {
            id: format!("Session{index:05}"),
            grant_id: grant_id.to_owned(),
            secret_hash: None,
            control_secret_hash: Some([index as u8; 32]),
            data_plane_secret_hash: Some([index as u8; 32]),
            resource_slot: Some(index),
            client_name: format!("Device {index}"),
            issued_at_epoch_seconds: 100 + u64::from(index),
            expires_at_epoch_seconds: NOW + 3_600,
        };
        let mut sessions = (1..=count)
            .map(|index| session(index, "Grant00001"))
            .collect::<Vec<_>>();
        sessions.push(session(99, "Grant00002"));
        PersistedState {
            schema_version: CURRENT_STATE_SCHEMA_VERSION,
            grants: vec![grant("Grant00001"), grant("Grant00002")],
            sessions,
        }
    }
}
