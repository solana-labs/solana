/// Measure this expression
///
/// Use `measure_time!()` when you have an expression that you want to measure.  `measure_time!()` will start
/// a new [`Measure`], evaluate your expression, stop the [`Measure`], and then return the
/// [`Measure`] object along with your expression's return value.
///
/// Use `measure_us!()` when you want to measure an expression in microseconds.
///
/// Use `meas_dur!()` when you want to measure an expression and get the Duration.
///
/// [`Measure`]: crate::measure::Measure
///
/// # Examples
///
/// ```
/// // Measure functions
/// # use solana_measure::{measure_time, measure_us, meas_dur};
/// # fn foo() {}
/// # fn bar(x: i32) {}
/// # fn add(x: i32, y: i32) -> i32 {x + y}
/// let (result, measure) = measure_time!(foo(), "foo takes no parameters");
/// let (result, measure) = measure_time!(bar(42), "bar takes one parameter");
/// let (result, measure) = measure_time!(add(1, 2), "add takes two parameters and returns a value");
/// let (result, measure_us) = measure_us!(add(1, 2));
/// let (result, duration) = meas_dur!(add(1, 2));
/// # assert_eq!(result, 1 + 2);
/// ```
///
/// ```
/// // Measure methods
/// # use solana_measure::{measure_time, measure_us, meas_dur};
/// # struct Foo {
/// #     f: i32,
/// # }
/// # impl Foo {
/// #     fn frobnicate(&self, bar: i32) -> i32 {
/// #         self.f * bar
/// #     }
/// # }
/// let foo = Foo { f: 42 };
/// let (result, measure) = measure_time!(foo.frobnicate(2), "measure methods");
/// let (result, measure_us) = measure_us!(foo.frobnicate(2));
/// let (result, duration) = meas_dur!(foo.frobnicate(2));
/// # assert_eq!(result, 42 * 2);
/// ```
///
/// ```
/// // Measure expression blocks
/// # use solana_measure::measure_time;
/// # fn complex_calculation() -> i32 { 42 }
/// # fn complex_transform(x: i32) -> i32 { x + 3 }
/// # fn record_result(y: i32) {}
/// let (result, measure) = measure_time!(
///     {
///         let x = complex_calculation();
///         # assert_eq!(x, 42);
///         let y = complex_transform(x);
///         # assert_eq!(y, 42 + 3);
///         record_result(y);
///         y
///     },
///     "measure a block of many operations",
/// );
/// # assert_eq!(result, 42 + 3);
/// ```
///
/// ```
/// // The `name` parameter is optional
/// # use solana_measure::{measure_time, measure_us};
/// # fn meow() {};
/// let (result, measure) = measure_time!(meow());
/// let (result, measure_us) = measure_us!(meow());
/// ```
#[macro_export]
macro_rules! measure_time {
    ($val:expr, $name:tt $(,)?) => {{
        let mut measure = $crate::measure::Measure::start($name);
        let result = $val;
        measure.stop();
        (result, measure)
    }};
    ($val:expr) => {
        measure_time!($val, "")
    };
}

#[macro_export]
macro_rules! measure_us {
    ($expr:expr) => {{
        let (result, duration) = $crate::meas_dur!($expr);
        (result, duration.as_micros() as u64)
    }};
}

/// Measures how long it takes to execute an expression, and returns a Duration
///
/// # Examples
///
/// ```
/// # use solana_measure::meas_dur;
/// # fn meow(x: i32, y: i32) -> i32 {x + y}
/// let (result, duration) = meas_dur!(meow(1, 2) + 3);
/// # assert_eq!(result, 1 + 2 + 3);
/// ```
//
// The macro name, `meas_dur`, is "measure" + "duration".
// When said aloud, the pronunciation is close to "measure".
#[macro_export]
macro_rules! meas_dur {
    ($expr:expr) => {{
        let start = std::time::Instant::now();
        let result = $expr;
        (result, start.elapsed())
    }};
}

#[cfg(test)]
mod tests {
    use std::{thread::sleep, time::Duration};

    fn my_multiply(x: i32, y: i32) -> i32 {
        x * y
    }

    fn square(x: i32) -> i32 {
        my_multiply(x, x)
    }

    struct SomeStruct {
        x: i32,
    }
    impl SomeStruct {
        fn add_to(&self, x: i32) -> i32 {
            x + self.x
        }
    }

    #[test]
    fn test_measure_macro() {
        // Ensure that the measurement side actually works
        {
            let (_result, measure) = measure_time!(sleep(Duration::from_millis(1)), "test");
            assert!(measure.as_s() > 0.0);
            assert!(measure.as_ms() > 0);
            assert!(measure.as_us() > 0);
        }

        // Ensure that the macro can be called with functions
        {
            let (result, _measure) = measure_time!(my_multiply(3, 4), "test");
            assert_eq!(result, 3 * 4);

            let (result, _measure) = measure_time!(square(5), "test");
            assert_eq!(result, 5 * 5)
        }

        // Ensure that the macro can be called with methods
        {
            let some_struct = SomeStruct { x: 42 };
            let (result, _measure) = measure_time!(some_struct.add_to(4), "test");
            assert_eq!(result, 42 + 4);
        }

        // Ensure that the macro can be called with blocks
        {
            let (result, _measure) = measure_time!({ 1 + 2 }, "test");
            assert_eq!(result, 3);
        }

        // Ensure that the macro can be called with a trailing comma
        {
            let (result, _measure) = measure_time!(square(5), "test",);
            assert_eq!(result, 5 * 5)
        }

        // Ensure that the macro can be called without a name
        {
            let (result, _measure) = measure_time!(square(5));
            assert_eq!(result, 5 * 5)
        }
    }

    #[test]
    fn test_measure_us_macro() {
        // Ensure that the measurement side actually works
        {
            let (_result, measure) = measure_us!(sleep(Duration::from_millis(1)));
            assert!(measure > 0);
        }

        // Ensure that the macro can be called with functions
        {
            let (result, _measure) = measure_us!(my_multiply(3, 4));
            assert_eq!(result, 3 * 4);

            let (result, _measure) = measure_us!(square(5));
            assert_eq!(result, 5 * 5)
        }

        // Ensure that the macro can be called with methods
        {
            let some_struct = SomeStruct { x: 42 };
            let (result, _measure) = measure_us!(some_struct.add_to(4));
            assert_eq!(result, 42 + 4);
        }

        // Ensure that the macro can be called with blocks
        {
            let (result, _measure) = measure_us!({ 1 + 2 });
            assert_eq!(result, 3);
        }
    }

    #[test]
    fn test_meas_dur_macro() {
        // Ensure that the macro can be called with functions
        {
            let (result, _duration) = meas_dur!(my_multiply(3, 4));
            assert_eq!(result, 3 * 4);

            let (result, _duration) = meas_dur!(square(5));
            assert_eq!(result, 5 * 5)
        }

        // Ensure that the macro can be called with methods
        {
            let some_struct = SomeStruct { x: 42 };
            let (result, _duration) = meas_dur!(some_struct.add_to(4));
            assert_eq!(result, 42 + 4);
        }

        // Ensure that the macro can be called with blocks
        {
            let (result, _duration) = meas_dur!({ 1 + 2 });
            assert_eq!(result, 3);
        }
    }
}
