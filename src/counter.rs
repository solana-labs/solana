use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use timing;

pub struct Counter {
    pub name: &'static str,
    pub counts: AtomicUsize,
    pub nanos: AtomicUsize,
    pub times: AtomicUsize,
    pub lograte: usize,
}

macro_rules! create_counter {
    ($name:expr, $lograte:expr) => {
        Counter {
            name: $name,
            counts: AtomicUsize::new(0),
            nanos: AtomicUsize::new(0),
            times: AtomicUsize::new(0),
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
            assert_eq!(COUNTER.name, "test");
        }
    }
}
