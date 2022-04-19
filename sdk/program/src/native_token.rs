#![allow(clippy::integer_arithmetic)]
/// There are 10^9 lamports in one SAND
pub const LAMPORTS_PER_SAND: u64 = 1_000_000_000;

/// Approximately convert fractional native tokens (lamports) into native tokens (SAND)
pub fn lamports_to_sand(lamports: u64) -> f64 {
    lamports as f64 / LAMPORTS_PER_SAND as f64
}

/// Approximately convert native tokens (SAND) into fractional native tokens (lamports)
pub fn sand_to_lamports(sand: f64) -> u64 {
    (sand * LAMPORTS_PER_SAND as f64) as u64
}

use std::fmt::{Debug, Display, Formatter, Result};
pub struct Sand(pub u64);

impl Sand {
    fn write_in_sand(&self, f: &mut Formatter) -> Result {
        write!(
            f,
            "◎{}.{:09}",
            self.0 / LAMPORTS_PER_SAND,
            self.0 % LAMPORTS_PER_SAND
        )
    }
}

impl Display for Sand {
    fn fmt(&self, f: &mut Formatter) -> Result {
        self.write_in_sand(f)
    }
}

impl Debug for Sand {
    fn fmt(&self, f: &mut Formatter) -> Result {
        self.write_in_sand(f)
    }
}
