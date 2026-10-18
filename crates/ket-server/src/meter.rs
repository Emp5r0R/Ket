//! Turns the engines' running byte counters into what each session moved
//! since the last reading. Kept in memory only.
//!
//! An engine's counter starts again from zero when the engine restarts or,
//! for OpenVPN, when the device connects again (its reading names a new
//! `since`), so a counter that went down or changed `since` counts in full.
//! A counter seen for the first time counts in full when it began after both
//! the meter started and the grant's allowance began to apply. Otherwise it
//! may hold bytes counted before Ket restarted, or moved before the grant
//! had an allowance, so it only sets the starting point. Ket therefore never
//! counts a byte twice, and may miss what moved in the last moments before it
//! restarted.

use std::collections::HashMap;

use crate::{data_plane::UsageReading, service::SessionAllocation};

/// A live session of a grant with an allowance.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MeteredSession {
    pub allocation: SessionAllocation,
    pub issued_at_epoch_seconds: u64,
    /// When the grant's allowance began to apply.
    pub counted_since_epoch_seconds: u64,
}

pub(crate) struct UsageMeter {
    started_at: u64,
    /// The last reading of each counter: (engine, session) to (since, bytes).
    last: HashMap<(usize, String), (u64, u64)>,
}

impl UsageMeter {
    pub fn new(started_at: u64) -> Self {
        Self {
            started_at,
            last: HashMap::new(),
        }
    }

    /// What each of `sessions` moved since the previous call, by session ID.
    /// Counters an engine did not report this time are kept for the next
    /// reading; those of sessions no longer listed are forgotten.
    pub fn take(
        &mut self,
        sessions: &[MeteredSession],
        readings: &[UsageReading],
    ) -> HashMap<String, u64> {
        let sessions_by_id = sessions
            .iter()
            .map(|session| (session.allocation.id.as_str(), session))
            .collect::<HashMap<_, _>>();
        self.last
            .retain(|(_, session_id), _| sessions_by_id.contains_key(session_id.as_str()));

        let mut moved = HashMap::new();
        for reading in readings {
            let Some(session) = sessions_by_id.get(reading.session_id.as_str()) else {
                continue;
            };
            let key = (reading.engine, reading.session_id.clone());
            let delta = match self.last.get(&key) {
                Some(&(since, bytes)) if since == reading.since && reading.bytes >= bytes => {
                    reading.bytes - bytes
                }
                Some(_) => reading.bytes,
                None => {
                    let began = session.issued_at_epoch_seconds.max(reading.since);
                    if began >= self.started_at.max(session.counted_since_epoch_seconds) {
                        reading.bytes
                    } else {
                        0
                    }
                }
            };
            self.last.insert(key, (reading.since, reading.bytes));
            let total = moved.entry(reading.session_id.clone()).or_insert(0_u64);
            *total = total.saturating_add(delta);
        }
        moved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str, issued_at: u64, counted_since: u64) -> MeteredSession {
        MeteredSession {
            allocation: SessionAllocation {
                id: id.to_owned(),
                resource_slot: 0,
            },
            issued_at_epoch_seconds: issued_at,
            counted_since_epoch_seconds: counted_since,
        }
    }

    fn reading(session_id: &str, engine: usize, since: u64, bytes: u64) -> UsageReading {
        UsageReading {
            session_id: session_id.to_owned(),
            engine,
            since,
            bytes,
        }
    }

    #[test]
    fn counters_already_running_set_the_start_and_new_sessions_count_in_full() {
        // Ket restarted at 100: what the old session's counter holds may
        // have been counted already.
        let mut meter = UsageMeter::new(100);
        let old = session("Old000000001", 50, 10);
        assert_eq!(
            meter.take(
                std::slice::from_ref(&old),
                &[reading("Old000000001", 0, 0, 5_000)]
            ),
            HashMap::from([("Old000000001".to_owned(), 0)])
        );
        assert_eq!(
            meter.take(
                std::slice::from_ref(&old),
                &[reading("Old000000001", 0, 0, 5_600)]
            ),
            HashMap::from([("Old000000001".to_owned(), 600)])
        );

        let sessions = [old, session("New000000001", 120, 10)];
        let moved = meter.take(
            &sessions,
            &[
                reading("Old000000001", 0, 0, 5_700),
                reading("New000000001", 0, 0, 300),
            ],
        );
        assert_eq!(moved["Old000000001"], 100);
        assert_eq!(moved["New000000001"], 300);
    }

    #[test]
    fn a_session_connected_before_the_first_reading_still_counts_in_full() {
        // The grant got its allowance at 200 and the device connected at
        // 205; the meter first reads it at 215.
        let mut meter = UsageMeter::new(100);
        let sessions = [session("Sess00000001", 205, 200)];
        assert_eq!(
            meter.take(&sessions, &[reading("Sess00000001", 0, 0, 9_000)])["Sess00000001"],
            9_000
        );
    }

    #[test]
    fn restarted_counters_and_new_connections_count_from_zero() {
        let mut meter = UsageMeter::new(100);
        let sessions = [session("Sess00000001", 100, 100)];
        meter.take(&sessions, &[reading("Sess00000001", 0, 0, 1_000)]);
        // The engine restarted: its counter went down.
        assert_eq!(
            meter.take(&sessions, &[reading("Sess00000001", 0, 0, 40)])["Sess00000001"],
            40
        );
        // OpenVPN: the device connected again, so the counter is new even
        // though it is already higher.
        meter.take(&sessions, &[reading("Sess00000001", 1, 110, 500)]);
        assert_eq!(
            meter.take(&sessions, &[reading("Sess00000001", 1, 140, 900)])["Sess00000001"],
            900
        );
    }

    #[test]
    fn engines_count_apart_and_a_missed_reading_is_not_a_reset() {
        let mut meter = UsageMeter::new(100);
        let sessions = [session("Sess00000001", 100, 100)];
        meter.take(
            &sessions,
            &[
                reading("Sess00000001", 0, 0, 100),
                reading("Sess00000001", 1, 0, 2_000),
            ],
        );
        // Engine 1 could not be read this time.
        assert_eq!(
            meter.take(&sessions, &[reading("Sess00000001", 0, 0, 150)])["Sess00000001"],
            50
        );
        assert_eq!(
            meter.take(
                &sessions,
                &[
                    reading("Sess00000001", 0, 0, 150),
                    reading("Sess00000001", 1, 0, 2_500)
                ],
            )["Sess00000001"],
            500
        );
    }

    #[test]
    fn a_grant_given_an_allowance_later_counts_from_then_on() {
        let mut meter = UsageMeter::new(0);
        // Connected at 10, long before the grant had an allowance (at 500).
        let sessions = [session("Sess00000001", 10, 500)];
        assert_eq!(
            meter.take(&sessions, &[reading("Sess00000001", 0, 0, 9_000_000)])["Sess00000001"],
            0
        );
        assert_eq!(
            meter.take(&sessions, &[reading("Sess00000001", 0, 0, 9_000_100)])["Sess00000001"],
            100
        );
        // OpenVPN: a connection made after the allowance counts in full.
        assert_eq!(
            meter.take(&sessions, &[reading("Sess00000001", 1, 600, 700)])["Sess00000001"],
            700
        );
    }

    #[test]
    fn readings_for_unknown_sessions_are_ignored_and_ended_ones_forgotten() {
        let mut meter = UsageMeter::new(100);
        let sessions = [session("Sess00000001", 100, 100)];
        let moved = meter.take(
            &sessions,
            &[
                reading("Sess00000001", 0, 0, 10),
                reading("Other0000001", 0, 0, 99),
            ],
        );
        assert_eq!(moved, HashMap::from([("Sess00000001".to_owned(), 10)]));
        meter.take(&[], &[]);
        assert!(meter.last.is_empty());
    }
}
