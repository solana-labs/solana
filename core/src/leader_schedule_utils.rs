use crate::blocktree::Blocktree;
use crate::leader_schedule::LeaderSchedule;
use crate::staking_utils;
use solana_runtime::bank::Bank;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::timing::NUM_CONSECUTIVE_LEADER_SLOTS;

/// Return the leader schedule for the given epoch.
fn leader_schedule(epoch_height: u64, bank: &Bank) -> Option<LeaderSchedule> {
    staking_utils::delegated_stakes_at_epoch(bank, epoch_height).map(|stakes| {
        let mut seed = [0u8; 32];
        seed[0..8].copy_from_slice(&epoch_height.to_le_bytes());
        let mut stakes: Vec<_> = stakes.into_iter().collect();
        sort_stakes(&mut stakes);
        LeaderSchedule::new(
            &stakes,
            seed,
            bank.get_slots_in_epoch(epoch_height),
            NUM_CONSECUTIVE_LEADER_SLOTS,
        )
    })
}

fn sort_stakes(stakes: &mut Vec<(Pubkey, u64)>) {
    // Sort first by stake. If stakes are the same, sort by pubkey to ensure a
    // deterministic result.
    // Note: Use unstable sort, because we dedup right after to remove the equal elements.
    stakes.sort_unstable_by(|(l_id, l_stake), (r_id, r_stake)| {
        if r_stake == l_stake {
            r_id.cmp(&l_id)
        } else {
            r_stake.cmp(&l_stake)
        }
    });

    // Now that it's sorted, we can do an O(n) dedup.
    stakes.dedup();
}

/// Return the leader for the given slot.
pub fn slot_leader_at(slot: u64, bank: &Bank) -> Option<Pubkey> {
    let (epoch, slot_index) = bank.get_epoch_and_slot_index(slot);

    leader_schedule(epoch, bank).map(|leader_schedule| leader_schedule[slot_index])
}

/// Return the next slot after the given current_slot that the given node will be leader
pub fn next_leader_slot(
    pubkey: &Pubkey,
    mut current_slot: u64,
    bank: &Bank,
    blocktree: Option<&Blocktree>,
) -> Option<u64> {
    let (mut epoch, mut start_index) = bank.get_epoch_and_slot_index(current_slot + 1);
    while let Some(leader_schedule) = leader_schedule(epoch, bank) {
        // clippy thinks I should do this:
        //  for (i, <item>) in leader_schedule
        //                           .iter()
        //                           .enumerate()
        //                           .take(bank.get_slots_in_epoch(epoch))
        //                           .skip(from_slot_index + 1) {
        //
        //  but leader_schedule doesn't implement Iter...
        #[allow(clippy::needless_range_loop)]
        for i in start_index..bank.get_slots_in_epoch(epoch) {
            current_slot += 1;
            if *pubkey == leader_schedule[i] {
                if let Some(blocktree) = blocktree {
                    if let Some(meta) = blocktree.meta(current_slot).unwrap() {
                        // We have already sent a blob for this slot, so skip it
                        if meta.received > 0 {
                            continue;
                        }
                    }
                }

                return Some(current_slot);
            }
        }

        epoch += 1;
        start_index = 0;
    }
    None
}

// Returns the number of ticks remaining from the specified tick_height to the end of the
// slot implied by the tick_height
pub fn num_ticks_left_in_slot(bank: &Bank, tick_height: u64) -> u64 {
    bank.ticks_per_slot() - tick_height % bank.ticks_per_slot() - 1
}

pub fn tick_height_to_slot(ticks_per_slot: u64, tick_height: u64) -> u64 {
    tick_height / ticks_per_slot
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blocktree::get_tmp_ledger_path;
    use crate::blocktree::tests::make_slot_entries;
    use crate::staking_utils;
    use crate::voting_keypair::tests::new_vote_account;
    use solana_sdk::genesis_block::{GenesisBlock, BOOTSTRAP_LEADER_LAMPORTS};
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use std::sync::Arc;

    #[test]
    fn test_next_leader_slot() {
        let pubkey = Pubkey::new_rand();
        let mut genesis_block = GenesisBlock::new_with_leader(
            BOOTSTRAP_LEADER_LAMPORTS,
            &pubkey,
            BOOTSTRAP_LEADER_LAMPORTS,
        )
        .0;
        genesis_block.epoch_warmup = false;

        let bank = Bank::new(&genesis_block);
        assert_eq!(slot_leader_at(bank.slot(), &bank).unwrap(), pubkey);
        assert_eq!(next_leader_slot(&pubkey, 0, &bank, None), Some(1));
        assert_eq!(next_leader_slot(&pubkey, 1, &bank, None), Some(2));
        assert_eq!(
            next_leader_slot(
                &pubkey,
                2 * genesis_block.slots_per_epoch - 1, // no schedule generated for epoch 2
                &bank,
                None
            ),
            None
        );

        assert_eq!(
            next_leader_slot(
                &Pubkey::new_rand(), // not in leader_schedule
                0,
                &bank,
                None
            ),
            None
        );
    }

    #[test]
    fn test_next_leader_slot_blocktree() {
        let pubkey = Pubkey::new_rand();
        let mut genesis_block = GenesisBlock::new_with_leader(
            BOOTSTRAP_LEADER_LAMPORTS,
            &pubkey,
            BOOTSTRAP_LEADER_LAMPORTS,
        )
        .0;
        genesis_block.epoch_warmup = false;

        let bank = Bank::new(&genesis_block);
        let ledger_path = get_tmp_ledger_path!();
        {
            let blocktree = Arc::new(
                Blocktree::open(&ledger_path).expect("Expected to be able to open database ledger"),
            );

            assert_eq!(slot_leader_at(bank.slot(), &bank).unwrap(), pubkey);
            // Check that the next leader slot after 0 is slot 1
            assert_eq!(
                next_leader_slot(&pubkey, 0, &bank, Some(&blocktree)),
                Some(1)
            );

            // Write a blob into slot 2 that chains to slot 1,
            // but slot 1 is empty so should not be skipped
            let (blobs, _) = make_slot_entries(2, 1, 1);
            blocktree.write_blobs(&blobs[..]).unwrap();
            assert_eq!(
                next_leader_slot(&pubkey, 0, &bank, Some(&blocktree)),
                Some(1)
            );

            // Write a blob into slot 1
            let (blobs, _) = make_slot_entries(1, 0, 1);

            // Check that slot 1 and 2 are skipped
            blocktree.write_blobs(&blobs[..]).unwrap();
            assert_eq!(
                next_leader_slot(&pubkey, 0, &bank, Some(&blocktree)),
                Some(3)
            );

            // Integrity checks
            assert_eq!(
                next_leader_slot(
                    &pubkey,
                    2 * genesis_block.slots_per_epoch - 1, // no schedule generated for epoch 2
                    &bank,
                    Some(&blocktree)
                ),
                None
            );

            assert_eq!(
                next_leader_slot(
                    &Pubkey::new_rand(), // not in leader_schedule
                    0,
                    &bank,
                    Some(&blocktree)
                ),
                None
            );
        }
        Blocktree::destroy(&ledger_path).unwrap();
    }

    #[test]
    fn test_next_leader_slot_next_epoch() {
        let pubkey = Pubkey::new_rand();
        let (mut genesis_block, mint_keypair) = GenesisBlock::new_with_leader(
            2 * BOOTSTRAP_LEADER_LAMPORTS,
            &pubkey,
            BOOTSTRAP_LEADER_LAMPORTS,
        );
        genesis_block.epoch_warmup = false;

        let bank = Bank::new(&genesis_block);
        let delegate_id = Pubkey::new_rand();

        // Create new vote account
        let new_voting_keypair = Keypair::new();
        new_vote_account(
            &mint_keypair,
            &new_voting_keypair,
            &delegate_id,
            &bank,
            BOOTSTRAP_LEADER_LAMPORTS,
        );

        // Have to wait until the epoch at after the epoch stakes generated at genesis
        // for the new votes to take effect.
        let mut target_slot = 1;
        let epoch = bank.get_stakers_epoch(0);
        while bank.get_stakers_epoch(target_slot) == epoch {
            target_slot += 1;
        }

        let bank = Bank::new_from_parent(&Arc::new(bank), &Pubkey::default(), target_slot);
        let mut expected_slot = 0;
        let epoch = bank.get_stakers_epoch(target_slot);
        for i in 0..epoch {
            expected_slot += bank.get_slots_in_epoch(i);
        }

        let schedule = leader_schedule(epoch, &bank).unwrap();
        let mut index = 0;
        while schedule[index] != delegate_id {
            index += 1
        }

        expected_slot += index;

        assert_eq!(
            next_leader_slot(&delegate_id, 0, &bank, None),
            Some(expected_slot),
        );
    }

    #[test]
    fn test_leader_schedule_via_bank() {
        let pubkey = Pubkey::new_rand();
        let (genesis_block, _mint_keypair) = GenesisBlock::new_with_leader(
            BOOTSTRAP_LEADER_LAMPORTS,
            &pubkey,
            BOOTSTRAP_LEADER_LAMPORTS,
        );
        let bank = Bank::new(&genesis_block);

        let ids_and_stakes: Vec<_> = staking_utils::delegated_stakes(&bank).into_iter().collect();
        let seed = [0u8; 32];
        let leader_schedule = LeaderSchedule::new(
            &ids_and_stakes,
            seed,
            genesis_block.slots_per_epoch,
            NUM_CONSECUTIVE_LEADER_SLOTS,
        );

        assert_eq!(leader_schedule[0], pubkey);
        assert_eq!(leader_schedule[1], pubkey);
        assert_eq!(leader_schedule[2], pubkey);
    }

    #[test]
    fn test_leader_scheduler1_basic() {
        let pubkey = Pubkey::new_rand();
        let genesis_block = GenesisBlock::new_with_leader(
            BOOTSTRAP_LEADER_LAMPORTS,
            &pubkey,
            BOOTSTRAP_LEADER_LAMPORTS,
        )
        .0;
        let bank = Bank::new(&genesis_block);
        assert_eq!(slot_leader_at(bank.slot(), &bank).unwrap(), pubkey);
    }

    #[test]
    fn test_sort_stakes_basic() {
        let pubkey0 = Pubkey::new_rand();
        let pubkey1 = Pubkey::new_rand();
        let mut stakes = vec![(pubkey0, 1), (pubkey1, 2)];
        sort_stakes(&mut stakes);
        assert_eq!(stakes, vec![(pubkey1, 2), (pubkey0, 1)]);
    }

    #[test]
    fn test_sort_stakes_with_dup() {
        let pubkey0 = Pubkey::new_rand();
        let pubkey1 = Pubkey::new_rand();
        let mut stakes = vec![(pubkey0, 1), (pubkey1, 2), (pubkey0, 1)];
        sort_stakes(&mut stakes);
        assert_eq!(stakes, vec![(pubkey1, 2), (pubkey0, 1)]);
    }

    #[test]
    fn test_sort_stakes_with_equal_stakes() {
        let pubkey0 = Pubkey::default();
        let pubkey1 = Pubkey::new_rand();
        let mut stakes = vec![(pubkey0, 1), (pubkey1, 1)];
        sort_stakes(&mut stakes);
        assert_eq!(stakes, vec![(pubkey1, 1), (pubkey0, 1)]);
    }
}
