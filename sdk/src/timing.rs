//! The `timing` module provides std::time utility functions.
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn duration_as_ns(d: &Duration) -> u64 {
    d.as_secs() * 1_000_000_000 + u64::from(d.subsec_nanos())
}

pub fn duration_as_us(d: &Duration) -> u64 {
    (d.as_secs() * 1000 * 1000) + (u64::from(d.subsec_nanos()) / 1_000)
}

pub fn duration_as_ms(d: &Duration) -> u64 {
    (d.as_secs() * 1000) + (u64::from(d.subsec_nanos()) / 1_000_000)
}

pub fn duration_as_s(d: &Duration) -> f32 {
    d.as_secs() as f32 + (d.subsec_nanos() as f32 / 1_000_000_000.0)
}

pub fn timestamp() -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("create timestamp in timing");
    duration_as_ms(&now)
}

pub const SECONDS_PER_YEAR: f64 = (365.242_199 * 24.0 * 60.0 * 60.0);

/// from years to slots
pub fn years_as_slots(years: f64, tick_duration: &Duration, ticks_per_slot: u64) -> f64 {
    // slots is  years * slots/year
    years       *
    //  slots/year  is  seconds/year ...
        SECONDS_PER_YEAR
    //  * (ns/s)/(ns/tick) / ticks/slot = 1/s/1/tick = ticks/s
        *(1_000_000_000.0 / duration_as_ns(tick_duration) as f64)
    //  / ticks/slot
        / ticks_per_slot as f64
}

/// From slots per year to tick_duration
pub fn slot_duration_from_slots_per_year(slots_per_year: f64) -> Duration {
    // Regarding division by zero potential below: for some reason, if Rust stores an `inf` f64 and
    // then converts it to a u64 on use, it always returns 0, as opposed to std::u64::MAX or any
    // other huge value
    let slot_in_ns = (SECONDS_PER_YEAR * 1_000_000_000.0) / slots_per_year;
    Duration::from_nanos(slot_in_ns as u64)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
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
}
