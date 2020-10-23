/// A helper for calculating a stake-weighted timestamp estimate from a set of timestamps and epoch
/// stake.
use solana_sdk::{
    account::Account,
    clock::{Slot, UnixTimestamp},
    pubkey::Pubkey,
};
use std::{
    collections::{BTreeMap, HashMap},
    time::Duration,
};

pub const TIMESTAMP_SLOT_RANGE: usize = 16;
const MAX_ALLOWABLE_DRIFT_PERCENTAGE: u32 = 25;

pub enum EstimateType {
    Bounded,
    Unbounded, // Deprecated.  Remove in the Solana v1.6.0 timeframe
}

pub fn calculate_stake_weighted_timestamp(
    unique_timestamps: &HashMap<Pubkey, (Slot, UnixTimestamp)>,
    stakes: &HashMap<Pubkey, (u64, Account)>,
    slot: Slot,
    slot_duration: Duration,
    estimate_type: EstimateType,
    epoch_start_timestamp: Option<(Slot, UnixTimestamp)>,
) -> Option<UnixTimestamp> {
    match estimate_type {
        EstimateType::Bounded => calculate_bounded_stake_weighted_timestamp(
            unique_timestamps,
            stakes,
            slot,
            slot_duration,
            epoch_start_timestamp,
        ),
        EstimateType::Unbounded => calculate_unbounded_stake_weighted_timestamp(
            unique_timestamps,
            stakes,
            slot,
            slot_duration,
        ),
    }
}

fn calculate_unbounded_stake_weighted_timestamp(
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

fn calculate_bounded_stake_weighted_timestamp(
    unique_timestamps: &HashMap<Pubkey, (Slot, UnixTimestamp)>,
    stakes: &HashMap<Pubkey, (u64, Account)>,
    slot: Slot,
    slot_duration: Duration,
    epoch_start_timestamp: Option<(Slot, UnixTimestamp)>,
) -> Option<UnixTimestamp> {
    let mut stake_per_timestamp: BTreeMap<UnixTimestamp, u128> = BTreeMap::new();
    let mut total_stake = 0;
    for (vote_pubkey, (timestamp_slot, timestamp)) in unique_timestamps.iter() {
        let offset = slot.saturating_sub(*timestamp_slot) as u32 * slot_duration;
        let estimate = timestamp + offset.as_secs() as i64;
        let stake = stakes
            .get(&vote_pubkey)
            .map(|(stake, _account)| stake)
            .unwrap_or(&0);
        stake_per_timestamp
            .entry(estimate)
            .and_modify(|stake_sum| *stake_sum += *stake as u128)
            .or_insert(*stake as u128);
        total_stake += *stake as u128;
    }
    if total_stake == 0 {
        return None;
    }
    let mut stake_accumulator = 0;
    let mut estimate = 0;
    // Populate `estimate` with stake-weighted median timestamp
    for (timestamp, stake) in stake_per_timestamp.into_iter() {
        stake_accumulator += stake;
        if stake_accumulator > total_stake / 2 {
            estimate = timestamp;
            break;
        }
    }
    // Bound estimate by `MAX_ALLOWABLE_DRIFT_PERCENTAGE` since the start of the epoch
    if let Some((epoch_start_slot, epoch_start_timestamp)) = epoch_start_timestamp {
        let poh_estimate_offset = slot.saturating_sub(epoch_start_slot) as u32 * slot_duration;
        let estimate_offset =
            Duration::from_secs(estimate.saturating_sub(epoch_start_timestamp) as u64);
        let delta = if estimate_offset > poh_estimate_offset {
            estimate_offset - poh_estimate_offset
        } else {
            poh_estimate_offset - estimate_offset
        };
        if delta > poh_estimate_offset * MAX_ALLOWABLE_DRIFT_PERCENTAGE / 100 {
            estimate = epoch_start_timestamp + poh_estimate_offset.as_secs() as i64;
        }
    }
    Some(estimate)
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
            calculate_unbounded_stake_weighted_timestamp(
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
            calculate_unbounded_stake_weighted_timestamp(
                &unique_timestamps,
                &stakes,
                slot as Slot,
                slot_duration
            ),
            Some(recent_timestamp + expected_offset as i64)
        );
    }

    #[test]
    fn test_calculate_bounded_stake_weighted_timestamp_uses_median() {
        let recent_timestamp: UnixTimestamp = 1_578_909_061;
        let slot = 5;
        let slot_duration = Duration::from_millis(400);
        let pubkey0 = solana_sdk::pubkey::new_rand();
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();
        let pubkey3 = solana_sdk::pubkey::new_rand();
        let pubkey4 = solana_sdk::pubkey::new_rand();

        // Test low-staked outlier(s)
        let stakes: HashMap<Pubkey, (u64, Account)> = [
            (
                pubkey0,
                (sol_to_lamports(1.0), Account::new(1, 0, &Pubkey::default())),
            ),
            (
                pubkey1,
                (sol_to_lamports(1.0), Account::new(1, 0, &Pubkey::default())),
            ),
            (
                pubkey2,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey3,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey4,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
        ]
        .iter()
        .cloned()
        .collect();

        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (pubkey0, (5, 0)),
            (pubkey1, (5, recent_timestamp)),
            (pubkey2, (5, recent_timestamp)),
            (pubkey3, (5, recent_timestamp)),
            (pubkey4, (5, recent_timestamp)),
        ]
        .iter()
        .cloned()
        .collect();

        let unbounded = calculate_unbounded_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
        )
        .unwrap();

        let bounded = calculate_bounded_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            None,
        )
        .unwrap();
        assert_eq!(bounded - unbounded, 527); // timestamp w/ 0.00003% of the stake can shift the timestamp backward 8min
        assert_eq!(bounded, recent_timestamp); // low-staked outlier cannot affect bounded timestamp

        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (pubkey0, (5, recent_timestamp)),
            (pubkey1, (5, i64::MAX)),
            (pubkey2, (5, recent_timestamp)),
            (pubkey3, (5, recent_timestamp)),
            (pubkey4, (5, recent_timestamp)),
        ]
        .iter()
        .cloned()
        .collect();

        let unbounded = calculate_unbounded_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
        )
        .unwrap();

        let bounded = calculate_bounded_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            None,
        )
        .unwrap();
        assert_eq!(unbounded - bounded, 3074455295455); // timestamp w/ 0.00003% of the stake can shift the timestamp forward 97k years!
        assert_eq!(bounded, recent_timestamp); // low-staked outlier cannot affect bounded timestamp

        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (pubkey0, (5, 0)),
            (pubkey1, (5, i64::MAX)),
            (pubkey2, (5, recent_timestamp)),
            (pubkey3, (5, recent_timestamp)),
            (pubkey4, (5, recent_timestamp)),
        ]
        .iter()
        .cloned()
        .collect();

        let bounded = calculate_bounded_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            None,
        )
        .unwrap();
        assert_eq!(bounded, recent_timestamp); // multiple low-staked outliers cannot affect bounded timestamp if they don't shift the median

        // Test higher-staked outlier(s)
        let stakes: HashMap<Pubkey, (u64, Account)> = [
            (
                pubkey0,
                (
                    sol_to_lamports(1_000_000.0), // 1/3 stake
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey1,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey2,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
        ]
        .iter()
        .cloned()
        .collect();

        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (pubkey0, (5, 0)),
            (pubkey1, (5, i64::MAX)),
            (pubkey2, (5, recent_timestamp)),
        ]
        .iter()
        .cloned()
        .collect();

        let bounded = calculate_bounded_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            None,
        )
        .unwrap();
        assert_eq!(bounded, recent_timestamp); // outlier(s) cannot affect bounded timestamp if they don't shift the median

        let stakes: HashMap<Pubkey, (u64, Account)> = [
            (
                pubkey0,
                (
                    sol_to_lamports(1_000_001.0), // 1/3 stake
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey1,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
        ]
        .iter()
        .cloned()
        .collect();

        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> =
            [(pubkey0, (5, 0)), (pubkey1, (5, recent_timestamp))]
                .iter()
                .cloned()
                .collect();

        let bounded = calculate_bounded_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            None,
        )
        .unwrap();
        assert_eq!(recent_timestamp - bounded, 1578909061); // outliers > 1/2 of available stake can affect timestamp
    }

    #[test]
    fn test_calculate_bounded_stake_weighted_timestamp_poh() {
        let epoch_start_timestamp: UnixTimestamp = 1_578_909_061;
        let slot = 20;
        let slot_duration = Duration::from_millis(400);
        let poh_offset = (slot * slot_duration).as_secs();
        let acceptable_delta = (MAX_ALLOWABLE_DRIFT_PERCENTAGE * poh_offset as u32 / 100) as i64;
        let poh_estimate = epoch_start_timestamp + poh_offset as i64;
        let pubkey0 = solana_sdk::pubkey::new_rand();
        let pubkey1 = solana_sdk::pubkey::new_rand();
        let pubkey2 = solana_sdk::pubkey::new_rand();

        let stakes: HashMap<Pubkey, (u64, Account)> = [
            (
                pubkey0,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey1,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
            (
                pubkey2,
                (
                    sol_to_lamports(1_000_000.0),
                    Account::new(1, 0, &Pubkey::default()),
                ),
            ),
        ]
        .iter()
        .cloned()
        .collect();

        // Test when stake-weighted median is too high
        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (pubkey0, (slot as u64, poh_estimate + acceptable_delta + 1)),
            (pubkey1, (slot as u64, poh_estimate + acceptable_delta + 1)),
            (pubkey2, (slot as u64, poh_estimate + acceptable_delta + 1)),
        ]
        .iter()
        .cloned()
        .collect();

        let bounded = calculate_bounded_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            Some((0, epoch_start_timestamp)),
        )
        .unwrap();
        assert_eq!(bounded, poh_estimate);

        // Test when stake-weighted median is too low
        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (pubkey0, (slot as u64, poh_estimate - acceptable_delta - 1)),
            (pubkey1, (slot as u64, poh_estimate - acceptable_delta - 1)),
            (pubkey2, (slot as u64, poh_estimate - acceptable_delta - 1)),
        ]
        .iter()
        .cloned()
        .collect();

        let bounded = calculate_bounded_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            Some((0, epoch_start_timestamp)),
        )
        .unwrap();
        assert_eq!(bounded, poh_estimate);

        // Test stake-weighted median within bounds
        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (pubkey0, (slot as u64, poh_estimate + acceptable_delta)),
            (pubkey1, (slot as u64, poh_estimate + acceptable_delta)),
            (pubkey2, (slot as u64, poh_estimate + acceptable_delta)),
        ]
        .iter()
        .cloned()
        .collect();

        let bounded = calculate_bounded_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            Some((0, epoch_start_timestamp)),
        )
        .unwrap();
        assert_eq!(bounded, poh_estimate + acceptable_delta);

        let unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)> = [
            (pubkey0, (slot as u64, poh_estimate - acceptable_delta)),
            (pubkey1, (slot as u64, poh_estimate - acceptable_delta)),
            (pubkey2, (slot as u64, poh_estimate - acceptable_delta)),
        ]
        .iter()
        .cloned()
        .collect();

        let bounded = calculate_bounded_stake_weighted_timestamp(
            &unique_timestamps,
            &stakes,
            slot as Slot,
            slot_duration,
            Some((0, epoch_start_timestamp)),
        )
        .unwrap();
        assert_eq!(bounded, poh_estimate - acceptable_delta);
    }
}
