//! The `timing` module provides std::time utility functions.
use {
    crate::unchecked_div_by_const,
    std::{
        sync::atomic::{AtomicU64, Ordering},
        time::{Duration, SystemTime, UNIX_EPOCH},
    },
};

pub fn duration_as_ns(d: &Duration) -> u64 {
    d.as_secs()
        .saturating_mul(1_000_000_000)
        .saturating_add(u64::from(d.subsec_nanos()))
}

pub fn duration_as_us(d: &Duration) -> u64 {
    d.as_secs()
        .saturating_mul(1_000_000)
        .saturating_add(unchecked_div_by_const!(u64::from(d.subsec_nanos()), 1_000))
}

pub fn duration_as_ms(d: &Duration) -> u64 {
    d.as_secs()
        .saturating_mul(1000)
        .saturating_add(unchecked_div_by_const!(
            u64::from(d.subsec_nanos()),
            1_000_000
        ))
}

pub fn duration_as_s(d: &Duration) -> f32 {
    d.as_secs() as f32 + (d.subsec_nanos() as f32 / 1_000_000_000.0)
}

/// return timestamp as ms
pub fn timestamp() -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("create timestamp in timing");
    duration_as_ms(&now)
}

pub const SECONDS_PER_YEAR: f64 = 365.242_199 * 24.0 * 60.0 * 60.0;

/// from years to slots
pub fn years_as_slots(years: f64, tick_duration: &Duration, ticks_per_slot: u64) -> f64 {
    // slots is  years * slots/year
    years       *
    //  slots/year  is  seconds/year ...
        SECONDS_PER_YEAR
    //  * (ns/s)/(ns/tick) / ticks/slot = 1/s/1/tick = ticks/s
        * (1_000_000_000.0 / duration_as_ns(tick_duration) as f64)
    //  / ticks/slot
        / ticks_per_slot as f64
}

/// From slots per year to slot duration
pub fn slot_duration_from_slots_per_year(slots_per_year: f64) -> Duration {
    // Recently, rust changed from infinity as usize being zero to 2^64-1; ensure it's zero here
    let slot_in_ns = if slots_per_year != 0.0 {
        (SECONDS_PER_YEAR * 1_000_000_000.0) / slots_per_year
    } else {
        0.0
    };
    Duration::from_nanos(slot_in_ns as u64)
}

#[derive(Debug, Default)]
pub struct AtomicInterval {
    last_update: AtomicU64,
}

impl AtomicInterval {
    /// true if 'interval_time_ms' has elapsed since last time we returned true as long as it has been 'interval_time_ms' since this struct was created
    pub fn should_update(&self, interval_time_ms: u64) -> bool {
        self.should_update_ext(interval_time_ms, true)
    }

    /// a primary use case is periodic metric reporting, potentially from different threads
    /// true if 'interval_time_ms' has elapsed since last time we returned true
    /// except, if skip_first=false, false until 'interval_time_ms' has elapsed since this struct was created
    pub fn should_update_ext(&self, interval_time_ms: u64, skip_first: bool) -> bool {
        let now = timestamp();
        let last = self.last_update.load(Ordering::Relaxed);
        now.saturating_sub(last) > interval_time_ms
            && self
                .last_update
                .compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed)
                == Ok(last)
            && !(skip_first && last == 0)
    }

    /// return ms elapsed since the last time the time was set
    pub fn elapsed_ms(&self) -> u64 {
        let now = timestamp();
        let last = self.last_update.load(Ordering::Relaxed);
        now.saturating_sub(last) // wrapping somehow?
    }

    /// return ms until the interval_time will have elapsed
    pub fn remaining_until_next_interval(&self, interval_time: u64) -> u64 {
        interval_time.saturating_sub(self.elapsed_ms())
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_interval_update() {
        solana_logger::setup();
        let i = AtomicInterval::default();
        assert!(!i.should_update(1000));

        let i = AtomicInterval::default();
        assert!(i.should_update_ext(1000, false));

        std::thread::sleep(Duration::from_millis(10));
        assert!(i.elapsed_ms() > 9 && i.elapsed_ms() < 1000);
        assert!(
            i.remaining_until_next_interval(1000) > 9
                && i.remaining_until_next_interval(1000) < 991
        );
        assert!(i.should_update(9));
        assert!(!i.should_update(100));
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn test_years_as_slots() {
        let tick_duration = Duration::from_micros(1000 * 1000 / 160);

        // interestingly large numbers with 160 ticks/second
        assert_eq!(years_as_slots(0.0, &tick_duration, 4) as u64, 0);
        assert_eq!(
            years_as_slots(1.0 / 12f64, &tick_duration, 4) as u64,
            105_189_753
        );
        assert_eq!(years_as_slots(1.0, &tick_duration, 4) as u64, 1_262_277_039);

        let tick_duration = Duration::from_micros(1000 * 1000);
        // one second in years with one tick per second + one tick per slot
        assert_eq!(
            years_as_slots(1.0 / SECONDS_PER_YEAR, &tick_duration, 1),
            1.0
        );
    }

    #[test]
    fn test_slot_duration_from_slots_per_year() {
        let slots_per_year = 1_262_277_039.0;
        let ticks_per_slot = 4;

        assert_eq!(
            slot_duration_from_slots_per_year(slots_per_year),
            Duration::from_micros(1000 * 1000 / 160) * ticks_per_slot
        );
        assert_eq!(
            slot_duration_from_slots_per_year(0.0),
            Duration::from_micros(0) * ticks_per_slot
        );

        let slots_per_year = SECONDS_PER_YEAR;
        let ticks_per_slot = 1;
        assert_eq!(
            slot_duration_from_slots_per_year(slots_per_year),
            Duration::from_millis(1000) * ticks_per_slot
        );
    }

    #[test]
    fn test_duration_as() {
        // zero
        let test_zero = Duration::from_nanos(0);
        assert_eq!(duration_as_ns(&test_zero), 0);
        assert_eq!(duration_as_us(&test_zero), 0);
        assert_eq!(duration_as_ms(&test_zero), 0);
        assert!((duration_as_s(&test_zero) - 0f32) <= f32::EPSILON);
        // min non-zero for each unit
        let test_1ns = Duration::from_nanos(1);
        assert_eq!(duration_as_ns(&test_1ns), 1);
        assert_eq!(duration_as_us(&test_1ns), 0);
        assert_eq!(duration_as_ms(&test_1ns), 0);
        assert!((duration_as_s(&test_1ns) - 0.000_000_001f32) <= f32::EPSILON);
        let test_1ns = Duration::from_micros(1);
        assert_eq!(duration_as_ns(&test_1ns), 1_000);
        assert_eq!(duration_as_us(&test_1ns), 1);
        assert_eq!(duration_as_ms(&test_1ns), 0);
        assert!((duration_as_s(&test_1ns) - 0.000_001f32) <= f32::EPSILON);
        let test_1ns = Duration::from_millis(1);
        assert_eq!(duration_as_ns(&test_1ns), 1_000_000);
        assert_eq!(duration_as_us(&test_1ns), 1_000);
        assert_eq!(duration_as_ms(&test_1ns), 1);
        assert!((duration_as_s(&test_1ns) - 0.001f32) <= f32::EPSILON);
        let test_1ns = Duration::from_secs(1);
        assert_eq!(duration_as_ns(&test_1ns), 1_000_000_000);
        assert_eq!(duration_as_us(&test_1ns), 1_000_000);
        assert_eq!(duration_as_ms(&test_1ns), 1_000);
        assert!((duration_as_s(&test_1ns) - 1f32) <= f32::EPSILON);
        // max without error for each unit (except secs, 'cause if you use floats
        // you deserve to get got)
        const DUR_MAX_SECS: u64 = Duration::MAX.as_secs();
        const NS_PER_SEC: u64 = 1_000_000_000;
        let max_as_ns_secs = DUR_MAX_SECS / NS_PER_SEC;
        let max_as_ns_ns = (DUR_MAX_SECS % NS_PER_SEC) as u32;
        let max_as_ns = Duration::new(max_as_ns_secs, max_as_ns_ns);
        assert_eq!(max_as_ns_secs, 18_446_744_073);
        assert_eq!(max_as_ns_ns, 709_551_615);
        assert_eq!(duration_as_ns(&max_as_ns), u64::MAX);
        const US_PER_SEC: u64 = 1_000_000;
        let max_as_us_secs = DUR_MAX_SECS / US_PER_SEC;
        let max_as_us_ns = (DUR_MAX_SECS % US_PER_SEC) as u32;
        let max_as_us = Duration::new(max_as_us_secs, max_as_us_ns * 1_000);
        assert_eq!(max_as_us_secs, 18_446_744_073_709);
        assert_eq!(max_as_us_ns, 551_615);
        assert_eq!(duration_as_us(&max_as_us), u64::MAX);
        const MS_PER_SEC: u64 = 1_000;
        let max_as_ms_secs = DUR_MAX_SECS / MS_PER_SEC;
        let max_as_ms_ns = (DUR_MAX_SECS % MS_PER_SEC) as u32;
        let max_as_ms = Duration::new(max_as_ms_secs, max_as_ms_ns * 1_000_000);
        assert_eq!(max_as_ms_secs, 18_446_744_073_709_551);
        assert_eq!(max_as_ms_ns, 615);
        assert_eq!(duration_as_ms(&max_as_ms), u64::MAX);
    }
}
