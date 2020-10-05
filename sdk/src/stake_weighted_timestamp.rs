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
    unique_timestamps: HashMap<Pubkey, (Slot, UnixTimestamp)>,
    stakes: &HashMap<Pubkey, (u64, Account)>,
    slot: Slot,
    slot_duration: Duration,
) -> Option<UnixTimestamp> {
    let (stake_weighted_timestamps_sum, total_stake) = unique_timestamps
        .into_iter()
        .filter_map(|(vote_pubkey, (timestamp_slot, timestamp))| {
            let offset = (slot - timestamp_slot) as u32 * slot_duration;
            stakes.get(&vote_pubkey).map(|(stake, _account)| {
                (
                    (timestamp as u128 + offset.as_secs() as u128) * *stake as u128,
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

/// Calculates an estimated slot timestamp based on the epoch start timestamp and a set of samples;
/// populated sample length must be >= 1, and the difference between the slot and the epoch start
/// slot must be < u32::MAX
pub fn calculate_timestamp_from_samples(
    samples: &HashMap<Slot, Option<UnixTimestamp>>,
    slot: Slot,
    epoch_start_timestamp: (Slot, UnixTimestamp),
) -> Option<UnixTimestamp> {
    let slot_delta = slot.checked_sub(epoch_start_timestamp.0)?;
    if slot_delta > std::u32::MAX as u64 {
        return None;
    }
    let mut sample_tuples: Vec<_> = samples
        .iter()
        .filter_map(|(slot, maybe_timestamp)| {
            maybe_timestamp.map(|timestamp| (*slot, timestamp as u64))
        })
        .collect();
    if sample_tuples.is_empty() {
        return None;
    }
    sample_tuples.insert(0, (epoch_start_timestamp.0, epoch_start_timestamp.1 as u64));
    sample_tuples.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let average_slot_duration = calculate_average_slot_duration(sample_tuples)?;
    Some((slot_delta as u32 * average_slot_duration).as_secs() as i64 + epoch_start_timestamp.1)
}

/// Calculates the mean slot duration of a monotonically increasing set of samples. First calculates
/// the mean slot duration between each pair of samples, and returns the mean of those means. Sample
/// length must be >= 2.
fn calculate_average_slot_duration(samples: Vec<(u64, u64)>) -> Option<Duration> {
    let mut slot_duration: Vec<Duration> = vec![];
    let gaps = samples.len().checked_sub(1)?;
    for x in 0..gaps {
        let (old_slot, old_timestamp) = samples[x];
        let (new_slot, new_timestamp) = samples[x + 1];
        let timestamp_delta = new_timestamp.checked_sub(old_timestamp)?;
        if timestamp_delta == 0 {
            return None;
        }
        let duration = Duration::from_secs(timestamp_delta);
        slot_duration.push(duration.checked_div((new_slot.checked_sub(old_slot)?) as u32)?);
    }
    Some(
        slot_duration
            .iter()
            .sum::<Duration>()
            .checked_div(gaps as u32)?,
    )
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
        let pubkey0 = Pubkey::new_rand();
        let pubkey1 = Pubkey::new_rand();
        let pubkey2 = Pubkey::new_rand();
        let pubkey3 = Pubkey::new_rand();
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
                unique_timestamps.clone(),
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
                unique_timestamps,
                &stakes,
                slot as Slot,
                slot_duration
            ),
            Some(recent_timestamp + expected_offset as i64)
        );
    }

    #[test]
    fn test_calculate_average_slot_duration() {
        let recent_timestamp: u64 = 1_578_909_000;

        let samples = vec![];
        assert_eq!(calculate_average_slot_duration(samples), None);

        let samples = vec![(0, recent_timestamp)];
        assert_eq!(calculate_average_slot_duration(samples), None);

        let samples = vec![(0, recent_timestamp), (1, recent_timestamp)];
        assert_eq!(calculate_average_slot_duration(samples), None);

        let samples = vec![(0, recent_timestamp), (0, recent_timestamp + 1)];
        assert_eq!(calculate_average_slot_duration(samples), None);

        let samples = vec![(0, recent_timestamp), (2, recent_timestamp + 2)];
        assert_eq!(
            calculate_average_slot_duration(samples),
            Some(Duration::from_secs(1))
        );

        let samples = vec![
            (0, recent_timestamp),
            (2, recent_timestamp + 2),
            (4, recent_timestamp + 3),
        ];
        assert_eq!(
            calculate_average_slot_duration(samples),
            Some(Duration::from_millis(750))
        );
    }

    #[test]
    fn test_calculate_timestamp_from_samples() {
        let recent_timestamp: UnixTimestamp = 1_578_909_000;
        let slot = 12;
        let epoch_starting_timestamp = (0, recent_timestamp);
        let mut samples = HashMap::new();
        assert_eq!(
            calculate_timestamp_from_samples(&samples, slot, epoch_starting_timestamp),
            None
        );

        // Insert some empty samples
        samples.insert(1, None);
        samples.insert(6, None);
        assert_eq!(
            calculate_timestamp_from_samples(&samples, slot, epoch_starting_timestamp),
            None
        );

        // Insert some populated samples, assert estimate changes as expected
        samples.insert(4, Some(recent_timestamp + 2)); // Mean slot duration is 0.5s
        assert_eq!(
            calculate_timestamp_from_samples(&samples, slot, epoch_starting_timestamp),
            Some(recent_timestamp + 6)
        );
        samples.insert(8, Some(recent_timestamp + 6)); // Mean slot duration is now 0.75s
        assert_eq!(
            calculate_timestamp_from_samples(&samples, slot, epoch_starting_timestamp),
            Some(recent_timestamp + 9)
        );

        // Ensure absence of empty samples has no effect
        samples.remove(&1).unwrap();
        samples.remove(&6).unwrap();
        assert_eq!(
            calculate_timestamp_from_samples(&samples, slot, epoch_starting_timestamp),
            Some(recent_timestamp + 9)
        );
    }
}
