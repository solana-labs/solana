use influx_db_client as influxdb;
use metrics;
use std::env;
use std::sync::atomic::{AtomicUsize, Ordering};
use timing;

const DEFAULT_METRICS_RATE: usize = 100;

pub struct Counter {
    pub name: &'static str,
    /// total accumulated value
    pub counts: AtomicUsize,
    pub times: AtomicUsize,
    /// last accumulated value logged
    pub lastlog: AtomicUsize,
    pub lograte: AtomicUsize,
}

macro_rules! create_counter {
    ($name:expr, $lograte:expr) => {
        Counter {
            name: $name,
            counts: AtomicUsize::new(0),
            times: AtomicUsize::new(0),
            lastlog: AtomicUsize::new(0),
            lograte: AtomicUsize::new($lograte),
        }
    };
}

macro_rules! inc_counter {
    ($name:expr, $count:expr) => {
        unsafe { $name.inc($count) };
    };
}

macro_rules! inc_new_counter {
    ($name:expr, $count:expr) => {{
        static mut INC_NEW_COUNTER: Counter = create_counter!($name, 0);
        inc_counter!(INC_NEW_COUNTER, $count);
    }};
    ($name:expr, $count:expr, $lograte:expr) => {{
        static mut INC_NEW_COUNTER: Counter = create_counter!($name, $lograte);
        inc_counter!(INC_NEW_COUNTER, $count);
    }};
}

impl Counter {
    fn default_log_rate() -> usize {
        let v = env::var("SOLANA_DEFAULT_METRICS_RATE")
            .map(|x| x.parse().unwrap_or(DEFAULT_METRICS_RATE))
            .unwrap_or(DEFAULT_METRICS_RATE);
        if v == 0 {
            DEFAULT_METRICS_RATE
        } else {
            v
        }
    }
    pub fn inc(&mut self, events: usize) {
        let counts = self.counts.fetch_add(events, Ordering::Relaxed);
        let times = self.times.fetch_add(1, Ordering::Relaxed);
        let mut lograte = self.lograte.load(Ordering::Relaxed);
        if lograte == 0 {
            lograte = Counter::default_log_rate();
            self.lograte.store(lograte, Ordering::Relaxed);
        }
        if times % lograte == 0 && times > 0 {
            let lastlog = self.lastlog.load(Ordering::Relaxed);
            info!(
                "COUNTER:{{\"name\": \"{}\", \"counts\": {}, \"samples\": {},  \"now\": {}}}",
                self.name,
                counts,
                times,
                timing::timestamp(),
            );
            metrics::submit(
                influxdb::Point::new(&format!("counter-{}", self.name))
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
    use counter::{Counter, DEFAULT_METRICS_RATE};
    use std::env;
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[test]
    fn test_counter() {
        static mut COUNTER: Counter = create_counter!("test", 100);
        let count = 1;
        inc_counter!(COUNTER, count);
        unsafe {
            assert_eq!(COUNTER.counts.load(Ordering::Relaxed), 1);
            assert_eq!(COUNTER.times.load(Ordering::Relaxed), 1);
            assert_eq!(COUNTER.lograte.load(Ordering::Relaxed), 100);
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
    #[test]
    fn test_inc_new_counter() {
        //make sure that macros are syntactically correct
        //the variable is internal to the macro scope so there is no way to introspect it
        inc_new_counter!("counter-1", 1);
        inc_new_counter!("counter-2", 1, 2);
    }
    #[test]
    fn test_lograte() {
        static mut COUNTER: Counter = create_counter!("test_lograte", 0);
        inc_counter!(COUNTER, 2);
        unsafe {
            assert_eq!(
                COUNTER.lograte.load(Ordering::Relaxed),
                DEFAULT_METRICS_RATE
            );
        }
    }
    #[test]
    fn test_lograte_env() {
        assert_ne!(DEFAULT_METRICS_RATE, 0);
        static mut COUNTER: Counter = create_counter!("test_lograte_env", 0);
        env::set_var("SOLANA_DEFAULT_METRICS_RATE", "50");
        inc_counter!(COUNTER, 2);
        unsafe {
            assert_eq!(COUNTER.lograte.load(Ordering::Relaxed), 50);
        }

        static mut COUNTER2: Counter = create_counter!("test_lograte_env", 0);
        env::set_var("SOLANA_DEFAULT_METRICS_RATE", "0");
        inc_counter!(COUNTER2, 2);
        unsafe {
            assert_eq!(
                COUNTER2.lograte.load(Ordering::Relaxed),
                DEFAULT_METRICS_RATE
            );
        }
    }
}
