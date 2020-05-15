use solana_ledger::entry::Entry;
use solana_ledger::shred::{
    max_entries_per_n_shred, verify_test_data_shred, Shred, Shredder, MAX_DATA_SHREDS_PER_FEC_BLOCK,
};
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::{hash::Hash, system_transaction};
use std::convert::TryInto;
use std::sync::Arc;

#[test]
fn test_multi_fec_block_coding() {
    let keypair = Arc::new(Keypair::new());
    let slot = 0x1234_5678_9abc_def0;
    let shredder = Shredder::new(slot, slot - 5, 1.0, keypair.clone(), 0, 0)
        .expect("Failed in creating shredder");

    let num_fec_sets = 100;
    let num_data_shreds = (MAX_DATA_SHREDS_PER_FEC_BLOCK * num_fec_sets) as usize;
    let keypair0 = Keypair::new();
    let keypair1 = Keypair::new();
    let tx0 = system_transaction::transfer(&keypair0, &keypair1.pubkey(), 1, Hash::default());
    let entry = Entry::new(&Hash::default(), 1, vec![tx0]);
    let num_entries = max_entries_per_n_shred(&entry, num_data_shreds as u64);

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
