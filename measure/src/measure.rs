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

    /// Wrap a function with Measure
    ///
    /// Use `Measure::wrap()` when you have a function that you want to measure.  `wrap()` will
    /// start a new `Measure`, call your function, stop the measure, then return the `Measure`
    /// object along with your function's return value.
    ///
    /// You will likely need to wrap your function in a closure, and wrap the arguments in a tuple.
    /// See the tests for more details.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Call a function
    /// let (result, measure) = Measure::wrap(|(arg1, arg2)| my_function(arg1, arg2), ("abc", 123), "my_func");
    /// ```
    ///
    /// ```ignore
    /// /// Call a method
    /// struct Foo { ...  }
    /// impl Foo { fn bar(&self) { ... } }
    ///
    /// let foo = Foo { };
    /// let (result, measure) = Measure::wrap(|this| Foo::bar(&this), &foo, "bar");
    /// ```
    pub fn wrap<T, R, F: FnOnce(T) -> R>(func: F, args: T, name: &'static str) -> (R, Self) {
        let mut measure = Self::start(name);
        let result = func(args);
        measure.stop();
        (result, measure)
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

    fn my_multiply(x: i32, y: i32) -> i32 {
        x * y
    }

    struct Foo {
        x: i32,
    }
    impl Foo {
        fn add_to(&self, x: i32) -> i32 {
            x + self.x
        }
    }

    #[test]
    fn test_measure_with() {
        // Ensure that the measurement side actually works
        {
            let (_result, measure) = Measure::wrap(|s| sleep(Duration::from_secs(s)), 1, "test");
            assert!(measure.as_s() >= 0.99f32 && measure.as_s() <= 1.01f32);
            assert!(measure.as_ms() >= 990 && measure.as_ms() <= 1_010);
            assert!(measure.as_us() >= 999_000 && measure.as_us() <= 1_010_000);
        }

        // Ensure that wrap() can be called with a simple closure
        {
            let expected = 1;
            let (actual, _measure) = Measure::wrap(|x| x, expected, "test");
            assert_eq!(actual, expected);
        }

        // Ensure that wrap() can be called with a tuple
        {
            let (result, _measure) = Measure::wrap(|(x, y)| x + y, (1, 2), "test");
            assert_eq!(result, 1 + 2);
        }

        // Ensure that wrap() can be called with a normal function
        {
            let (result, _measure) = Measure::wrap(|(x, y)| my_multiply(x, y), (3, 4), "test");
            assert_eq!(result, 3 * 4);
        }

        // Ensure that wrap() can be called with a method (and self)
        {
            let foo = Foo { x: 42 };
            let (result, _measure) =
                Measure::wrap(|(this, x)| Foo::add_to(&this, x), (foo, 4), "test");
            assert_eq!(result, 42 + 4);
        }

        // Ensure that wrap() can be called with a method (and &self)
        {
            let foo = Foo { x: 42 };
            let (result, _measure) =
                Measure::wrap(|(this, x)| Foo::add_to(&this, x), (&foo, 4), "test");
            assert_eq!(result, 42 + 4);
            assert_eq!(foo.add_to(6), 42 + 6);
        }
    }
}
