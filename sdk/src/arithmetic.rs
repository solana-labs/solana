use std::time::Duration;

/// A helper trait for primitive types that do not yet implement saturating arithmetic methods
pub trait SaturatingArithmetic<T> {
    fn sol_saturating_add(&self, rhs: Self) -> Self;
    fn sol_saturating_sub(&self, rhs: Self) -> Self;
    fn sol_saturating_mul(&self, rhs: T) -> Self;
}

/// Saturating arithmetic for Duration, until Rust support moves from nightly to stable
/// Duration::MAX is constructed manually, as Duration consts are not yet stable either.
impl SaturatingArithmetic<u32> for Duration {
    fn sol_saturating_add(&self, rhs: Self) -> Self {
        self.checked_add(rhs)
            .unwrap_or_else(|| Self::new(u64::MAX, 1_000_000_000u32.saturating_sub(1)))
    }
    fn sol_saturating_sub(&self, rhs: Self) -> Self {
        self.checked_sub(rhs).unwrap_or_else(|| Self::new(0, 0))
    }
    fn sol_saturating_mul(&self, rhs: u32) -> Self {
        self.checked_mul(rhs)
            .unwrap_or_else(|| Self::new(u64::MAX, 1_000_000_000u32.saturating_sub(1)))
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_duration() {
        let empty_duration = Duration::new(0, 0);
        let max_duration = Duration::new(u64::MAX, 1_000_000_000 - 1);
        let duration = Duration::new(u64::MAX, 0);

        let add = duration.sol_saturating_add(duration);
        assert_eq!(add, max_duration);

        let sub = duration.sol_saturating_sub(max_duration);
        assert_eq!(sub, empty_duration);

        let mult = duration.sol_saturating_mul(u32::MAX);
        assert_eq!(mult, max_duration);
    }
}
