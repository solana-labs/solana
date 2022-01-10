//! The accounts data space has a maximum size it is permitted to grow to.  This module contains
//! the constants and types for tracking and metering the accounts data space during program
//! runtime.
use solana_sdk::instruction::InstructionError;

/// The maximum allowed size, in bytes, of the accounts data
/// 128 GB was chosen because it is the RAM amount listed under Hardware Recommendations on
/// [Validator Requirements](https://docs.solana.com/running-validator/validator-reqs), and
/// validators often put the ledger on a RAM disk (i.e. tmpfs).
pub const MAX_ACCOUNTS_DATA_LEN: u64 = 128_000_000_000;

/// Meter and track the amount of available accounts data space
#[derive(Debug, Default, Clone, Copy, Eq, PartialEq)]
pub struct AccountsDataMeter {
    /// The maximum amount of accounts data space that can be used (in bytes)
    maximum: u64,

    /// The current amount of accounts data space used (in bytes)
    current: u64,
}

impl AccountsDataMeter {
    /// Make a new AccountsDataMeter
    pub fn new(current_accounts_data_len: u64) -> Self {
        let accounts_data_meter = Self {
            maximum: MAX_ACCOUNTS_DATA_LEN,
            current: current_accounts_data_len,
        };
        debug_assert!(accounts_data_meter.current <= accounts_data_meter.maximum);
        accounts_data_meter
    }

    /// Return the maximum amount of accounts data space that can be used (in bytes)
    pub fn maximum(&self) -> u64 {
        self.maximum
    }

    /// Return the current amount of accounts data space used (in bytes)
    pub fn current(&self) -> u64 {
        self.current
    }

    /// Get the remaining amount of accounts data space (in bytes)
    pub fn remaining(&self) -> u64 {
        self.maximum.saturating_sub(self.current)
    }

    /// Consume accounts data space, in bytes.  If `amount` is positive, we are *increasing* the
    /// amount of accounts data space used.  If `amount` is negative, we are *decreasing* the
    /// amount of accounts data space used.
    pub fn consume(&mut self, amount: i64) -> Result<(), InstructionError> {
        if amount == 0 {
            // nothing to do here; lets us skip doing unnecessary work in the 'else' case
            return Ok(());
        }

        if amount.is_positive() {
            let amount = amount as u64;
            if amount > self.remaining() {
                return Err(InstructionError::AccountsDataBudgetExceeded);
            }
            self.current = self.current.saturating_add(amount);
        } else {
            let amount = amount.abs() as u64;
            self.current = self.current.saturating_sub(amount);
        }

        Ok(())
    }
}

#[cfg(test)]
impl AccountsDataMeter {
    pub fn set_maximum(&mut self, maximum: u64) {
        self.maximum = maximum;
    }
    pub fn set_current(&mut self, current: u64) {
        self.current = current;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let current = 1234;
        let accounts_data_meter = AccountsDataMeter::new(current);
        assert_eq!(accounts_data_meter.maximum, MAX_ACCOUNTS_DATA_LEN);
        assert_eq!(accounts_data_meter.current, current);
    }

    #[test]
    fn test_new_can_use_max_len() {
        let _ = AccountsDataMeter::new(MAX_ACCOUNTS_DATA_LEN);
    }

    #[test]
    #[should_panic]
    fn test_new_panics_if_current_len_too_big() {
        let _ = AccountsDataMeter::new(MAX_ACCOUNTS_DATA_LEN + 1);
    }

    #[test]
    fn test_remaining() {
        let current_accounts_data_len = 0;
        let accounts_data_meter = AccountsDataMeter::new(current_accounts_data_len);
        assert_eq!(accounts_data_meter.remaining(), MAX_ACCOUNTS_DATA_LEN);
    }

    #[test]
    fn test_remaining_saturates() {
        let current_accounts_data_len = 0;
        let mut accounts_data_meter = AccountsDataMeter::new(current_accounts_data_len);
        // To test that remaining() saturates, need to break the invariant that current <= maximum
        accounts_data_meter.current = MAX_ACCOUNTS_DATA_LEN + 1;
        assert_eq!(accounts_data_meter.remaining(), 0);
    }

    #[test]
    fn test_consume() {
        let current_accounts_data_len = 0;
        let mut accounts_data_meter = AccountsDataMeter::new(current_accounts_data_len);

        // Test: simple, positive numbers
        let result = accounts_data_meter.consume(0);
        assert!(result.is_ok());
        let result = accounts_data_meter.consume(1);
        assert!(result.is_ok());
        let result = accounts_data_meter.consume(4);
        assert!(result.is_ok());
        let result = accounts_data_meter.consume(9);
        assert!(result.is_ok());

        // Test: can consume the remaining amount
        let remaining = accounts_data_meter.remaining() as i64;
        let result = accounts_data_meter.consume(remaining);
        assert!(result.is_ok());
        assert_eq!(accounts_data_meter.remaining(), 0);
    }

    #[test]
    fn test_consume_deallocate() {
        let current_accounts_data_len = 10_000;
        let mut accounts_data_meter = AccountsDataMeter::new(current_accounts_data_len);
        let remaining_before = accounts_data_meter.remaining();

        let amount = (current_accounts_data_len / 2) as i64;
        let amount = -amount;
        let result = accounts_data_meter.consume(amount);
        assert!(result.is_ok());
        let remaining_after = accounts_data_meter.remaining();
        assert_eq!(remaining_after, remaining_before + amount.abs() as u64);
    }

    #[test]
    fn test_consume_too_much() {
        let current_accounts_data_len = 0;
        let mut accounts_data_meter = AccountsDataMeter::new(current_accounts_data_len);

        // Test: consuming more than what's available (1) returns an error, (2) does not consume
        let remaining = accounts_data_meter.remaining();
        let result = accounts_data_meter.consume(remaining as i64 + 1);
        assert!(result.is_err());
        assert_eq!(accounts_data_meter.remaining(), remaining);
    }

    #[test]
    fn test_consume_zero() {
        // Pre-condition: set up the accounts data meter such that there is no remaining space
        let current_accounts_data_len = 1234;
        let mut accounts_data_meter = AccountsDataMeter::new(current_accounts_data_len);
        accounts_data_meter.maximum = current_accounts_data_len;
        assert_eq!(accounts_data_meter.remaining(), 0);

        // Test: can always consume zero, even if there is no remaining space
        let result = accounts_data_meter.consume(0);
        assert!(result.is_ok());
    }
}
