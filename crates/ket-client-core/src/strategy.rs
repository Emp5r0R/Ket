use std::collections::{BTreeMap, BTreeSet};

use ket_core::{SessionTransport, TransportProtocol};
use serde::{Deserialize, Serialize};

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

/// The most transports a history keeps, so a saved one stays small. A
/// server offers far fewer; past the cap, records are dropped by ID.
const MAX_RECORDS: usize = 32;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
struct TransportRecord {
    consecutive_failures: u32,
    last_latency_ms: Option<u64>,
    cooldown_until_epoch_seconds: u64,
    /// Set by the last success: on the network this history belongs to, a
    /// transport that worked is tried before one that never has.
    worked: bool,
}

/// How each transport fared, on one network when it is saved per network.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default)]
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
                worked: true,
            },
        );
        self.bound();
    }

    fn bound(&mut self) {
        while self.records.len() > MAX_RECORDS {
            let Some(oldest) = self.records.keys().next().cloned() else {
                break;
            };
            self.records.remove(&oldest);
        }
    }

    pub fn record_failure(&mut self, id: &str, now: u64, base_cooldown_seconds: u64) {
        let record = self.records.entry(id.to_owned()).or_default();
        record.consecutive_failures = record.consecutive_failures.saturating_add(1);
        let multiplier = 1_u64 << record.consecutive_failures.saturating_sub(1).min(3);
        record.cooldown_until_epoch_seconds =
            now.saturating_add(base_cooldown_seconds.saturating_mul(multiplier).min(120));
        self.bound();
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
        self.rank_with_preference(transports, adapters, history, now, None)
    }

    pub fn rank_with_preference<'a>(
        &self,
        transports: &'a [SessionTransport],
        adapters: &[std::sync::Arc<dyn TransportAdapter>],
        history: &TransportHistory,
        now: u64,
        preferred_protocol: Option<&TransportProtocol>,
    ) -> Vec<&'a SessionTransport> {
        self.rank_for(
            transports,
            adapters,
            history,
            now,
            preferred_protocol,
            &BTreeSet::new(),
        )
    }

    /// Ranks for one connection. An explicit `preferred_protocol` filters to
    /// that protocol. Otherwise (Automatic) the protocols in
    /// `automatic_excluded` are left out, unless that would leave nothing.
    pub fn rank_for<'a>(
        &self,
        transports: &'a [SessionTransport],
        adapters: &[std::sync::Arc<dyn TransportAdapter>],
        history: &TransportHistory,
        now: u64,
        preferred_protocol: Option<&TransportProtocol>,
        automatic_excluded: &BTreeSet<TransportProtocol>,
    ) -> Vec<&'a SessionTransport> {
        let explicit_protocol = preferred_protocol;
        let ranking_preference = explicit_protocol.or(self.policy.preferred_protocol.as_ref());
        let mut candidates: Vec<_> = transports
            .iter()
            .filter(|transport| adapters.iter().any(|adapter| adapter.supports(transport)))
            .filter(|transport| {
                explicit_protocol.is_none_or(|protocol| protocol == &transport.profile.protocol)
            })
            .collect();
        if explicit_protocol.is_none()
            && candidates
                .iter()
                .any(|transport| !automatic_excluded.contains(&transport.profile.protocol))
        {
            candidates
                .retain(|transport| !automatic_excluded.contains(&transport.profile.protocol));
        }
        candidates.sort_by_key(|transport| {
            let record = history.records.get(&transport.profile.id);
            let cooling_down =
                record.is_some_and(|record| record.cooldown_until_epoch_seconds > now);
            let failures = record.map_or(0, |record| record.consecutive_failures) as u64;
            let worked = record.is_some_and(|record| record.worked);
            let latency = record
                .and_then(|record| record.last_latency_ms)
                .unwrap_or(500);
            let preferred =
                ranking_preference.is_some_and(|protocol| protocol == &transport.profile.protocol);
            (
                cooling_down,
                !preferred,
                failures,
                !worked,
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

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, sync::Arc};

    use crate::ConnectOptions;

    use async_trait::async_trait;
    use ket_core::{Network, TransportCredential, TransportProfile};

    use crate::{ClientError, ProbeReport, StartedTunnel};

    use super::*;

    struct SupportingAdapter(Vec<TransportProtocol>);

    #[async_trait]
    impl TransportAdapter for SupportingAdapter {
        fn supports(&self, transport: &SessionTransport) -> bool {
            self.0.contains(&transport.profile.protocol)
        }

        async fn probe(&self, _transport: &SessionTransport) -> Result<ProbeReport, ClientError> {
            unreachable!("ranking must not probe transports")
        }

        async fn connect(
            &self,
            _transport: &SessionTransport,
            _probe: &ProbeReport,
            _options: &ConnectOptions,
        ) -> Result<StartedTunnel, ClientError> {
            unreachable!("ranking must not connect transports")
        }
    }

    #[test]
    fn default_policy_is_bounded_and_automatic() {
        let policy = SelectionPolicy::default();
        assert_eq!(policy.max_attempts, 3);
        assert_eq!(policy.preferred_protocol, None);
        assert_eq!(policy.failure_cooldown_seconds, 15);
    }

    #[test]
    fn ranking_filters_unsupported_transports_and_is_deterministic() {
        let selector = TransportSelector::default();
        let transports = vec![
            transport("z-reality", TransportProtocol::VlessXtlsReality, 5),
            transport("b-hysteria", TransportProtocol::Hysteria2, 1),
            transport("a-hysteria", TransportProtocol::Hysteria2, 1),
            transport("unsupported", TransportProtocol::Ikev2, 0),
        ];
        let adapters: Vec<Arc<dyn TransportAdapter>> = vec![Arc::new(SupportingAdapter(vec![
            TransportProtocol::Hysteria2,
            TransportProtocol::VlessXtlsReality,
        ]))];

        let ranked = selector.rank(&transports, &adapters, &TransportHistory::default(), 100);
        assert_eq!(ids(&ranked), ["a-hysteria", "b-hysteria", "z-reality"]);
    }

    #[test]
    fn configured_preference_ranks_first_but_explicit_preference_filters_strictly() {
        let selector = TransportSelector::new(SelectionPolicy {
            max_attempts: 10,
            preferred_protocol: Some(TransportProtocol::Stealth),
            failure_cooldown_seconds: 15,
        });
        let transports = vec![
            transport("reality", TransportProtocol::VlessXtlsReality, 1),
            transport("stealth", TransportProtocol::Stealth, 99),
            transport("hysteria", TransportProtocol::Hysteria2, 2),
        ];
        let adapters: Vec<Arc<dyn TransportAdapter>> = vec![Arc::new(SupportingAdapter(vec![
            TransportProtocol::Hysteria2,
            TransportProtocol::Stealth,
            TransportProtocol::VlessXtlsReality,
        ]))];

        assert_eq!(
            ids(&selector.rank(&transports, &adapters, &TransportHistory::default(), 0)),
            ["stealth", "reality", "hysteria"]
        );
        assert_eq!(
            ids(&selector.rank_with_preference(
                &transports,
                &adapters,
                &TransportHistory::default(),
                0,
                Some(&TransportProtocol::Hysteria2),
            )),
            ["hysteria"]
        );
    }

    #[test]
    fn failure_cooldown_backs_off_caps_and_success_resets_history() {
        let mut history = TransportHistory::default();
        for expected in [115, 130, 160, 220, 220, 220] {
            history.record_failure("route", 100, 15);
            assert_eq!(
                history.records["route"].cooldown_until_epoch_seconds,
                expected
            );
        }
        assert_eq!(history.records["route"].consecutive_failures, 6);

        history.record_success("route", 42);
        let record = &history.records["route"];
        assert_eq!(record.consecutive_failures, 0);
        assert_eq!(record.cooldown_until_epoch_seconds, 0);
        assert_eq!(record.last_latency_ms, Some(42));
    }

    #[test]
    fn automatic_leaves_out_unchecked_protocols_but_an_explicit_choice_does_not() {
        let selector = TransportSelector::new(SelectionPolicy {
            max_attempts: 10,
            preferred_protocol: None,
            failure_cooldown_seconds: 15,
        });
        let transports = vec![
            transport("reality", TransportProtocol::VlessXtlsReality, 1),
            transport("hysteria", TransportProtocol::Hysteria2, 0),
        ];
        let adapters: Vec<Arc<dyn TransportAdapter>> = vec![Arc::new(SupportingAdapter(vec![
            TransportProtocol::Hysteria2,
            TransportProtocol::VlessXtlsReality,
        ]))];
        let history = TransportHistory::default();
        let without_hysteria = BTreeSet::from([TransportProtocol::Hysteria2]);
        assert_eq!(
            ids(&selector.rank_for(&transports, &adapters, &history, 0, None, &without_hysteria)),
            ["reality"]
        );
        assert_eq!(
            ids(&selector.rank_for(
                &transports,
                &adapters,
                &history,
                0,
                Some(&TransportProtocol::Hysteria2),
                &without_hysteria,
            )),
            ["hysteria"]
        );
        let everything = BTreeSet::from([
            TransportProtocol::Hysteria2,
            TransportProtocol::VlessXtlsReality,
        ]);
        assert_eq!(
            ids(&selector.rank_for(&transports, &adapters, &history, 0, None, &everything)),
            ["hysteria", "reality"],
            "unchecking every protocol must not leave nothing to connect with"
        );
    }

    #[test]
    fn histories_round_trip_and_stay_bounded() {
        let mut history = TransportHistory::default();
        history.record_success("route", 42);
        history.record_failure("other", 100, 15);
        let encoded = serde_json::to_string(&history).unwrap();
        assert_eq!(
            serde_json::from_str::<TransportHistory>(&encoded).unwrap(),
            history
        );
        assert_eq!(
            serde_json::from_str::<TransportHistory>("{}").unwrap(),
            TransportHistory::default()
        );
        for index in 0..100 {
            history.record_success(&format!("route-{index:03}"), 1);
        }
        assert_eq!(history.records.len(), MAX_RECORDS);
    }

    #[test]
    fn history_orders_ready_successes_before_failed_or_cooling_routes() {
        let selector = TransportSelector::new(SelectionPolicy {
            max_attempts: 5,
            preferred_protocol: None,
            failure_cooldown_seconds: 15,
        });
        let transports = vec![
            transport("fresh", TransportProtocol::Hysteria2, 1),
            transport("fast", TransportProtocol::Hysteria2, 10),
            transport("failed", TransportProtocol::Hysteria2, 0),
            transport("cooling", TransportProtocol::Hysteria2, 0),
        ];
        let adapters: Vec<Arc<dyn TransportAdapter>> = vec![Arc::new(SupportingAdapter(vec![
            TransportProtocol::Hysteria2,
        ]))];
        let mut history = TransportHistory::default();
        history.record_success("fast", 5);
        history.record_failure("failed", 0, 15);
        history.record_failure("cooling", 100, 15);

        // A transport that worked is tried before an untried one of higher
        // priority: on this network it is the last known good.
        assert_eq!(
            ids(&selector.rank(&transports, &adapters, &history, 100)),
            ["fast", "fresh", "failed", "cooling"]
        );
    }

    #[test]
    fn max_attempts_is_enforced_with_a_minimum_of_one() {
        let transports = vec![
            transport("a", TransportProtocol::Hysteria2, 1),
            transport("b", TransportProtocol::Hysteria2, 2),
        ];
        let adapters: Vec<Arc<dyn TransportAdapter>> = vec![Arc::new(SupportingAdapter(vec![
            TransportProtocol::Hysteria2,
        ]))];
        let selector = TransportSelector::new(SelectionPolicy {
            max_attempts: 0,
            preferred_protocol: None,
            failure_cooldown_seconds: 15,
        });
        assert_eq!(
            ids(&selector.rank(&transports, &adapters, &TransportHistory::default(), 0)),
            ["a"]
        );
    }

    fn ids<'a>(transports: &[&'a SessionTransport]) -> Vec<&'a str> {
        transports
            .iter()
            .map(|transport| transport.profile.id.as_str())
            .collect()
    }

    fn transport(id: &str, protocol: TransportProtocol, priority: u16) -> SessionTransport {
        SessionTransport {
            profile: TransportProfile {
                id: id.to_owned(),
                display_name: id.to_owned(),
                protocol,
                endpoint: "vpn.example.test".to_owned(),
                port: 443,
                network: Network::Tcp,
                priority,
                tls_server_name: None,
                options: BTreeMap::new(),
            },
            credential: None::<TransportCredential>,
        }
    }
}
