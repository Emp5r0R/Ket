//! What devices recently did with each access grant, for `ket invites show`.
//!
//! The log lives in memory only: it is never written to disk, starts empty
//! when the server restarts, and forgets a grant's entries when the grant is
//! revoked or forgotten. Entries name the app and its session, never an
//! address. Repeats of the same thing by the same device are counted in one
//! entry, so a device that checks in every few minutes keeps one line.

use std::collections::{HashMap, VecDeque};

use ket_core::{GrantActivity, GrantActivityEvent, TransportProtocol};

/// Entries kept per grant; the oldest goes first.
pub(crate) const MAX_ENTRIES_PER_GRANT: usize = 24;

/// One thing that happened, before it is counted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Activity {
    pub event: GrantActivityEvent,
    pub session_id: Option<String>,
    pub client_name: String,
    pub protocol: Option<TransportProtocol>,
}

impl Activity {
    /// Something a device did or had done to it through its session.
    pub(crate) fn session(event: GrantActivityEvent, session_id: &str, client_name: &str) -> Self {
        Self {
            event,
            session_id: Some(session_id.to_owned()),
            client_name: client_name.to_owned(),
            protocol: None,
        }
    }

    /// An enrollment the grant refused, before any session existed.
    pub(crate) fn enrollment(event: GrantActivityEvent, client_name: &str) -> Self {
        Self {
            event,
            session_id: None,
            client_name: client_name.to_owned(),
            protocol: None,
        }
    }

    pub(crate) fn over(mut self, protocol: TransportProtocol) -> Self {
        self.protocol = Some(protocol);
        self
    }

    fn same_as(&self, entry: &GrantActivity) -> bool {
        self.event == entry.event
            && self.session_id == entry.session_id
            && self.client_name == entry.client_name
            && self.protocol == entry.protocol
    }
}

#[derive(Debug, Default)]
pub(crate) struct ActivityLog {
    grants: HashMap<String, VecDeque<GrantActivity>>,
}

impl ActivityLog {
    pub(crate) fn record(&mut self, grant_id: &str, activity: Activity, now: u64) {
        let entries = self.grants.entry(grant_id.to_owned()).or_default();
        if let Some(position) = entries.iter().position(|entry| activity.same_as(entry))
            && let Some(mut entry) = entries.remove(position)
        {
            entry.count = entry.count.saturating_add(1);
            entry.last_at_epoch_seconds = now;
            entries.push_back(entry);
            return;
        }
        if entries.len() >= MAX_ENTRIES_PER_GRANT {
            entries.pop_front();
        }
        entries.push_back(GrantActivity {
            event: activity.event,
            session_id: activity.session_id,
            client_name: activity.client_name,
            protocol: activity.protocol,
            count: 1,
            first_at_epoch_seconds: now,
            last_at_epoch_seconds: now,
        });
    }

    /// The grant's entries, newest first.
    pub(crate) fn recent(&self, grant_id: &str) -> Vec<GrantActivity> {
        self.grants
            .get(grant_id)
            .map(|entries| entries.iter().rev().cloned().collect())
            .unwrap_or_default()
    }

    /// Forgets every grant `keep` refuses.
    pub(crate) fn retain(&mut self, keep: impl Fn(&str) -> bool) {
        self.grants.retain(|grant_id, _| keep(grant_id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checked_in(session: &str) -> Activity {
        Activity::session(GrantActivityEvent::CheckedIn, session, "android/1.2.0")
    }

    #[test]
    fn repeats_are_counted_in_one_entry_that_moves_to_the_front() {
        let mut log = ActivityLog::default();
        log.record("grant", checked_in("aaaa"), 100);
        log.record(
            "grant",
            Activity::enrollment(GrantActivityEvent::RefusedDeviceLimit, "linux/1.2.0"),
            150,
        );
        log.record("grant", checked_in("aaaa"), 200);
        let recent = log.recent("grant");
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].event, GrantActivityEvent::CheckedIn);
        assert_eq!(recent[0].count, 2);
        assert_eq!(recent[0].first_at_epoch_seconds, 100);
        assert_eq!(recent[0].last_at_epoch_seconds, 200);
        assert_eq!(recent[1].event, GrantActivityEvent::RefusedDeviceLimit);
        assert_eq!(recent[1].session_id, None);
    }

    #[test]
    fn protocols_and_sessions_keep_entries_apart() {
        let mut log = ActivityLog::default();
        let signed_in =
            |session| Activity::session(GrantActivityEvent::SignedIn, session, "linux/1.2.0");
        log.record(
            "grant",
            signed_in("aaaa").over(TransportProtocol::OpenVpnStunnel),
            1,
        );
        log.record(
            "grant",
            signed_in("aaaa").over(TransportProtocol::Hysteria2),
            2,
        );
        log.record(
            "grant",
            signed_in("bbbb").over(TransportProtocol::Hysteria2),
            3,
        );
        assert_eq!(log.recent("grant").len(), 3);
        assert!(log.recent("other").is_empty());
    }

    #[test]
    fn each_grant_keeps_a_bounded_log_and_forgotten_grants_go() {
        let mut log = ActivityLog::default();
        for session in 0..(MAX_ENTRIES_PER_GRANT + 5) {
            log.record(
                "grant",
                checked_in(&format!("{session:04}")),
                session as u64,
            );
        }
        log.record("other", checked_in("aaaa"), 1);
        let recent = log.recent("grant");
        assert_eq!(recent.len(), MAX_ENTRIES_PER_GRANT);
        assert_eq!(
            recent.last().and_then(|entry| entry.session_id.as_deref()),
            Some("0005")
        );
        log.retain(|grant_id| grant_id == "other");
        assert!(log.recent("grant").is_empty());
        assert_eq!(log.recent("other").len(), 1);
    }
}
