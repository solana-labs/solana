//! Configuration for network [rent].
//!
//! [rent]: https://docs.solana.com/implemented-proposals/rent

#![allow(clippy::integer_arithmetic)]

use {crate::clock::DEFAULT_SLOTS_PER_EPOCH, solana_sdk_macro::CloneZeroed};

/// Configuration of network rent.
#[repr(C)]
#[derive(Serialize, Deserialize, PartialEq, CloneZeroed, Copy, Debug, AbiExample)]
pub struct Rent {
    /// Rental rate in lamports/byte-year.
    pub lamports_per_byte_year: u64,

    /// Amount of time (in years) a balance must include rent for the account to
    /// be rent exempt.
    pub exemption_threshold: f64,

    /// The percentage of collected rent that is burned.
    ///
    /// Valid values are in the range [0, 100]. The remaining percentage is
    /// distributed to validators.
    pub burn_percent: u8,
}

/// Default rental rate in lamports/byte-year.
///
/// This calculation is based on:
/// - 10^9 lamports per SOL
/// - $1 per SOL
/// - $0.01 per megabyte day
/// - $3.65 per megabyte year
pub const DEFAULT_LAMPORTS_PER_BYTE_YEAR: u64 = 1_000_000_000 / 100 * 365 / (1024 * 1024);

/// Default amount of time (in years) the balance has to include rent for the
/// account to be rent exempt.
pub const DEFAULT_EXEMPTION_THRESHOLD: f64 = 2.0;

/// Default percentage of collected rent that is burned.
///
/// Valid values are in the range [0, 100]. The remaining percentage is
/// distributed to validators.
pub const DEFAULT_BURN_PERCENT: u8 = 50;

/// Account storage overhead for calculation of base rent.
///
/// This is the number of bytes required to store an account with no data. It is
/// added to an accounts data length when calculating [`Rent::minimum_balance`].
pub const ACCOUNT_STORAGE_OVERHEAD: u64 = 128;

impl Default for Rent {
    fn default() -> Self {
        Self {
            lamports_per_byte_year: DEFAULT_LAMPORTS_PER_BYTE_YEAR,
            exemption_threshold: DEFAULT_EXEMPTION_THRESHOLD,
            burn_percent: DEFAULT_BURN_PERCENT,
        }
    }
}

impl Rent {
    /// Calculate how much rent to burn from the collected rent.
    ///
    /// The first value returned is the amount burned. The second is the amount
    /// to distribute to validators.
    pub fn calculate_burn(&self, rent_collected: u64) -> (u64, u64) {
        let burned_portion = (rent_collected * u64::from(self.burn_percent)) / 100;
        (burned_portion, rent_collected - burned_portion)
    }

    /// Minimum balance due for rent-exemption of a given account data size.
    ///
    /// Note: a stripped-down version of this calculation is used in
    /// `calculate_split_rent_exempt_reserve` in the stake program. When this
    /// function is updated, eg. when making rent variable, the stake program
    /// will need to be refactored.
    pub fn minimum_balance(&self, data_len: usize) -> u64 {
        let bytes = data_len as u64;
        (((ACCOUNT_STORAGE_OVERHEAD + bytes) * self.lamports_per_byte_year) as f64
            * self.exemption_threshold) as u64
    }

    /// Whether a given balance and data length would be exempt.
    pub fn is_exempt(&self, balance: u64, data_len: usize) -> bool {
        balance >= self.minimum_balance(data_len)
    }

    /// Rent due on account's data length with balance.
    pub fn due(&self, balance: u64, data_len: usize, years_elapsed: f64) -> RentDue {
        if self.is_exempt(balance, data_len) {
            RentDue::Exempt
        } else {
            RentDue::Paying(self.due_amount(data_len, years_elapsed))
        }
    }

    /// Rent due for account that is known to be not exempt.
    pub fn due_amount(&self, data_len: usize, years_elapsed: f64) -> u64 {
        let actual_data_len = data_len as u64 + ACCOUNT_STORAGE_OVERHEAD;
        let lamports_per_year = self.lamports_per_byte_year * actual_data_len;
        (lamports_per_year as f64 * years_elapsed) as u64
    }

    /// Creates a `Rent` that charges no lamports.
    ///
    /// This is used for testing.
    pub fn free() -> Self {
        Self {
            lamports_per_byte_year: 0,
            ..Rent::default()
        }
    }

    /// Creates a `Rent` that is scaled based on the number of slots in an epoch.
    ///
    /// This is used for testing.
    pub fn with_slots_per_epoch(slots_per_epoch: u64) -> Self {
        let ratio = slots_per_epoch as f64 / DEFAULT_SLOTS_PER_EPOCH as f64;
        let exemption_threshold = DEFAULT_EXEMPTION_THRESHOLD * ratio;
        let lamports_per_byte_year = (DEFAULT_LAMPORTS_PER_BYTE_YEAR as f64 / ratio) as u64;
        Self {
            lamports_per_byte_year,
            exemption_threshold,
            ..Self::default()
        }
    }
}

/// The return value of [`Rent::due`].
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum RentDue {
    /// Used to indicate the account is rent exempt.
    Exempt,
    /// The account owes this much rent.
    Paying(u64),
}

impl RentDue {
    /// Return the lamports due for rent.
    pub fn lamports(&self) -> u64 {
        match self {
            RentDue::Exempt => 0,
            RentDue::Paying(x) => *x,
        }
    }

    /// Return 'true' if rent exempt.
    pub fn is_exempt(&self) -> bool {
        match self {
            RentDue::Exempt => true,
            RentDue::Paying(_) => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_due() {
        let default_rent = Rent::default();

        assert_eq!(
            default_rent.due(0, 2, 1.2),
            RentDue::Paying(
                (((2 + ACCOUNT_STORAGE_OVERHEAD) * DEFAULT_LAMPORTS_PER_BYTE_YEAR) as f64 * 1.2)
                    as u64
            ),
        );
        assert_eq!(
            default_rent.due(
                (((2 + ACCOUNT_STORAGE_OVERHEAD) * DEFAULT_LAMPORTS_PER_BYTE_YEAR) as f64
                    * DEFAULT_EXEMPTION_THRESHOLD) as u64,
                2,
                1.2
            ),
            RentDue::Exempt,
        );

        let custom_rent = Rent {
            lamports_per_byte_year: 5,
            exemption_threshold: 2.5,
            ..Rent::default()
        };

        assert_eq!(
            custom_rent.due(0, 2, 1.2),
            RentDue::Paying(
                (((2 + ACCOUNT_STORAGE_OVERHEAD) * custom_rent.lamports_per_byte_year) as f64 * 1.2)
                    as u64,
            )
        );

        assert_eq!(
            custom_rent.due(
                (((2 + ACCOUNT_STORAGE_OVERHEAD) * custom_rent.lamports_per_byte_year) as f64
                    * custom_rent.exemption_threshold) as u64,
                2,
                1.2
            ),
            RentDue::Exempt
        );
    }

    #[test]
    fn test_rent_due_lamports() {
        assert_eq!(RentDue::Exempt.lamports(), 0);

        let amount = 123;
        assert_eq!(RentDue::Paying(amount).lamports(), amount);
    }

    #[test]
    fn test_rent_due_is_exempt() {
        assert!(RentDue::Exempt.is_exempt());
        assert!(!RentDue::Paying(0).is_exempt());
    }

    #[test]
    fn test_clone() {
        let rent = Rent {
            lamports_per_byte_year: 1,
            exemption_threshold: 2.2,
            burn_percent: 3,
        };
        #[allow(clippy::clone_on_copy)]
        let cloned_rent = rent.clone();
        assert_eq!(cloned_rent, rent);
    }
}
