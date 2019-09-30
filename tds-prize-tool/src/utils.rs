pub(crate) fn sol_to_lamports(sol: f64) -> u64 {
    (sol * 2u64.pow(34) as f64) as u64
}
