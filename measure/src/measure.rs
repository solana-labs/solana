use solana_sdk::timing::duration_as_ns;
use std::{fmt, time::Instant};

pub struct Measure {
    name: &'static str,
    start: Instant,
    duration: u64,
}

impl Measure {
    pub fn start(name: &'static str) -> Self {
        Self {
            name,
            start: Instant::now(),
            duration: 0,
        }
    }

    pub fn stop(&mut self) {
        self.duration = duration_as_ns(&self.start.elapsed());
    }

    pub fn as_us(&self) -> u64 {
        self.duration / 1000
    }

    pub fn as_ms(&self) -> u64 {
        self.duration / (1000 * 1000)
    }

    pub fn as_s(&self) -> f32 {
        self.duration as f32 / (1000.0f32 * 1000.0f32 * 1000.0f32)
    }
}

impl fmt::Display for Measure {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.duration == 0 {
            write!(f, "{} running", self.name)
        } else if self.as_us() < 1 {
            write!(f, "{} took {}ns", self.name, self.duration)
        } else if self.as_ms() < 1 {
            write!(f, "{} took {}us", self.name, self.as_us())
        } else if self.as_s() < 1. {
            write!(f, "{} took {}ms", self.name, self.as_ms())
        } else {
            write!(f, "{} took {:.1}s", self.name, self.as_s())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    #[test]
    fn test_measure() {
        let mut measure = Measure::start("test");
        sleep(Duration::from_secs(1));
        measure.stop();
        assert!(measure.as_s() >= 0.99f32 && measure.as_s() <= 1.01f32);
        assert!(measure.as_ms() >= 990 && measure.as_ms() <= 1_010);
        assert!(measure.as_us() >= 999_000 && measure.as_us() <= 1_010_000);
    }

    #[test]
    fn test_measure_display() {
        let measure = Measure {
            name: "test_ns",
            start: Instant::now(),
            duration: 1,
        };
        assert_eq!(format!("{}", measure), "test_ns took 1ns");

        let measure = Measure {
            name: "test_us",
            start: Instant::now(),
            duration: 1000,
        };
        assert_eq!(format!("{}", measure), "test_us took 1us");

        let measure = Measure {
            name: "test_ms",
            start: Instant::now(),
            duration: 1000 * 1000,
        };
        assert_eq!(format!("{}", measure), "test_ms took 1ms");

        let measure = Measure {
            name: "test_s",
            start: Instant::now(),
            duration: 1000 * 1000 * 1000,
        };
        assert_eq!(format!("{}", measure), "test_s took 1.0s");

        let measure = Measure::start("test_not_stopped");
        assert_eq!(format!("{}", measure), "test_not_stopped running");
    }
}
