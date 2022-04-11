/// Use `measure!()` macro when you have an expression that you want to measure.
///
/// It has similar functionality to the Measure::this() call.
/// It will start a new `Measure`, call your expression, stop the measure, then return the `Measure`
/// object along with your expression's return value.
///
/// # Examples
/// ```
/// // With macro
/// # use solana_measure::measure;
/// # fn zero() {};
/// let (result, measure) = measure!(zero(), "zero params");
///
/// // Without macro
/// # use solana_measure::measure::Measure;
/// let (result, measure) = Measure::this(|_| zero(), (), "zero params");
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

        // Ensure that the macro can be called with a simple block
        {
            let expected = 1;
            let (actual, _measure) = measure!({ expected }, "test");
            assert_eq!(actual, expected);
        }

        // Ensure that the macro can be called with a normal function
        {
            let (result, _measure) = measure!(my_multiply(3, 4), "test");
            assert_eq!(result, 3 * 4);
        }

        // Ensure that the macro can be called with a normal function with one argument
        {
            let (result, _measure) = measure!(square(5), "test");
            assert_eq!(result, 5 * 5)
        }

        // Ensure that the macro can be called with a method (and self)
        {
            let some_struct = SomeStruct { x: 42 };
            let (result, _measure) = measure!(some_struct.add_to(4), "test");
            assert_eq!(result, 42 + 4);
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
}
