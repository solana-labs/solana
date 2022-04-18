/// Copied nightly-only experimental `checked_add_signed` implementation
/// from https://github.com/rust-lang/rust/pull/87601/files
/// Should be removed once the feature is added to standard library.
pub trait UintUtil {
    /// Checked addition with a signed integer. Computes `self + rhs`,
    /// returning `None` if overflow occurred.
    fn checked_add_signed(self, rhs: i64) -> Option<Self>
    where
        Self: Sized;

    /// Calculates `self` + `rhs` with a signed `rhs`
    ///
    /// Returns a tuple of the addition along with a boolean indicating
    /// whether an arithmetic overflow would occur. If an overflow would
    /// have occurred then the wrapped value is returned.
    fn overflowing_add_signed(self, rhs: i64) -> (Self, bool)
    where
        Self: Sized;
}

impl UintUtil for u64 {
    #[inline]
    fn checked_add_signed(self, rhs: i64) -> Option<Self> {
        let (a, b) = UintUtil::overflowing_add_signed(self, rhs);
        if b {
            None
        } else {
            Some(a)
        }
    }

    #[inline]
    fn overflowing_add_signed(self, rhs: i64) -> (Self, bool) {
        let (res, overflowed) = self.overflowing_add(rhs as Self);
        (res, overflowed ^ (rhs < 0))
    }
}

/// Copied nightly-only experimental `checked_add_unsigned` implementation
/// from https://github.com/rust-lang/rust/pull/87601/files
/// Should be removed once the feature is added to standard library.
pub trait IntUtil {
    /// Checked addition with an unsigned integer. Computes `self + rhs`,
    /// returning `None` if overflow occurred.
    fn checked_add_unsigned(self, rhs: u64) -> Option<Self>
    where
        Self: Sized;

    /// Checked subtraction with an unsigned integer. Computes `self - rhs`,
    /// returning `None` if overflow occurred.
    fn checked_sub_unsigned(self, rhs: u64) -> Option<Self>
    where
        Self: Sized;

    /// Calculates `self` + `rhs` with an unsigned `rhs`
    ///
    /// Returns a tuple of the addition along with a boolean indicating
    /// whether an arithmetic overflow would occur. If an overflow would
    /// have occurred then the wrapped value is returned.
    fn overflowing_add_unsigned(self, rhs: u64) -> (Self, bool)
    where
        Self: Sized;

    /// Calculates `self` - `rhs` with an unsigned `rhs`
    ///
    /// Returns a tuple of the subtraction along with a boolean indicating
    /// whether an arithmetic overflow would occur. If an overflow would
    /// have occurred then the wrapped value is returned.
    fn overflowing_sub_unsigned(self, rhs: u64) -> (Self, bool)
    where
        Self: Sized;
}

impl IntUtil for i64 {
    #[inline]
    fn checked_add_unsigned(self, rhs: u64) -> Option<Self> {
        let (a, b) = IntUtil::overflowing_add_unsigned(self, rhs);
        if b {
            None
        } else {
            Some(a)
        }
    }

    #[inline]
    fn checked_sub_unsigned(self, rhs: u64) -> Option<Self> {
        let (a, b) = IntUtil::overflowing_sub_unsigned(self, rhs);
        if b {
            None
        } else {
            Some(a)
        }
    }

    #[inline]
    fn overflowing_add_unsigned(self, rhs: u64) -> (Self, bool) {
        let rhs = rhs as Self;
        let (res, overflowed) = self.overflowing_add(rhs);
        (res, overflowed ^ (rhs < 0))
    }

    #[inline]
    fn overflowing_sub_unsigned(self, rhs: u64) -> (Self, bool) {
        let rhs = rhs as Self;
        let (res, overflowed) = self.overflowing_sub(rhs);
        (res, overflowed ^ (rhs < 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_checked_add_signed() {
        assert_eq!(UintUtil::checked_add_signed(1u64, 2), Some(3));
        assert_eq!(UintUtil::checked_add_signed(3u64, -2), Some(1));
        assert_eq!(UintUtil::checked_add_signed(1u64, -2), None);
        assert_eq!(UintUtil::checked_add_signed(u64::MAX - 2, 3), None);
    }

    #[test]
    fn test_overflowing_add_signed() {
        assert_eq!(UintUtil::overflowing_add_signed(1u64, 2), (3, false));
        assert_eq!(UintUtil::overflowing_add_signed(1u64, -2), (u64::MAX, true));
        assert_eq!(UintUtil::overflowing_add_signed(u64::MAX - 2, 4), (1, true));
    }

    #[test]
    fn test_checked_add_unsigned() {
        assert_eq!(IntUtil::checked_add_unsigned(1i64, 2), Some(3));
        assert_eq!(IntUtil::checked_add_unsigned(-3i64, 2), Some(-1));
        assert_eq!(IntUtil::checked_add_unsigned(1i64, 0), Some(1));
        assert_eq!(IntUtil::checked_add_unsigned(i64::MAX - 2, 3), None);
        assert_eq!(IntUtil::checked_sub_unsigned(0i64, u64::MAX), None);
    }

    #[test]
    fn test_overflowing_add_unsigned() {
        assert_eq!(IntUtil::overflowing_add_unsigned(1i64, 2), (3, false));
        assert_eq!(IntUtil::overflowing_add_unsigned(-1i64, 2), (1, false));
        assert_eq!(
            IntUtil::overflowing_add_unsigned(i64::MIN, u64::MAX),
            (i64::MAX, false)
        );
        assert_eq!(
            IntUtil::overflowing_add_unsigned(i64::MAX - 2, 3),
            (i64::MIN, true)
        );
    }

    #[test]
    fn test_checked_sub_unsigned() {
        assert_eq!(IntUtil::checked_sub_unsigned(1i64, 2), Some(-1));
        assert_eq!(IntUtil::checked_sub_unsigned(1i64, 0), Some(1));
        assert_eq!(IntUtil::checked_sub_unsigned(i64::MIN + 2, 3), None);
        assert_eq!(IntUtil::checked_sub_unsigned(0i64, u64::MAX), None);
    }

    #[test]
    fn test_overflowing_sub_unsigned() {
        assert_eq!(IntUtil::overflowing_sub_unsigned(1i64, 2), (-1, false));
        assert_eq!(
            IntUtil::overflowing_sub_unsigned(i64::MAX, u64::MAX),
            (i64::MIN, false)
        );
        assert_eq!(
            IntUtil::overflowing_sub_unsigned(i64::MIN + 2, 3),
            (i64::MAX, true)
        );
    }
}
