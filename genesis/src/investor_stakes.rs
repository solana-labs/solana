//! investor stakes generator
use crate::lockups::Lockups;

// create stake accounts for lamports with at most stake_granularity in each
//  account
pub(crate) fn investor_stakes(
    authorized_staker: &Pubkey,
    authorized_withdrawer: &Pubkey,
    total_lamports: u64,
    stake_granularity: u64,
    lockups: Lockups,
    custodian: &Pubkey,
) -> impl Iterator<Item = Account> {
    lockups.map(|increment| {
        let increment_lamports = increment.lamports(total_lamports);
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPOCHS_PER_MONTH: Epoch = 2;

    #[test]
    fn test_make_lockups() {
        // this number made up, to induce rounding errors
        let tokens: u64 = 1725987234408924;

        assert_eq!(
            Lockups::new(0.20, 6 * EPOCHS_PER_MONTH, 24, EPOCHS_PER_MONTH)
                .map(|increment| {
                    dbg!(&increment);
                    (increment.fraction * tokens as f64) as u64
                        - (increment.prev_fraction * tokens as f64) as u64
                })
                .sum::<u64>(),
            tokens
        );
    }
}
