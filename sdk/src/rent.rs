//! configuration for network rent

#[repr(C)]
#[derive(Serialize, Deserialize, PartialEq, Clone, Copy, Debug)]
pub struct Rent {
    /// Rental rate
    pub lamports_per_byte_year: u64,

    /// exemption threshold, in years
    pub exemption_threshold: f64,

    // What portion of collected rent are to be destroyed, percentage-wise
    pub burn_percent: u8,
}

/// default rental rate in lamports/byte-year, based on:
///  10^9 lamports per SOL
///  $1 per SOL
///  $0.01 per megabyte day
///  $3.65 per megabyte year
pub const DEFAULT_LAMPORTS_PER_BYTE_YEAR: u64 = 0; //1_000_000_000 / 100 * 365 / (1024 * 1024);

/// default amount of time (in years) the balance has to include rent for
pub const DEFAULT_EXEMPTION_THRESHOLD: f64 = 2.0;

/// default percentage of rent to burn (Valid values are 0 to 100)
pub const DEFAULT_BURN_PERCENT: u8 = 100;

/// account storage overhead for calculation of base rent
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
    /// minimum balance due for a given size Account::data.len()
    pub fn minimum_balance(&self, data_len: usize) -> u64 {
        let bytes = data_len as u64;
        (((ACCOUNT_STORAGE_OVERHEAD + bytes) * self.lamports_per_byte_year) as f64
            * self.exemption_threshold) as u64
    }

    /// whether a given balance and data_len would be exempt
    pub fn is_exempt(&self, balance: u64, data_len: usize) -> bool {
        balance >= self.minimum_balance(data_len)
    }

    /// rent due on account's data_len with balance
    pub fn due(&self, balance: u64, data_len: usize, years_elapsed: f64) -> (u64, bool) {
        if self.is_exempt(balance, data_len) {
            (0, true)
        } else {
            (
                ((self.lamports_per_byte_year * (data_len as u64 + ACCOUNT_STORAGE_OVERHEAD))
                    as f64
                    * years_elapsed) as u64,
                false,
            )
        }
    }

    pub fn free() -> Self {
        Self {
            lamports_per_byte_year: 0,
            ..Rent::default()
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
            (
                (((2 + ACCOUNT_STORAGE_OVERHEAD) * DEFAULT_LAMPORTS_PER_BYTE_YEAR) as f64 * 1.2)
                    as u64,
                DEFAULT_LAMPORTS_PER_BYTE_YEAR == 0
            )
        );
        assert_eq!(
            default_rent.due(
                (((2 + ACCOUNT_STORAGE_OVERHEAD) * DEFAULT_LAMPORTS_PER_BYTE_YEAR) as f64
                    * DEFAULT_EXEMPTION_THRESHOLD) as u64,
                2,
                1.2
            ),
            (0, true)
        );

        let mut custom_rent = Rent::default();
        custom_rent.lamports_per_byte_year = 5;
        custom_rent.exemption_threshold = 2.5;

        assert_eq!(
            custom_rent.due(0, 2, 1.2),
            (
                (((2 + ACCOUNT_STORAGE_OVERHEAD) * custom_rent.lamports_per_byte_year) as f64 * 1.2)
                    as u64,
                false
            )
        );

        assert_eq!(
            custom_rent.due(
                (((2 + ACCOUNT_STORAGE_OVERHEAD) * custom_rent.lamports_per_byte_year) as f64
                    * custom_rent.exemption_threshold) as u64,
                2,
                1.2
            ),
            (0, true)
        );
    }

    // uncomment me and make my eprintlns macros
    //    #[test]
    //    fn test_rent_model() {
    //        use crate::timing::*;
    //
    //        const SECONDS_PER_YEAR: f64 = (365.25 * 24.0 * 60.0 * 60.0);
    //        const SLOTS_PER_YEAR: f64 =
    //            SECONDS_PER_YEAR / (DEFAULT_TICKS_PER_SLOT as f64 / DEFAULT_TICKS_PER_SECOND as f64);
    //
    //        let rent = Rent::default();
    //
    //        eprintln();
    //        // lamports charged per byte per slot at $1/MByear, rent per slot is zero
    //        eprintln(
    //            "{} lamports per byte-slot, rent.due(): {}",
    //            (1.0 / SLOTS_PER_YEAR) * DEFAULT_LAMPORTS_PER_BYTE_YEAR as f64,
    //            rent.due(0, 1, 1.0 / SLOTS_PER_YEAR).0,
    //        );
    //        // lamports charged per byte per _epoch_ starts to have some significant digits
    //        eprintln(
    //            "{} lamports per byte-epoch, rent.due(): {}",
    //            (1.0 / SLOTS_PER_YEAR)
    //                * (DEFAULT_LAMPORTS_PER_BYTE_YEAR * DEFAULT_SLOTS_PER_EPOCH) as f64,
    //            rent.due(
    //                0,
    //                1,
    //                (1.0 / SLOTS_PER_YEAR) * DEFAULT_SLOTS_PER_EPOCH as f64
    //            )
    //            .0,
    //        );
    //        // have a look at what a large-ish sysvar would cost, were it a real account...
    //        eprintln(
    //            "stake_history: {}kB == {} lamports per epoch",
    //            crate::sysvar::stake_history::StakeHistory::size_of() / 1024,
    //            rent.due(
    //                0,
    //                crate::sysvar::stake_history::StakeHistory::size_of(),
    //                (1.0 / SLOTS_PER_YEAR) * DEFAULT_SLOTS_PER_EPOCH as f64
    //            )
    //            .0,
    //        );
    //    }
}
