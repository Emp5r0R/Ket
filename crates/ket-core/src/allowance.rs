//! Data allowances: how much an access grant's devices may move together,
//! both directions counted, in each UTC calendar day, week (from Monday), or
//! month. When a period ends the count starts again from zero.

use serde::{Deserialize, Deserializer, Serialize};

/// The smallest and largest allowance a grant may have: 1 MB and 1 PB
/// (binary units, as `ket` prints them).
pub const MIN_ALLOWANCE_BYTES: u64 = 1 << 20;
pub const MAX_ALLOWANCE_BYTES: u64 = 1 << 50;

const SECONDS_PER_DAY: u64 = 86_400;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AllowancePeriod {
    Day,
    Week,
    Month,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DataAllowance {
    pub bytes: u64,
    pub period: AllowancePeriod,
}

impl DataAllowance {
    pub fn is_valid(&self) -> bool {
        (MIN_ALLOWANCE_BYTES..=MAX_ALLOWANCE_BYTES).contains(&self.bytes)
    }
}

/// An allowance and how much of it this period has used.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AllowanceStatus {
    #[serde(flatten)]
    pub allowance: DataAllowance,
    pub used_bytes: u64,
    /// When this period ends and the count starts again.
    pub resets_at_epoch_seconds: u64,
}

impl AllowanceStatus {
    pub fn is_used_up(&self) -> bool {
        self.used_bytes >= self.allowance.bytes
    }
}

impl AllowancePeriod {
    /// The start of the period holding `now` (Unix seconds).
    pub fn start(self, now: u64) -> u64 {
        let day = now / SECONDS_PER_DAY;
        let first = match self {
            Self::Day => day,
            // 1970-01-01 was a Thursday, so day 4 was a Monday.
            Self::Week => day.saturating_sub((day + 3) % 7),
            Self::Month => {
                let (year, month, _) = civil_from_days(day);
                days_from_civil(year, month, 1)
            }
        };
        first * SECONDS_PER_DAY
    }

    /// When the period holding `now` ends and the next one begins.
    pub fn end(self, now: u64) -> u64 {
        match self {
            Self::Day => self.start(now) + SECONDS_PER_DAY,
            Self::Week => self.start(now) + 7 * SECONDS_PER_DAY,
            Self::Month => {
                let (year, month, _) = civil_from_days(now / SECONDS_PER_DAY);
                let (year, month) = if month == 12 {
                    (year + 1, 1)
                } else {
                    (year, month + 1)
                };
                days_from_civil(year, month, 1) * SECONDS_PER_DAY
            }
        }
    }
}

/// `2026-11-01 00:00 UTC`, the form refusals give the reset time in.
pub fn utc_minute(epoch_seconds: u64) -> String {
    let (year, month, day) = civil_from_days(epoch_seconds / SECONDS_PER_DAY);
    let minutes = epoch_seconds % SECONDS_PER_DAY / 60;
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02} UTC",
        minutes / 60,
        minutes % 60
    )
}

/// Days since 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's
/// `civil_from_days`, for dates from 1970 on).
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let day_of_era = z % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

fn days_from_civil(year: u64, month: u64, day: u64) -> u64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year / 400;
    let year_of_era = year % 400;
    let shifted_month = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * shifted_month + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// For `Option<Option<T>>` request fields: absent leaves the value as it is
/// (`None`), `null` removes it (`Some(None)`).
pub(crate) fn present<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// 2026-10-18 13:45:10 UTC, a Sunday.
    const NOW: u64 = 1_792_331_110;

    #[test]
    fn periods_follow_the_utc_calendar() {
        assert_eq!(utc_minute(NOW), "2026-10-18 13:45 UTC");
        assert_eq!(
            utc_minute(AllowancePeriod::Day.start(NOW)),
            "2026-10-18 00:00 UTC"
        );
        assert_eq!(
            utc_minute(AllowancePeriod::Day.end(NOW)),
            "2026-10-19 00:00 UTC"
        );
        assert_eq!(
            utc_minute(AllowancePeriod::Week.start(NOW)),
            "2026-10-12 00:00 UTC"
        );
        assert_eq!(
            utc_minute(AllowancePeriod::Week.end(NOW)),
            "2026-10-19 00:00 UTC"
        );
        assert_eq!(
            utc_minute(AllowancePeriod::Month.start(NOW)),
            "2026-10-01 00:00 UTC"
        );
        assert_eq!(
            utc_minute(AllowancePeriod::Month.end(NOW)),
            "2026-11-01 00:00 UTC"
        );
    }

    #[test]
    fn period_edges_belong_to_the_period_they_start() {
        for period in [
            AllowancePeriod::Day,
            AllowancePeriod::Week,
            AllowancePeriod::Month,
        ] {
            let start = period.start(NOW);
            assert_eq!(period.start(start), start);
            assert_eq!(period.start(start - 1), period.start(start - 1));
            assert!(period.start(start - 1) < start);
            assert_eq!(period.end(start - 1), start);
            assert_eq!(period.start(period.end(NOW)), period.end(NOW));
        }
        // The first days of 1970 had no Monday before them.
        assert_eq!(AllowancePeriod::Week.start(0), 0);
    }

    #[test]
    fn months_roll_over_years_and_leap_days() {
        let december = 1_796_083_200 + 5 * 86_400; // 2026-12-06
        assert_eq!(
            utc_minute(AllowancePeriod::Month.start(december)),
            "2026-12-01 00:00 UTC"
        );
        assert_eq!(
            utc_minute(AllowancePeriod::Month.end(december)),
            "2027-01-01 00:00 UTC"
        );
        let leap = 1_835_395_200; // 2028-02-29
        assert_eq!(utc_minute(leap), "2028-02-29 00:00 UTC");
        assert_eq!(
            utc_minute(AllowancePeriod::Month.end(leap)),
            "2028-03-01 00:00 UTC"
        );
        assert_eq!(utc_minute(0), "1970-01-01 00:00 UTC");
    }

    #[test]
    fn allowances_have_stable_wire_names_and_bounds() {
        let status = AllowanceStatus {
            allowance: DataAllowance {
                bytes: 50 << 30,
                period: AllowancePeriod::Month,
            },
            used_bytes: 3 << 30,
            resets_at_epoch_seconds: 1_793_491_200,
        };
        assert_eq!(
            serde_json::to_value(status).unwrap(),
            json!({
                "bytes": 53_687_091_200_u64,
                "period": "month",
                "used_bytes": 3_221_225_472_u64,
                "resets_at_epoch_seconds": 1_793_491_200
            })
        );
        assert!(!status.is_used_up());
        assert!(
            AllowanceStatus {
                used_bytes: 50 << 30,
                ..status
            }
            .is_used_up()
        );
        let bounds = |bytes| {
            DataAllowance {
                bytes,
                period: AllowancePeriod::Day,
            }
            .is_valid()
        };
        assert!(bounds(MIN_ALLOWANCE_BYTES) && bounds(MAX_ALLOWANCE_BYTES));
        assert!(!bounds(MIN_ALLOWANCE_BYTES - 1) && !bounds(MAX_ALLOWANCE_BYTES + 1));
    }
}
