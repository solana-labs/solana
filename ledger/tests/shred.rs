use solana_ledger::entry::Entry;
use solana_ledger::shred::{
    max_entries_per_n_shred, sort_data_coding_into_fec_sets, verify_test_data_shred, Shred,
    Shredder, MAX_DATA_SHREDS_PER_FEC_BLOCK, SIZE_OF_DATA_SHRED_PAYLOAD,
};
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::{clock::Slot, hash::Hash, system_transaction};
use std::{collections::BTreeMap, convert::TryInto, sync::Arc};

fn run_test_multi_fec_block_coding(slot: Slot) {
    let keypair = Arc::new(Keypair::new());
    let shredder = Shredder::new(slot, slot - 5, 1.0, keypair.clone(), 0, 0)
        .expect("Failed in creating shredder");

    let num_fec_sets = 100;
    let num_data_shreds = (MAX_DATA_SHREDS_PER_FEC_BLOCK * num_fec_sets) as usize;
    let keypair0 = Keypair::new();
    let keypair1 = Keypair::new();
    let tx0 = system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
    let entry = Entry::new(&Hash::default(), 1, vec![tx0]);
    let num_entries = max_entries_per_n_shred(
        &entry,
        num_data_shreds as u64,
        Some(SIZE_OF_DATA_SHRED_PAYLOAD),
    );

    let entries: Vec<_> = (0..num_entries)
        .map(|_| {
            let keypair0 = Keypair::new();
            let keypair1 = Keypair::new();
            let tx0 =
                system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
            Entry::new(&Hash::default(), 1, vec![tx0])
        })
        .collect();

    let serialized_entries = bincode::serialize(&entries).unwrap();
    let (data_shreds, coding_shreds, next_index) = shredder.entries_to_shreds(&entries, true, 0);
    assert_eq!(next_index as usize, num_data_shreds);
    assert_eq!(data_shreds.len(), num_data_shreds);
    assert_eq!(coding_shreds.len(), num_data_shreds);

    for c in &coding_shreds {
        assert!(!c.is_data());
    }

    let mut all_shreds = vec![];
    for i in 0..num_fec_sets {
        let shred_start_index = (MAX_DATA_SHREDS_PER_FEC_BLOCK * i) as usize;
        let end_index = shred_start_index + MAX_DATA_SHREDS_PER_FEC_BLOCK as usize - 1;
        let fec_set_shreds = data_shreds[shred_start_index..=end_index]
            .iter()
            .cloned()
            .chain(coding_shreds[shred_start_index..=end_index].iter().cloned())
            .collect::<Vec<_>>();

        let mut shred_info: Vec<Shred> = fec_set_shreds
            .iter()
            .enumerate()
            .filter_map(|(i, b)| if i % 2 != 0 { Some(b.clone()) } else { None })
            .collect();

        let recovered_data = Shredder::try_recovery(
            shred_info.clone(),
            MAX_DATA_SHREDS_PER_FEC_BLOCK as usize,
            MAX_DATA_SHREDS_PER_FEC_BLOCK as usize,
            shred_start_index,
            shred_start_index,
            slot,
        )
        .unwrap();

        for (i, recovered_shred) in recovered_data.into_iter().enumerate() {
            let index = shred_start_index + (i * 2);
            verify_test_data_shred(
                &recovered_shred,
                index.try_into().unwrap(),
                slot,
                slot - 5,
                &keypair.pubkey(),
                true,
                index == end_index,
                index == end_index,
            );

            shred_info.insert(i * 2, recovered_shred);
        }

        all_shreds.extend(
            shred_info
                .into_iter()
                .take(MAX_DATA_SHREDS_PER_FEC_BLOCK as usize),
        );
    }

    let result = Shredder::deshred(&all_shreds[..]).unwrap();
    assert_eq!(serialized_entries[..], result[..serialized_entries.len()]);
}

#[test]
fn test_multi_fec_block_coding() {
    run_test_multi_fec_block_coding(0x1234_5678_9abc_def0);
}

#[test]
fn test_multi_fec_block_different_size_coding() {
    let keypair = Arc::new(Keypair::new());
    let slot = 0x1234_5678_9abc_def0;
    let parent_slot = slot - 5;
    let shredder = Shredder::new(slot, parent_slot, 1.0, keypair.clone(), 0, 0)
        .expect("Failed in creating shredder");
    let keypair0 = Keypair::new();
    let keypair1 = Keypair::new();
    let tx0 = system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
    let entry = Entry::new(&Hash::default(), 1, vec![tx0]);

    // Make enough entries for `MAX_DATA_SHREDS_PER_FEC_BLOCK + 2` shreds so one
    // fec set will have `MAX_DATA_SHREDS_PER_FEC_BLOCK` shreds and the next
    // will have 2 shreds.
    assert!(MAX_DATA_SHREDS_PER_FEC_BLOCK > 2);
    let num_entries = max_entries_per_n_shred(
        &entry,
        MAX_DATA_SHREDS_PER_FEC_BLOCK as u64 + 2,
        Some(SIZE_OF_DATA_SHRED_PAYLOAD),
    );

    let entries: Vec<_> = (0..num_entries)
        .map(|_| {
            let keypair0 = Keypair::new();
            let keypair1 = Keypair::new();
            let tx0 =
                system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
            Entry::new(&Hash::default(), 1, vec![tx0])
        })
        .collect();

    // Run the shredder twice, generate data and coding shreds
    let mut next_index = 0;
    let mut fec_data = BTreeMap::new();
    let mut fec_coding = BTreeMap::new();
    for _ in 0..2 {
        let (data_shreds, coding_shreds, new_next_index) =
            shredder.entries_to_shreds(&entries, false, next_index);
        assert_eq!(
            data_shreds.len(),
            MAX_DATA_SHREDS_PER_FEC_BLOCK as usize + 2
        );
        next_index = new_next_index;
        sort_data_coding_into_fec_sets(data_shreds, coding_shreds, &mut fec_data, &mut fec_coding);
    }

    for fec_set in fec_data.values() {
        assert!(fec_set.last().unwrap().data_complete());
    }

    assert_eq!(fec_data.len(), fec_coding.len());

    // Test recovery
    for (fec_data_shreds, fec_coding_shreds) in fec_data.values().zip(fec_coding.values()) {
        let first_data_index = fec_data_shreds.first().unwrap().index() as usize;
        let first_code_index = fec_coding_shreds.first().unwrap().index() as usize;
        let num_data = fec_data_shreds.len();
        let num_coding = fec_coding_shreds.len();
        let all_shreds: Vec<Shred> = fec_data_shreds
            .into_iter()
            .step_by(2)
            .chain(fec_coding_shreds.into_iter().step_by(2))
            .cloned()
            .collect();

        let recovered_data = Shredder::try_recovery(
            all_shreds,
            num_data,
            num_coding,
            first_data_index,
            first_code_index,
            slot,
        )
        .unwrap();

        for (i, recovered_shred) in recovered_data.into_iter().enumerate() {
            let index = first_data_index + (i * 2) + 1;
            // position within fec set
            let fec_set_index = index - first_data_index;
            verify_test_data_shred(
                &recovered_shred,
                index.try_into().unwrap(),
                slot,
                parent_slot,
                &keypair.pubkey(),
                true,
                false,
                fec_set_index == num_data - 1,
            );
        }
    }
}
