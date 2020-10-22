/// A helper for calculating a stake-weighted timestamp estimate from a set of timestamps and epoch
/// stake.
use solana_sdk::{
    account::Account,
    clock::{Slot, UnixTimestamp},
    pubkey::Pubkey,
};
use std::{collections::HashMap, time::Duration};

pub const TIMESTAMP_SLOT_RANGE: usize = 16;

pub fn calculate_stake_weighted_timestamp(
    unique_timestamps: &HashMap<Pubkey, (Slot, UnixTimestamp)>,
    stakes: &HashMap<Pubkey, (u64, Account)>,
    slot: Slot,
    slot_duration: Duration,
) -> Option<UnixTimestamp> {
    let (stake_weighted_timestamps_sum, total_stake) = unique_timestamps
        .iter()
        .filter_map(|(vote_pubkey, (timestamp_slot, timestamp))| {
            let offset = (slot - timestamp_slot) as u32 * slot_duration;
            stakes.get(&vote_pubkey).map(|(stake, _account)| {
                (
                    (*timestamp as u128 + offset.as_secs() as u128) * *stake as u128,
                    stake,
                )
            })
        })
        .fold((0, 0), |(timestamps, stakes), (timestamp, stake)| {
            (timestamps + timestamp, stakes + *stake as u128)
        });
    if total_stake > 0 {
        Some((stake_weighted_timestamps_sum / total_stake) as i64)
    } else {
        None
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use solana_sdk::native_token::sol_to_lamports;

    #[test]
    fn test_calculate_stake_weighted_timestamp() {
        let recent_timestamp: UnixTimestamp = 1_578_909_061;
        let slot = 5;
        let slot_duration = Duration::from_millis(400);
        let expected_offset = (slot * slot_duration).as_secs();
        let pubkey0 = solana_sdk::pubkey::new_rand();
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();
        let pubkey3 = solana_sdk::pubkey::new_rand();
        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (pubkey0, (0, recent_timestamp)),
            (pubkey1, (0, recent_timestamp)),
            (pubkey2, (0, recent_timestamp)),
            (pubkey3, (0, recent_timestamp)),
        ]
        .iter()
        .cloned()
        .collect();

        let stakes: HashMap<Pubkey, (u64, Account)> = [
            (
                pubkey0,
                (
                    sol_to_lamports(4_500_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey1,
                (
                    sol_to_lamports(4_500_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey2,
                (
                    sol_to_lamports(4_500_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey3,
                (
                    sol_to_lamports(4_500_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
        ]
        .iter()
        .cloned()
        .collect();
        assert_eq!(
            calculate_stake_weighted_timestamp(
                &unique_timestamps,
                &stakes,
                slot as Slot,
                slot_duration
            ),
            Some(recent_timestamp + expected_offset as i64)
        );

        let stakes: HashMap<Pubkey, (u64, Account)> = [
            (
                pubkey0,
                (
                    sol_to_lamports(15_000_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey1,
                (
                    sol_to_lamports(1_000_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey2,
                (
                    sol_to_lamports(1_000_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey3,
                (
                    sol_to_lamports(1_000_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
        ]
        .iter()
        .cloned()
        .collect();
        assert_eq!(
            calculate_stake_weighted_timestamp(
                &unique_timestamps,
                &stakes,
                slot as Slot,
                slot_duration
            ),
            Some(recent_timestamp + expected_offset as i64)
        );
    }
}
