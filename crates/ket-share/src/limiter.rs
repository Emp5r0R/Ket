//! Slows down anyone guessing the password.
//!
//! A device that gets the password wrong [`MAX_FAILURES`] times within
//! [`WINDOW`] is turned away without being asked again until a full window
//! has passed since its last wrong try. Because many addresses can be used
//! from one network, all wrong tries together are capped as well: past
//! [`MAX_TOTAL_FAILURES`] in a window, only devices already signed in can
//! open new connections until it passes.

use std::{
    collections::HashMap,
    net::IpAddr,
    time::{Duration, Instant},
};

pub const MAX_FAILURES: u32 = 5;
pub const MAX_TOTAL_FAILURES: usize = 30;
pub const WINDOW: Duration = Duration::from_secs(60);
/// Bounds memory however many addresses try.
const MAX_TRACKED: usize = 1024;

#[derive(Default)]
pub(crate) struct SignInLimiter {
    devices: HashMap<IpAddr, Failures>,
    recent: Vec<Instant>,
}

#[derive(Clone, Copy)]
struct Failures {
    count: u32,
    last: Instant,
}

impl SignInLimiter {
    /// Whether a new connection from `device` is turned away. `signed_in`
    /// says the device already has a connection open with the right password.
    pub(crate) fn turns_away(&mut self, device: IpAddr, signed_in: bool, now: Instant) -> bool {
        self.forget_before(now);
        if self
            .devices
            .get(&device)
            .is_some_and(|failures| failures.count >= MAX_FAILURES)
        {
            return true;
        }
        !signed_in && self.recent.len() >= MAX_TOTAL_FAILURES
    }

    pub(crate) fn failed(&mut self, device: IpAddr, now: Instant) {
        self.forget_before(now);
        let failures = self.devices.entry(device).or_insert(Failures {
            count: 0,
            last: now,
        });
        failures.count += 1;
        failures.last = now;
        if self.recent.len() < MAX_TOTAL_FAILURES {
            self.recent.push(now);
        } else {
            // Keeps the cap in force for a window after the latest try.
            self.recent.remove(0);
            self.recent.push(now);
        }
        if self.devices.len() > MAX_TRACKED
            && let Some(oldest) = self
                .devices
                .iter()
                .min_by_key(|(_, failures)| failures.last)
                .map(|(device, _)| *device)
        {
            self.devices.remove(&oldest);
        }
    }

    pub(crate) fn succeeded(&mut self, device: IpAddr) {
        self.devices.remove(&device);
    }

    fn forget_before(&mut self, now: Instant) {
        self.devices
            .retain(|_, failures| now.saturating_duration_since(failures.last) < WINDOW);
        self.recent
            .retain(|at| now.saturating_duration_since(*at) < WINDOW);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(last: u8) -> IpAddr {
        IpAddr::from([192, 168, 1, last])
    }

    #[test]
    fn a_device_is_turned_away_after_five_wrong_tries_for_a_minute() {
        let start = Instant::now();
        let mut limiter = SignInLimiter::default();
        for second in 0..4 {
            limiter.failed(device(2), start + Duration::from_secs(second));
            assert!(!limiter.turns_away(device(2), false, start + Duration::from_secs(second)));
        }
        limiter.failed(device(2), start + Duration::from_secs(10));
        assert!(limiter.turns_away(device(2), false, start + Duration::from_secs(10)));
        // Even with a connection already open, a device that keeps guessing
        // is stopped.
        assert!(limiter.turns_away(device(2), true, start + Duration::from_secs(69)));
        assert!(!limiter.turns_away(device(3), false, start + Duration::from_secs(69)));
        assert!(!limiter.turns_away(device(2), false, start + Duration::from_secs(70)));
    }

    #[test]
    fn spread_out_wrong_tries_never_add_up() {
        let start = Instant::now();
        let mut limiter = SignInLimiter::default();
        for minute in 0..20 {
            limiter.failed(device(2), start + Duration::from_secs(minute * 61));
        }
        assert!(!limiter.turns_away(device(2), false, start + Duration::from_secs(19 * 61)));
    }

    #[test]
    fn signing_in_clears_a_device_s_wrong_tries() {
        let start = Instant::now();
        let mut limiter = SignInLimiter::default();
        for _ in 0..4 {
            limiter.failed(device(2), start);
        }
        limiter.succeeded(device(2));
        limiter.failed(device(2), start);
        assert!(!limiter.turns_away(device(2), false, start));
    }

    #[test]
    fn many_addresses_guessing_lock_out_new_devices_but_not_signed_in_ones() {
        let start = Instant::now();
        let mut limiter = SignInLimiter::default();
        for last in 0..MAX_TOTAL_FAILURES {
            limiter.failed(device(last as u8 + 10), start);
        }
        assert!(limiter.turns_away(device(200), false, start));
        assert!(!limiter.turns_away(device(201), true, start));
        assert!(!limiter.turns_away(device(200), false, start + WINDOW));
    }

    #[test]
    fn memory_stays_bounded() {
        let start = Instant::now();
        let mut limiter = SignInLimiter::default();
        for index in 0..(MAX_TRACKED as u32 + 100) {
            limiter.failed(
                IpAddr::from((0x0a00_0000 + index).to_be_bytes()),
                start + Duration::from_millis(u64::from(index)),
            );
        }
        assert!(limiter.devices.len() <= MAX_TRACKED);
        assert!(limiter.recent.len() <= MAX_TOTAL_FAILURES);
    }
}
