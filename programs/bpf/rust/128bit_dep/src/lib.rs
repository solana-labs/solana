//! @brief Solana Rust-based BPF program utility functions and types

extern crate solana_sdk;

pub fn uadd(x: u128, y: u128) -> u128 {
    x + y
}
pub fn usubtract(x: u128, y: u128) -> u128 {
    x - y
}
pub fn umultiply(x: u128, y: u128) -> u128 {
    x * y
}
pub fn udivide(n: u128, d: u128) -> u128 {
    n / d
}
pub fn umodulo(n: u128, d: u128) -> u128 {
    n % d
}

pub fn add(x: i128, y: i128) -> i128 {
    x + y
}
pub fn subtract(x: i128, y: i128) -> i128 {
    x - y
}
pub fn multiply(x: i128, y: i128) -> i128 {
    x * y
}
pub fn divide(n: i128, d: i128) -> i128 {
    n / d
}
pub fn modulo(n: i128, d: i128) -> i128 {
    n % d
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn pull_in_externs() {
        // Rust on Linux excludes the externs unless there is a
        // direct dependency, use this test to force the pull in of the library.
        // This is not necessary on macos and unfortunate on Linux
        // Issue #4972
        use solana_sdk::program_test::*;
        unsafe { sol_log_("X".as_ptr(), 1) };
        sol_log_64_(1, 2, 3, 4, 5);
    }

    #[test]
    fn test_add() {
        assert_eq!(3, add(1, 2));
    }
}
