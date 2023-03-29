/// Measure this expression
///
/// Use `measure!()` when you have an expression that you want to measure.  `measure!()` will start
/// a new [`Measure`], evaluate your expression, stop the [`Measure`], and then return the
/// [`Measure`] object along with your expression's return value.
///
/// Use `measure_us!()` when you want to measure an expression in microseconds.
///
/// [`Measure`]: crate::measure::Measure
///
/// # Examples
///
/// ```
/// // Measure functions
/// # use solana_measure::{measure, measure_us};
/// # fn foo() {}
/// # fn bar(x: i32) {}
/// # fn add(x: i32, y: i32) -> i32 {x + y}
/// let (result, measure) = measure!(foo(), "foo takes no parameters");
/// let (result, measure) = measure!(bar(42), "bar takes one parameter");
/// let (result, measure) = measure!(add(1, 2), "add takes two parameters and returns a value");
/// let (result, measure_us) = measure_us!(add(1, 2));
/// # assert_eq!(result, 1 + 2);
/// ```
///
/// ```
/// // Measure methods
/// # use solana_measure::{measure, measure_us};
/// # struct Foo {
/// #     f: i32,
/// # }
/// # impl Foo {
/// #     fn frobnicate(&self, bar: i32) -> i32 {
/// #         self.f * bar
/// #     }
/// # }
/// let foo = Foo { f: 42 };
/// let (result, measure) = measure!(foo.frobnicate(2), "measure methods");
/// let (result, measure_us) = measure_us!(foo.frobnicate(2));
/// # assert_eq!(result, 42 * 2);
/// ```
///
/// ```
/// // Measure expression blocks
/// # use solana_measure::measure;
/// # fn complex_calculation() -> i32 { 42 }
/// # fn complex_transform(x: i32) -> i32 { x + 3 }
/// # fn record_result(y: i32) {}
/// let (result, measure) = measure!(
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
/// # use solana_measure::{measure, measure_us};
/// # fn meow() {};
/// let (result, measure) = measure!(meow());
/// let (result, measure_us) = measure_us!(meow());
/// ```
#[macro_export]
macro_rules! measure {
    ($val:expr, $name:tt $(,)?) => {{
        let mut measure = $crate::measure::Measure::start($name);
        let result = $val;
        measure.stop();
        (result, measure)
    }};
    ($val:expr) => {
        measure!($val, "")
    };
}

#[macro_export]
macro_rules! measure_us {
    ($val:expr) => {{
        let start = std::time::Instant::now();
        let result = $val;
        (result, solana_sdk::timing::duration_as_us(&start.elapsed()))
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
            let (_result, measure) = measure!(sleep(Duration::from_secs(1)), "test");
            assert!(measure.as_s() >= 0.99f32 && measure.as_s() <= 1.01f32);
            assert!(measure.as_ms() >= 990 && measure.as_ms() <= 1_010);
            assert!(measure.as_us() >= 999_000 && measure.as_us() <= 1_010_000);
        }

        // Ensure that the macro can be called with functions
        {
            let (result, _measure) = measure!(my_multiply(3, 4), "test");
            assert_eq!(result, 3 * 4);

            let (result, _measure) = measure!(square(5), "test");
            assert_eq!(result, 5 * 5)
        }

        // Ensure that the macro can be called with methods
        {
            let some_struct = SomeStruct { x: 42 };
            let (result, _measure) = measure!(some_struct.add_to(4), "test");
            assert_eq!(result, 42 + 4);
        }

        // Ensure that the macro can be called with blocks
        {
            let (result, _measure) = measure!({ 1 + 2 }, "test");
            assert_eq!(result, 3);
        }

        // Ensure that the macro can be called with a trailing comma
        {
            let (result, _measure) = measure!(square(5), "test",);
            assert_eq!(result, 5 * 5)
        }

        // Ensure that the macro can be called without a name
        {
            let (result, _measure) = measure!(square(5));
            assert_eq!(result, 5 * 5)
        }
    }

    #[test]
    fn test_measure_us_macro() {
        // Ensure that the measurement side actually works
        {
            let (_result, measure) = measure_us!(sleep(Duration::from_secs(1)));
            assert!((999_000..=1_010_000).contains(&measure));
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
}
