use std::time::Duration;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn duration_as_ms(d: &Duration) -> u64 {
    return (d.as_secs() * 1000) + (d.subsec_nanos() as u64 / 1_000_000);
}

pub fn duration_as_s(d: &Duration) -> f32 {
    return d.as_secs() as f32 + (d.subsec_nanos() as f32 / 1_000_000_000.0);
}

pub fn timestamp() -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("create timestamp in timing");
    return duration_as_ms(&now);
}
