use {
    solana_sdk::timing::{duration_as_ms, duration_as_ns, duration_as_s, duration_as_us},
    std::{
        fmt,
        time::{Duration, Instant},
    },
};

#[derive(Debug)]
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

    pub fn as_ns(&self) -> u64 {
        self.duration
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

    pub fn as_duration(&self) -> Duration {
        Duration::from_nanos(self.as_ns())
    }

    pub fn end_as_ns(self) -> u64 {
        duration_as_ns(&self.start.elapsed())
    }

    pub fn end_as_us(self) -> u64 {
        duration_as_us(&self.start.elapsed())
    }

    pub fn end_as_ms(self) -> u64 {
        duration_as_ms(&self.start.elapsed())
    }

    pub fn end_as_s(self) -> f32 {
        duration_as_s(&self.start.elapsed())
    }

    pub fn end_as_duration(self) -> Duration {
        self.start.elapsed()
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
    use {
        super::*,
        std::{fmt::Debug, thread::sleep},
    };

    #[test]
    fn test_measure() {
        let mut measure = Measure::start("test");
        sleep(Duration::from_millis(100));
        measure.stop();
        assert!(measure.as_s() >= 0.09f32 && measure.as_s() <= 0.11f32);
        assert!(measure.as_ms() >= 90 && measure.as_ms() <= 110);
        assert!(measure.as_us() >= 99_000 && measure.as_us() <= 110_000);
        assert!(measure.as_ns() >= 99_000_000 && measure.as_ns() <= 110_000_000);
        assert!(
            measure.as_duration() >= Duration::from_millis(90)
                && measure.as_duration() <= Duration::from_millis(110)
        );
    }

    #[test]
    fn test_measure_end_as() {
        #[track_caller]
        fn test_end_as<Res>(method: fn(Measure) -> Res, sleep_ms: u64, lower: Res, upper: Res)
        where
            Res: PartialOrd + Debug,
        {
            let measure = Measure::start("test");
            sleep(Duration::from_millis(sleep_ms));
            let result = method(measure);
            assert!(
                result >= lower,
                "Result below the expected bound.\n\
                 Lower bound: {lower:?}\n\
                 Result: {result:?}"
            );
            assert!(
                result <= upper,
                "Result above the expected bound.\n\
                 Upper bound: {upper:?}\n\
                 Result: {result:?}"
            );
        }

        test_end_as(Measure::end_as_s, 100, 0.09f32, 0.11f32);
        test_end_as(Measure::end_as_ms, 100, 90, 110);
        test_end_as(Measure::end_as_us, 100, 90_000, 110_000);
        test_end_as(Measure::end_as_ns, 100, 90_000_000, 110_000_000);
        test_end_as(
            Measure::end_as_duration,
            100,
            Duration::from_millis(90),
            Duration::from_millis(110),
        );
    }

    #[test]
    fn test_measure_display() {
        let measure = Measure {
            name: "test_ns",
            start: Instant::now(),
            duration: 1,
        };
        assert_eq!(format!("{measure}"), "test_ns took 1ns");

        let measure = Measure {
            name: "test_us",
            start: Instant::now(),
            duration: 1000,
        };
        assert_eq!(format!("{measure}"), "test_us took 1us");

        let measure = Measure {
            name: "test_ms",
            start: Instant::now(),
            duration: 1000 * 1000,
        };
        assert_eq!(format!("{measure}"), "test_ms took 1ms");

        let measure = Measure {
            name: "test_s",
            start: Instant::now(),
            duration: 1000 * 1000 * 1000,
        };
        assert_eq!(format!("{measure}"), "test_s took 1.0s");

        let measure = Measure::start("test_not_stopped");
        assert_eq!(format!("{measure}"), "test_not_stopped running");
    }
}
