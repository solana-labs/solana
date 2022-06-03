use std::{
    convert::TryFrom,
    fmt,
    iter::Sum,
    ops::{Add, Div, Mul, Sub},
};

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Rational {
    pub numerator: u64,
    pub denominator: u64,
}

impl PartialOrd for Rational {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        if self.denominator == 0 || other.denominator == 0 {
            None
        } else {
            let x = self.numerator as u128 * other.denominator as u128;
            let y = other.numerator as u128 * self.denominator as u128;
            Some(x.cmp(&y))
        }
    }
}

impl Div for Rational {
    type Output = f64;

    // We do not return a `Rational` here because `self.numerator *
    // rhs.denominator` or `rhs.numerator * self.denominator`could overflow.
    // Instead we deal with floating point numbers.
    fn div(self, rhs: Self) -> Self::Output {
        (self.numerator as f64 * rhs.denominator as f64)
            / (self.denominator as f64 * rhs.numerator as f64)
    }
}

impl Rational {
    pub fn to_f64(&self) -> f64 {
        self.numerator as f64 / self.denominator as f64
    }
}

/// Error returned when a calculation in a token type overflows, underflows, or divides by zero.
#[derive(Debug, Eq, PartialEq)]
pub struct ArithmeticError;

pub type Result<T> = std::result::Result<T, ArithmeticError>;

/// Generate a token type that wraps the minimal unit of the token, it’s
/// “Lamport”. The symbol is for 10<sup>9</sup> of its minimal units and is
/// only used for `Debug` and `Display` printing.
#[macro_export]
macro_rules! impl_token {
    ($TokenLamports:ident, $symbol:expr, decimals = $decimals:expr) => {
        #[derive(Copy, Clone, Default, Eq, Ord, PartialEq, PartialOrd)]
        pub struct $TokenLamports(pub u64);

        impl fmt::Display for $TokenLamports {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(
                    f,
                    "{}.{} {}",
                    self.0 / 10u64.pow($decimals),
                    &format!("{:0>9}", self.0 % 10u64.pow($decimals))[9 - $decimals..],
                    $symbol
                )
            }
        }

        impl fmt::Debug for $TokenLamports {
            fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
                fmt::Display::fmt(self, f)
            }
        }

        impl Mul<Rational> for $TokenLamports {
            type Output = Result<$TokenLamports>;
            fn mul(self, other: Rational) -> Result<$TokenLamports> {
                // This multiplication cannot overflow, because we expand the
                // u64s into u128, and u64::MAX * u64::MAX < u128::MAX.
                let result_u128 = ((self.0 as u128) * (other.numerator as u128))
                    .checked_div(other.denominator as u128)
                    .ok_or(ArithmeticError)?;
                u64::try_from(result_u128)
                    .map($TokenLamports)
                    .map_err(|_| ArithmeticError)
            }
        }

        impl Mul<u64> for $TokenLamports {
            type Output = Result<$TokenLamports>;
            fn mul(self, other: u64) -> Result<$TokenLamports> {
                self.0
                    .checked_mul(other)
                    .map($TokenLamports)
                    .ok_or(ArithmeticError)
            }
        }

        impl Div<u64> for $TokenLamports {
            type Output = Result<$TokenLamports>;
            fn div(self, other: u64) -> Result<$TokenLamports> {
                self.0
                    .checked_div(other)
                    .map($TokenLamports)
                    .ok_or(ArithmeticError)
            }
        }

        impl Sub<$TokenLamports> for $TokenLamports {
            type Output = Result<$TokenLamports>;
            fn sub(self, other: $TokenLamports) -> Result<$TokenLamports> {
                self.0
                    .checked_sub(other.0)
                    .map($TokenLamports)
                    .ok_or(ArithmeticError)
            }
        }

        impl Add<$TokenLamports> for $TokenLamports {
            type Output = Result<$TokenLamports>;
            fn add(self, other: $TokenLamports) -> Result<$TokenLamports> {
                self.0
                    .checked_add(other.0)
                    .map($TokenLamports)
                    .ok_or(ArithmeticError)
            }
        }

        impl Sum<$TokenLamports> for Result<$TokenLamports> {
            fn sum<I: Iterator<Item = $TokenLamports>>(iter: I) -> Self {
                let mut sum = $TokenLamports(0);
                for item in iter {
                    sum = (sum + item)?;
                }
                Ok(sum)
            }
        }
        /// Parse a numeric string as an amount of Lamports, i.e., with 9 digit precision.
        ///
        /// Note that this parses the Lamports amount divided by 10<sup>9</sup>,
        /// which can include a decimal point. It does not parse the number of
        /// Lamports! This makes this function the semi-inverse of `Display`
        /// (only `Display` adds the suffixes, and we do not expect that
        /// here).
        impl std::str::FromStr for $TokenLamports {
            type Err = &'static str;
            fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
                let mut value = 0_u64;
                let mut is_after_decimal = false;
                let mut exponent: i32 = $decimals;
                let mut had_digit = false;

                // Walk the bytes one by one, we only expect ASCII digits or '.', so bytes
                // suffice. We build up the value as we go, and if we get past the decimal
                // point, we also track how far we are past it.
                for ch in s.as_bytes() {
                    match ch {
                        b'0'..=b'9' => {
                            value = value * 10 + ((ch - b'0') as u64);
                            if is_after_decimal {
                                exponent -= 1;
                            }
                            had_digit = true;
                        }
                        b'.' if !is_after_decimal => is_after_decimal = true,
                        b'.' => return Err("Value can contain at most one '.' (decimal point)."),
                        b'_' => { /* As a courtesy, allow numeric underscores for readability. */ }
                        _ => return Err("Invalid value, only digits, '_', and '.' are allowed."),
                    }

                    if exponent < 0 {
                        return Err("Value can contain at most 9 digits after the decimal point.");
                    }
                }

                if !had_digit {
                    return Err("Value must contain at least one digit.");
                }

                // If the value contained fewer than 9 digits behind the decimal point
                // (or no decimal point at all), scale up the value so it is measured
                // in lamports.
                while exponent > 0 {
                    value *= 10;
                    exponent -= 1;
                }

                Ok($TokenLamports(value))
            }
        }
    };
}

impl_token!(Lamports, "SOL", decimals = 9);
