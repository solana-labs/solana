/// There are 10^9 lamports in one SOL
pub const SOL_LAMPORTS: u64 = 1_000_000_000;

/// Approximately convert fractional native tokens (lamports) into native tokens (SOL)
pub fn lamports_to_sol(lamports: u64) -> f64 {
    lamports as f64 / SOL_LAMPORTS as f64
}

/// Approximately convert native tokens (SOL) into fractional native tokens (lamports)
pub fn sol_to_lamports(sol: f64) -> u64 {
    (sol * SOL_LAMPORTS as f64) as u64
}
