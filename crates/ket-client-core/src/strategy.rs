use std::collections::BTreeMap;

use ket_core::{SessionTransport, TransportProtocol};

use crate::TransportAdapter;

#[derive(Clone, Debug)]
pub struct SelectionPolicy {
    pub max_attempts: usize,
    pub preferred_protocol: Option<TransportProtocol>,
    pub failure_cooldown_seconds: u64,
}

impl Default for SelectionPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            preferred_protocol: None,
            failure_cooldown_seconds: 15,
        }
    }
}

#[derive(Clone, Debug, Default)]
struct TransportRecord {
    consecutive_failures: u32,
    last_latency_ms: Option<u64>,
    cooldown_until_epoch_seconds: u64,
}

#[derive(Clone, Debug, Default)]
pub struct TransportHistory {
    records: BTreeMap<String, TransportRecord>,
}

impl TransportHistory {
    pub fn record_success(&mut self, id: &str, latency_ms: u64) {
        self.records.insert(
            id.to_owned(),
            TransportRecord {
                consecutive_failures: 0,
                last_latency_ms: Some(latency_ms),
                cooldown_until_epoch_seconds: 0,
            },
        );
    }

    pub fn record_failure(&mut self, id: &str, now: u64, base_cooldown_seconds: u64) {
        let record = self.records.entry(id.to_owned()).or_default();
        record.consecutive_failures = record.consecutive_failures.saturating_add(1);
        let multiplier = 1_u64 << record.consecutive_failures.saturating_sub(1).min(3);
        record.cooldown_until_epoch_seconds =
            now.saturating_add(base_cooldown_seconds.saturating_mul(multiplier).min(120));
    }
}

#[derive(Clone, Debug)]
pub struct TransportSelector {
    policy: SelectionPolicy,
}

impl TransportSelector {
    pub fn new(policy: SelectionPolicy) -> Self {
        Self { policy }
    }

    pub fn policy(&self) -> &SelectionPolicy {
        &self.policy
    }

    pub fn rank<'a>(
        &self,
        transports: &'a [SessionTransport],
        adapters: &[std::sync::Arc<dyn TransportAdapter>],
        history: &TransportHistory,
        now: u64,
    ) -> Vec<&'a SessionTransport> {
        let mut candidates: Vec<_> = transports
            .iter()
            .filter(|transport| adapters.iter().any(|adapter| adapter.supports(transport)))
            .collect();
        candidates.sort_by_key(|transport| {
            let record = history.records.get(&transport.profile.id);
            let cooling_down =
                record.is_some_and(|record| record.cooldown_until_epoch_seconds > now);
            let failures = record.map_or(0, |record| record.consecutive_failures) as u64;
            let latency = record
                .and_then(|record| record.last_latency_ms)
                .unwrap_or(500);
            let preferred = self
                .policy
                .preferred_protocol
                .as_ref()
                .is_some_and(|protocol| protocol == &transport.profile.protocol);
            (
                cooling_down,
                !preferred,
                failures,
                transport.profile.priority,
                latency,
                transport.profile.id.as_str(),
            )
        });
        candidates.truncate(self.policy.max_attempts.max(1));
        candidates
    }
}

impl Default for TransportSelector {
    fn default() -> Self {
        Self::new(SelectionPolicy::default())
    }
}
