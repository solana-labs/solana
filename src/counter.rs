use influx_db_client as influxdb;
use metrics;
use std::sync::atomic::{AtomicUsize, Ordering};
use timing;

const INFLUX_RATE: usize = 100;
pub const DEFAULT_LOG_RATE: usize = 10;

pub struct Counter {
    pub name: &'static str,
    /// total accumulated value
    pub counts: AtomicUsize,
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
            times: AtomicUsize::new(0),
            lastlog: AtomicUsize::new(0),
            lograte: $lograte,
        }
    };
}

macro_rules! inc_counter {
    ($name:expr, $count:expr) => {
        unsafe { $name.inc($count) };
    };
}

impl Counter {
    pub fn inc(&mut self, events: usize) {
        let counts = self.counts.fetch_add(events, Ordering::Relaxed);
        let times = self.times.fetch_add(1, Ordering::Relaxed);
        let lastlog = self.lastlog.load(Ordering::Relaxed);
        if times % self.lograte == 0 && times > 0 {
            info!(
                "COUNTER:{{\"name\": \"{}\", \"counts\": {}, \"samples\": {},  \"now\": {}}}",
                self.name,
                counts,
                times,
                timing::timestamp(),
            );
        }
        if times % INFLUX_RATE == 0 && times > 0 {
            metrics::submit(
                influxdb::Point::new(&format!("counter_{}", self.name))
                    .add_field(
                        "count",
                        influxdb::Value::Integer(counts as i64 - lastlog as i64),
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
    #[test]
    fn test_counter() {
        static mut COUNTER: Counter = create_counter!("test", 100);
        let count = 1;
        inc_counter!(COUNTER, count);
        unsafe {
            assert_eq!(COUNTER.counts.load(Ordering::Relaxed), 1);
            assert_eq!(COUNTER.times.load(Ordering::Relaxed), 1);
            assert_eq!(COUNTER.lograte, 100);
            assert_eq!(COUNTER.lastlog.load(Ordering::Relaxed), 0);
            assert_eq!(COUNTER.name, "test");
        }
        for _ in 0..199 {
            inc_counter!(COUNTER, 2);
        }
        unsafe {
            assert_eq!(COUNTER.lastlog.load(Ordering::Relaxed), 199);
        }
        inc_counter!(COUNTER, 2);
        unsafe {
            assert_eq!(COUNTER.lastlog.load(Ordering::Relaxed), 399);
        }
    }
}
