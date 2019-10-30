/// There are 2^34 lamports in one sol
pub const SOL_LAMPORTS: u64 = 1_000_000_000;

/// Approximately convert fractional native tokens (lamports) into native tokens (sol)
pub fn lamports_to_sol(lamports: u64) -> f64 {
    lamports as f64 / SOL_LAMPORTS as f64
}

/// Approximately convert native tokens (sol) into fractional native tokens (lamports)
pub fn sol_to_lamports(sol: f64) -> u64 {
    (sol * SOL_LAMPORTS as f64) as u64
}
