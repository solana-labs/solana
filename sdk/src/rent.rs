//! configuration for network rent

#[derive(Serialize, Deserialize, PartialEq, Clone, Debug)]
pub struct Rent {
    /// Rental rate
    pub lamports_per_byte_year: u64,

    /// exemption threshold, in years
    pub exemption_threshold: f64,
}

/// default rental rate in lamports/byte-year, based on:
///  2^^34 lamports per Sol
///  $1 per Sol
///  $0.01 per megabyte day
///  $3.65 per megabyte year
pub const DEFAULT_LAMPORTS_PER_BYTE_YEAR: u64 = 17_179_869_184 / 100 * 365 / (1024 * 1024);

/// default amount of time (in years) the balance has to include rent for
pub const DEFAULT_EXEMPTION_THRESHOLD: f64 = 2.0;

impl Default for Rent {
    fn default() -> Self {
        Self {
            lamports_per_byte_year: DEFAULT_LAMPORTS_PER_BYTE_YEAR,
            exemption_threshold: DEFAULT_EXEMPTION_THRESHOLD,
        }
    }
}

impl Rent {
    /// minimum balance due for a given size Account::data.len()
    pub fn minimum_balance(&self, data_len: usize) -> u64 {
        let bytes = data_len as u64;
        bytes * (self.exemption_threshold * self.lamports_per_byte_year as f64) as u64
    }

    /// whether a given balance and data_len would be exempt
    pub fn is_exempt(&self, balance: u64, data_len: usize) -> bool {
        balance >= self.minimum_balance(data_len)
    }

    /// rent due on account's data_len with balance
    pub fn due(&self, balance: u64, data_len: usize, years_elapsed: f64) -> u64 {
        if self.is_exempt(balance, data_len) {
            0
        } else {
            let bytes = data_len as u64;
            ((self.lamports_per_byte_year * bytes) as f64 * years_elapsed) as u64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_due() {
        let rent = Rent::default();

        assert_eq!(rent.due(0, 1, 1.0), DEFAULT_LAMPORTS_PER_BYTE_YEAR);
        assert_eq!(
            rent.due(
                DEFAULT_LAMPORTS_PER_BYTE_YEAR * DEFAULT_EXEMPTION_THRESHOLD as u64,
                1,
                1.0
            ),
            0
        );
    }
}
