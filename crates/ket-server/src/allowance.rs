//! A grant's data allowance and this period's running total: what is used,
//! whether devices may still connect, and adding what they moved. Periods
//! follow the UTC calendar ([`ket_core::AllowancePeriod`]); a total from an
//! earlier period counts as nothing.

use ket_core::AllowanceStatus;

use crate::model::{AccessGrantRecord, PeriodUsage};

/// What the grant's devices have moved this period.
pub(crate) fn used(grant: &AccessGrantRecord, now: u64) -> u64 {
    match (grant.allowance, grant.usage) {
        (Some(allowance), Some(usage))
            if usage.period_start_epoch_seconds == allowance.period.start(now) =>
        {
            usage.used_bytes
        }
        _ => 0,
    }
}

pub(crate) fn status(grant: &AccessGrantRecord, now: u64) -> Option<AllowanceStatus> {
    grant.allowance.map(|allowance| AllowanceStatus {
        allowance,
        used_bytes: used(grant, now),
        resets_at_epoch_seconds: allowance.period.end(now),
    })
}

/// When the grant has used up this period's allowance, the time it resets.
pub(crate) fn used_up_until(grant: &AccessGrantRecord, now: u64) -> Option<u64> {
    status(grant, now)
        .filter(AllowanceStatus::is_used_up)
        .map(|status| status.resets_at_epoch_seconds)
}

/// Adds `bytes` to this period's total. A grant without an allowance counts
/// nothing.
pub(crate) fn add(grant: &mut AccessGrantRecord, bytes: u64, now: u64) {
    let Some(allowance) = grant.allowance else {
        return;
    };
    if bytes == 0 {
        return;
    }
    grant.usage = Some(PeriodUsage {
        period_start_epoch_seconds: allowance.period.start(now),
        used_bytes: used(grant, now).saturating_add(bytes),
    });
}

#[cfg(test)]
mod tests {
    use ket_core::{AllowancePeriod, DataAllowance, ProtocolAccess};

    use super::*;

    const DAY: u64 = 86_400;
    /// 2026-10-18 13:45:10 UTC.
    const NOW: u64 = 1_792_331_110;

    fn grant(allowance: Option<DataAllowance>) -> AccessGrantRecord {
        AccessGrantRecord {
            id: "Grant00001".to_owned(),
            secret_hash: String::new(),
            label: "Test".to_owned(),
            max_connections: 1,
            expires_at_epoch_seconds: None,
            created_at_epoch_seconds: 0,
            paused: false,
            protocols: ProtocolAccess::All,
            allowance,
            allowance_since_epoch_seconds: allowance.map(|_| 0),
            usage: None,
        }
    }

    fn daily(bytes: u64) -> Option<DataAllowance> {
        Some(DataAllowance {
            bytes,
            period: AllowancePeriod::Day,
        })
    }

    #[test]
    fn usage_adds_up_until_the_allowance_is_used() {
        let mut grant = grant(daily(1_000));
        assert_eq!(used_up_until(&grant, NOW), None);
        add(&mut grant, 600, NOW);
        add(&mut grant, 0, NOW);
        assert_eq!(used(&grant, NOW), 600);
        assert_eq!(used_up_until(&grant, NOW), None);
        add(&mut grant, 400, NOW + 60);
        let tomorrow = AllowancePeriod::Day.end(NOW);
        assert_eq!(used_up_until(&grant, NOW + 60), Some(tomorrow));
        assert_eq!(
            status(&grant, NOW),
            Some(AllowanceStatus {
                allowance: daily(1_000).unwrap(),
                used_bytes: 1_000,
                resets_at_epoch_seconds: tomorrow,
            })
        );
    }

    #[test]
    fn a_new_period_starts_from_zero_and_keeps_one_total() {
        let mut grant = grant(daily(1_000));
        add(&mut grant, 1_500, NOW);
        let tomorrow = NOW + DAY;
        assert_eq!(used(&grant, tomorrow), 0);
        assert_eq!(used_up_until(&grant, tomorrow), None);
        add(&mut grant, 10, tomorrow);
        assert_eq!(
            grant.usage,
            Some(PeriodUsage {
                period_start_epoch_seconds: AllowancePeriod::Day.start(tomorrow),
                used_bytes: 10,
            })
        );
    }

    #[test]
    fn grants_without_an_allowance_count_nothing() {
        let mut grant = grant(None);
        add(&mut grant, 1_500, NOW);
        assert_eq!(grant.usage, None);
        assert_eq!(status(&grant, NOW), None);
        assert_eq!(used_up_until(&grant, NOW), None);
    }
}
