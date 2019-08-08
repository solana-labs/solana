use crate::{influxdb, submit};
use log::*;
use solana_sdk::timing;
use std::env;
use std::sync::atomic::{AtomicUsize, Ordering};

const DEFAULT_LOG_RATE: usize = 1000;
const DEFAULT_METRICS_RATE: usize = 1;
const DEFAULT_METRICS_HIGH_RATE: usize = 10;

/// Use default metrics high rate
pub const HIGH_RATE: usize = 999_999;

pub struct Counter {
    pub name: &'static str,
    /// total accumulated value
    pub counts: AtomicUsize,
    pub times: AtomicUsize,
    /// last accumulated value logged
    pub lastlog: AtomicUsize,
    pub lograte: AtomicUsize,
    pub metricsrate: AtomicUsize,
    pub point: Option<influxdb::Point>,
}

#[macro_export]
macro_rules! create_counter {
    ($name:expr, $lograte:expr, $metricsrate:expr) => {
        $crate::counter::Counter {
            name: $name,
            counts: std::sync::atomic::AtomicUsize::new(0),
            times: std::sync::atomic::AtomicUsize::new(0),
            lastlog: std::sync::atomic::AtomicUsize::new(0),
            lograte: std::sync::atomic::AtomicUsize::new($lograte),
            metricsrate: std::sync::atomic::AtomicUsize::new($metricsrate),
            point: None,
        }
    };
}

#[macro_export]
macro_rules! inc_counter {
    ($name:expr, $level:expr, $count:expr) => {
        unsafe {
            if log_enabled!($level) {
                $name.inc($level, $count)
            }
        };
    };
}

#[macro_export]
macro_rules! inc_counter_info {
    ($name:expr, $count:expr) => {
        unsafe {
            if log_enabled!(log::Level::Info) {
                $name.inc(log::Level::Info, $count)
            }
        };
    };
}

#[macro_export]
macro_rules! inc_new_counter {
    ($name:expr, $count:expr, $level:expr, $lograte:expr, $metricsrate:expr) => {{
        if log_enabled!($level) {
            static mut INC_NEW_COUNTER: $crate::counter::Counter =
                create_counter!($name, $lograte, $metricsrate);
            static INIT_HOOK: std::sync::Once = std::sync::Once::new();
            unsafe {
                INIT_HOOK.call_once(|| {
                    INC_NEW_COUNTER.init();
                });
            }
            inc_counter!(INC_NEW_COUNTER, $level, $count);
        }
    }};
}

#[macro_export]
macro_rules! inc_new_counter_error {
    ($name:expr, $count:expr) => {{
        inc_new_counter!($name, $count, log::Level::Error, 0, 0);
    }};
    ($name:expr, $count:expr, $lograte:expr) => {{
        inc_new_counter!($name, $count, log::Level::Error, $lograte, 0);
    }};
    ($name:expr, $count:expr, $lograte:expr, $metricsrate:expr) => {{
        inc_new_counter!($name, $count, log::Level::Error, $lograte, $metricsrate);
    }};
}

#[macro_export]
macro_rules! inc_new_counter_warn {
    ($name:expr, $count:expr) => {{
        inc_new_counter!($name, $count, log::Level::Warn, 0, 0);
    }};
    ($name:expr, $count:expr, $lograte:expr) => {{
        inc_new_counter!($name, $count, log::Level::Warn, $lograte, 0);
    }};
    ($name:expr, $count:expr, $lograte:expr, $metricsrate:expr) => {{
        inc_new_counter!($name, $count, log::Level::Warn, $lograte, $metricsrate);
    }};
}

#[macro_export]
macro_rules! inc_new_counter_info {
    ($name:expr, $count:expr) => {{
        inc_new_counter!($name, $count, log::Level::Info, 0, 0);
    }};
    ($name:expr, $count:expr, $lograte:expr) => {{
        inc_new_counter!($name, $count, log::Level::Info, $lograte, 0);
    }};
    ($name:expr, $count:expr, $lograte:expr, $metricsrate:expr) => {{
        inc_new_counter!($name, $count, log::Level::Info, $lograte, $metricsrate);
    }};
}

#[macro_export]
macro_rules! inc_new_counter_debug {
    ($name:expr, $count:expr) => {{
        inc_new_counter!($name, $count, log::Level::Debug, 0, 0);
    }};
    ($name:expr, $count:expr, $lograte:expr) => {{
        inc_new_counter!($name, $count, log::Level::Debug, $lograte, 0);
    }};
    ($name:expr, $count:expr, $lograte:expr, $metricsrate:expr) => {{
        inc_new_counter!($name, $count, log::Level::Debug, $lograte, $metricsrate);
    }};
}

#[macro_export]
macro_rules! inc_new_high_rate_counter_error {
    ($name:expr, $count:expr) => {{
        inc_new_counter!(
            $name,
            $count,
            log::Level::Error,
            0,
            $crate::counter::HIGH_RATE
        );
    }};
}

#[macro_export]
macro_rules! inc_new_high_rate_counter_warn {
    ($name:expr, $count:expr) => {{
        inc_new_counter!(
            $name,
            $count,
            log::Level::Warn,
            0,
            $crate::counter::HIGH_RATE
        );
    }};
}

#[macro_export]
macro_rules! inc_new_high_rate_counter_info {
    ($name:expr, $count:expr) => {{
        inc_new_counter!(
            $name,
            $count,
            log::Level::Info,
            0,
            $crate::counter::HIGH_RATE
        );
    }};
}

#[macro_export]
macro_rules! inc_new_high_rate_counter_debug {
    ($name:expr, $count:expr) => {{
        inc_new_counter!(
            $name,
            $count,
            log::Level::Debug,
            0,
            $crate::counter::HIGH_RATE
        );
    }};
}

impl Counter {
    fn default_metrics_high_rate() -> usize {
        let v = env::var("SOLANA_METRICS_HIGH_RATE")
            .map(|x| x.parse().unwrap_or(0))
            .unwrap_or(0);
        if v == 0 {
            DEFAULT_METRICS_HIGH_RATE
        } else {
            v
        }
    }
    fn default_log_rate() -> usize {
        let v = env::var("SOLANA_DEFAULT_LOG_RATE")
            .map(|x| x.parse().unwrap_or(DEFAULT_LOG_RATE))
            .unwrap_or(DEFAULT_LOG_RATE);
        if v == 0 {
            DEFAULT_LOG_RATE
        } else {
            v
        }
    }
    pub fn init(&mut self) {
        self.point = Some(
            influxdb::Point::new(&self.name)
                .add_field("count", influxdb::Value::Integer(0))
                .to_owned(),
        );
        self.lograte
            .compare_and_swap(0, Self::default_log_rate(), Ordering::Relaxed);
        self.metricsrate.compare_and_swap(
            HIGH_RATE,
            Self::default_metrics_high_rate(),
            Ordering::Relaxed,
        );
        self.metricsrate
            .compare_and_swap(0, DEFAULT_METRICS_RATE, Ordering::Relaxed);
    }
    pub fn inc(&mut self, level: log::Level, events: usize) {
        let counts = self.counts.fetch_add(events, Ordering::Relaxed);
        let times = self.times.fetch_add(1, Ordering::Relaxed);
        let lograte = self.lograte.load(Ordering::Relaxed);
        let metricsrate = self.metricsrate.load(Ordering::Relaxed);

        if times % lograte == 0 && times > 0 && log_enabled!(level) {
            log!(level,
                "COUNTER:{{\"name\": \"{}\", \"counts\": {}, \"samples\": {},  \"now\": {}, \"events\": {}}}",
                self.name,
                counts + events,
                times,
                timing::timestamp(),
                events,
            );
        }

        if times % metricsrate == 0 && times > 0 {
            let lastlog = self.lastlog.load(Ordering::Relaxed);
            let prev = self
                .lastlog
                .compare_and_swap(lastlog, counts, Ordering::Relaxed);
            if prev == lastlog {
                if let Some(ref mut point) = self.point {
                    point
                        .fields
                        .entry("count".to_string())
                        .and_modify(|v| {
                            *v = influxdb::Value::Integer(counts as i64 - lastlog as i64)
                        })
                        .or_insert(influxdb::Value::Integer(0));
                }
                if let Some(ref mut point) = self.point {
                    submit(point.to_owned(), level);
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use crate::counter::{Counter, DEFAULT_LOG_RATE, DEFAULT_METRICS_HIGH_RATE, HIGH_RATE};
    use log::Level;
    use log::*;
    use serial_test_derive::serial;
    use std::env;
    use std::sync::atomic::Ordering;
    use std::sync::{Once, RwLock};

    fn get_env_lock() -> &'static RwLock<()> {
        static mut ENV_LOCK: Option<RwLock<()>> = None;
        static INIT_HOOK: Once = Once::new();

        unsafe {
            INIT_HOOK.call_once(|| {
                ENV_LOCK = Some(RwLock::new(()));
            });
            &ENV_LOCK.as_ref().unwrap()
        }
    }

    #[test]
    #[serial]
    fn test_counter() {
        env_logger::Builder::from_env(env_logger::Env::new().default_filter_or("solana=info"))
            .try_init()
            .ok();
        let _readlock = get_env_lock().read();
        static mut COUNTER: Counter = create_counter!("test", 1000, 1);
        unsafe {
            COUNTER.init();
        }
        let count = 1;
        inc_counter!(COUNTER, Level::Info, count);
        unsafe {
            assert_eq!(COUNTER.counts.load(Ordering::Relaxed), 1);
            assert_eq!(COUNTER.times.load(Ordering::Relaxed), 1);
            assert_eq!(COUNTER.lograte.load(Ordering::Relaxed), 1000);
            assert_eq!(COUNTER.lastlog.load(Ordering::Relaxed), 0);
            assert_eq!(COUNTER.name, "test");
        }
        for _ in 0..199 {
            inc_counter!(COUNTER, Level::Info, 2);
        }
        unsafe {
            assert_eq!(COUNTER.lastlog.load(Ordering::Relaxed), 397);
        }
        inc_counter!(COUNTER, Level::Info, 2);
        unsafe {
            assert_eq!(COUNTER.lastlog.load(Ordering::Relaxed), 399);
        }
    }

    #[test]
    #[serial]
    fn test_high_rate_counter() {
        env_logger::Builder::from_env(env_logger::Env::new().default_filter_or("solana=info"))
            .try_init()
            .ok();
        let _readlock = get_env_lock().read();
        static mut COUNTER: Counter = create_counter!("test", 1000, HIGH_RATE);
        env::remove_var("SOLANA_METRICS_HIGH_RATE");
        unsafe {
            COUNTER.init();
            assert_eq!(
                COUNTER.metricsrate.load(Ordering::Relaxed),
                DEFAULT_METRICS_HIGH_RATE
            );
        }
    }

    #[test]
    #[serial]
    fn test_high_rate_counter_env() {
        env_logger::Builder::from_env(env_logger::Env::new().default_filter_or("solana=info"))
            .try_init()
            .ok();
        let _writelock = get_env_lock().write();
        static mut COUNTER: Counter = create_counter!("test", 1000, HIGH_RATE);
        env::set_var("SOLANA_METRICS_HIGH_RATE", "50");
        unsafe {
            COUNTER.init();
            assert_eq!(COUNTER.metricsrate.load(Ordering::Relaxed), 50);
        }
    }

    #[test]
    #[serial]
    fn test_inc_new_counter() {
        let _readlock = get_env_lock().read();
        //make sure that macros are syntactically correct
        //the variable is internal to the macro scope so there is no way to introspect it
        inc_new_counter_info!("1", 1);
        inc_new_counter_info!("2", 1, 3);
        inc_new_counter_info!("3", 1, 2, 1);
    }

    #[test]
    #[serial]
    fn test_lograte() {
        env_logger::Builder::from_env(env_logger::Env::new().default_filter_or("solana=info"))
            .try_init()
            .ok();
        let _readlock = get_env_lock().read();
        assert_eq!(
            Counter::default_log_rate(),
            DEFAULT_LOG_RATE,
            "default_log_rate() is {}, expected {}, SOLANA_DEFAULT_LOG_RATE environment variable set?",
            Counter::default_log_rate(),
            DEFAULT_LOG_RATE,
        );
        static mut COUNTER: Counter = create_counter!("test_lograte", 0, 1);
        unsafe {
            COUNTER.init();
            assert_eq!(COUNTER.lograte.load(Ordering::Relaxed), DEFAULT_LOG_RATE);
        }
    }

    #[test]
    #[serial]
    fn test_lograte_env() {
        env_logger::Builder::from_env(env_logger::Env::new().default_filter_or("solana=info"))
            .try_init()
            .ok();
        assert_ne!(DEFAULT_LOG_RATE, 0);
        let _writelock = get_env_lock().write();
        static mut COUNTER: Counter = create_counter!("test_lograte_env", 0, 1);
        env::set_var("SOLANA_DEFAULT_LOG_RATE", "50");
        unsafe {
            COUNTER.init();
            assert_eq!(COUNTER.lograte.load(Ordering::Relaxed), 50);
        }

        static mut COUNTER2: Counter = create_counter!("test_lograte_env", 0, 1);
        env::set_var("SOLANA_DEFAULT_LOG_RATE", "0");
        unsafe {
            COUNTER2.init();
            assert_eq!(COUNTER2.lograte.load(Ordering::Relaxed), DEFAULT_LOG_RATE);
        }
    }
}
