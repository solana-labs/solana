use influx_db_client as influxdb;
use metrics;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use timing;

pub struct Counter {
    pub name: &'static str,
    /// total accumulated value
    pub counts: AtomicUsize,
    pub nanos: AtomicUsize,
    pub times: AtomicUsize,
    /// last accumulated value logged
    pub lastlog: AtomicUsize,
    pub lograte: usize,
}

macro_rules! create_counter {
    ($name:expr, $lograte:expr) => {
        Counter {
            name: $name,
            counts: AtomicUsize::new(0),
            nanos: AtomicUsize::new(0),
            times: AtomicUsize::new(0),
            lastlog: AtomicUsize::new(0),
            lograte: $lograte,
        }
    };
}

macro_rules! inc_counter {
    ($name:expr, $count:expr, $start:expr) => {
        unsafe { $name.inc($count, $start.elapsed()) };
    };
}

impl Counter {
    pub fn inc(&mut self, events: usize, dur: Duration) {
        let total = dur.as_secs() * 1_000_000_000 + dur.subsec_nanos() as u64;
        let counts = self.counts.fetch_add(events, Ordering::Relaxed);
        let nanos = self.nanos.fetch_add(total as usize, Ordering::Relaxed);
        let times = self.times.fetch_add(1, Ordering::Relaxed);
        let lastlog = self.lastlog.load(Ordering::Relaxed);
        if times % self.lograte == 0 && times > 0 {
            info!(
                "COUNTER:{{\"name\": \"{}\", \"counts\": {}, \"nanos\": {}, \"samples\": {}, \"rate\": {}, \"now\": {}}}",
                self.name,
                counts,
                nanos,
                times,
                counts as f64 * 1e9 / nanos as f64,
                timing::timestamp(),
            );
            metrics::submit(
                influxdb::Point::new(&format!("counter_{}", self.name))
                    .add_field(
                        "count",
                        influxdb::Value::Integer(counts as i64 - lastlog as i64),
                    )
                    .add_field(
                        "duration_ms",
                        influxdb::Value::Integer(timing::duration_as_ms(&dur) as i64),
                    )
                    .to_owned(),
            );
            self.lastlog
                .compare_and_swap(lastlog, counts, Ordering::Relaxed);
        }
    }
}
#[cfg(test)]
mod tests {
    use counter::Counter;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Instant;
    #[test]
    fn test_counter() {
        static mut COUNTER: Counter = create_counter!("test", 100);
        let start = Instant::now();
        let count = 1;
        inc_counter!(COUNTER, count, start);
        unsafe {
            assert_eq!(COUNTER.counts.load(Ordering::Relaxed), 1);
            assert_ne!(COUNTER.nanos.load(Ordering::Relaxed), 0);
            assert_eq!(COUNTER.times.load(Ordering::Relaxed), 1);
            assert_eq!(COUNTER.lograte, 100);
            assert_eq!(COUNTER.lastlog.load(Ordering::Relaxed), 0);
            assert_eq!(COUNTER.name, "test");
        }
        for _ in 0..199 {
            inc_counter!(COUNTER, 2, start);
        }
        unsafe {
            assert_eq!(COUNTER.lastlog.load(Ordering::Relaxed), 199);
        }
        inc_counter!(COUNTER, 2, start);
        unsafe {
            assert_eq!(COUNTER.lastlog.load(Ordering::Relaxed), 399);
        }
    }
}
