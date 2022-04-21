/// return the bit width of the integer type
pub const fn num_bits<T>() -> u32 {
    (std::mem::size_of::<T>() * 8) as u32
}

/// generate fn to compute the floor of log2 for integer types
macro_rules! gen_log2_floor {
    ($n: ident, $t: ty) => {
        pub fn $n(x: $t) -> $t {
            (num_bits::<$t>() - x.leading_zeros() - 1) as $t
        }
    };
}
gen_log2_floor!(log2_floor_u32, u32);
gen_log2_floor!(log2_floor_u64, u64);

/// generate fn to compute the ceil of log2 for integer types
macro_rules! gen_log2_ceil {
    ($n: ident, $t: ty) => {
        pub fn $n(x: $t) -> $t {
            (num_bits::<$t>() - (x - 1 as $t).leading_zeros()) as $t
        }
    };
}
gen_log2_ceil!(log2_ceil_u32, u32);
gen_log2_ceil!(log2_ceil_u64, u64);

/// generate fn to compute the pow(2, exponent) unchecked for integer types
macro_rules! gen_pow2_unchecked {
    ($n: ident, $t: ty) => {
        pub fn $n(exponent: $t) -> $t {
            (1_u64 << exponent) as $t
        }
    };
}
gen_pow2_unchecked!(pow2_unchecked_u32, u32);
gen_pow2_unchecked!(pow2_unchecked_u64, u64);

/// generate fn to compute the pow(2, exponent) checked for integer types
macro_rules! gen_pow2_checked {
    ($n: ident, $t: ty) => {
        pub fn $n(exponent: $t) -> Option<$t> {
            1_u64.checked_shl(exponent as u32).map(|x| x as $t)
        }
    };
}
gen_pow2_checked!(pow2_checked_u32, u32);
gen_pow2_checked!(pow2_checked_u64, u64);

/// generate fn to check if the number is power of 2
macro_rules! gen_is_pow2 {
    ($n: ident, $t: ty) => {
        pub fn $n(x: $t) -> bool {
            0 == (x & (x - 1))
        }
    };
}
gen_is_pow2!(is_pow2_u32, u32);
gen_is_pow2!(is_pow2_u64, u64);

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_num_bits() {
        assert_eq!(num_bits::<i8>(), 8);
        assert_eq!(num_bits::<i16>(), 16);
        assert_eq!(num_bits::<i32>(), 32);
        assert_eq!(num_bits::<i64>(), 64);
        assert_eq!(num_bits::<u8>(), 8);
        assert_eq!(num_bits::<u16>(), 16);
        assert_eq!(num_bits::<u32>(), 32);
        assert_eq!(num_bits::<u64>(), 64);
    }

    macro_rules! gen_log2_floor_test {
        ($test_name: ident, $fn_name: ident, $t: ty) => {
            #[test]
            fn $test_name() {
                for n in 1..512 {
                    assert_eq!($fn_name(n), (n as f64).log2().floor() as $t);
                }

                let x: $t = u16::MAX.into();
                assert_eq!($fn_name(x), 15);
                assert_eq!($fn_name(x + 1), 16);
                assert_eq!($fn_name(x + 2), 16);
            }
        };
    }

    gen_log2_floor_test!(test_log2_floor_u32, log2_floor_u32, u32);
    gen_log2_floor_test!(test_log2_floor_u64, log2_floor_u64, u64);

    macro_rules! gen_log2_ceil_test {
        ($test_name: ident, $fn_name: ident, $t: ty) => {
            #[test]
            fn $test_name() {
                for n in 1..512 {
                    assert_eq!($fn_name(n), (n as f64).log2().ceil() as $t);
                }

                let x: $t = u16::MAX.into();
                assert_eq!($fn_name(x), 16);
                assert_eq!($fn_name(x + 1), 16);
                assert_eq!($fn_name(x + 2), 17);
            }
        };
    }

    gen_log2_ceil_test!(test_log2_ceil_u32, log2_ceil_u32, u32);
    gen_log2_ceil_test!(test_log2_ceil_u64, log2_ceil_u64, u64);

    macro_rules! gen_pow2_unchecked_test {
        ($test_name: ident, $fn_name: ident, $t: ty) => {
            #[test]
            fn $test_name() {
                for i in 1..10 {
                    assert_eq!($fn_name(i), (2 as $t).pow(i as u32) as $t);
                }
            }
        };
    }

    gen_pow2_unchecked_test!(test_pow2_unchecked_u32, pow2_unchecked_u32, u32);
    gen_pow2_unchecked_test!(test_pow2_unchecked_u64, pow2_unchecked_u64, u64);

    macro_rules! gen_pow2_checked_test {
        ($test_name: ident, $fn_name: ident, $t: ty) => {
            #[test]
            fn $test_name() {
                for i in 1..10 {
                    assert_eq!($fn_name(i).unwrap(), (2 as $t).pow(i as u32) as $t);
                }
            }
        };
    }

    gen_pow2_checked_test!(test_pow2_checked_u32, pow2_checked_u32, u32);
    gen_pow2_checked_test!(test_pow2_checked_u64, pow2_checked_u64, u64);

    macro_rules! gen_is_pow2_test {
        ($test_name: ident, $fn_name: ident, $t: ty) => {
            #[test]
            fn $test_name() {
                for i in 1..10 {
                    assert!($fn_name(1 << i as $t));
                }

                for i in 1..10 {
                    assert!(!$fn_name(3 << i as $t));
                }
            }
        };
    }

    gen_is_pow2_test!(test_is_pow2_u32, is_pow2_u32, u32);
    gen_is_pow2_test!(test_is_pow2_u64, is_pow2_u64, u64);
}
