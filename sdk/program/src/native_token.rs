/// There are 10^9 lamports in one SAFE
pub const LAMPORTS_PER_SAFE: u64 = 1_000_000_000;

/// Approximately convert fractional native tokens (lamports) into native tokens (SAFE)
pub fn lamports_to_safe (lamports: u64) -> f64 {
    lamports as f64 / LAMPORTS_PER_SAFE as f64
}

/// Approximately convert native tokens (SAFE) into fractional native tokens (lamports)
pub fn safe_to_lamports(sol: f64) -> u64 {
    (sol * LAMPORTS_PER_SAFE as f64) as u64
}

use std::fmt::{Debug, Display, Formatter, Result};
pub struct Safe(pub u64);

impl Safe {
    fn write_in_safe (&self, f: &mut Formatter) -> Result {
        write!(
            f,
            "◎{}.{:09}",
            self.0 / LAMPORTS_PER_SAFE,
            self.0 % LAMPORTS_PER_SAFE
        )
    }
}

impl Display for Safe {
    fn fmt(&self, f: &mut Formatter) -> Result {
        self.write_in_safe (f)
    }
}

impl Debug for Safe {
    fn fmt(&self, f: &mut Formatter) -> Result {
        self.write_in_safe (f)
    }
}
