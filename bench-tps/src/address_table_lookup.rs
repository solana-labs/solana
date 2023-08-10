use {
    crate::bench_tps_client::*,
    itertools::Itertools,
    log::*,
    solana_address_lookup_table_program::{
        instruction::{create_lookup_table, extend_lookup_table},
        state::AddressLookupTable,
    },
    solana_sdk::{
        commitment_config::CommitmentConfig,
        hash::Hash,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        slot_history::Slot,
        transaction::Transaction,
    },
    std::{sync::Arc, thread::sleep, time::Duration},
};

/// To create a lookup-table account via `client`, and extend it with `num_addresses`.
pub fn create_address_lookup_table_account<T: 'static + BenchTpsClient + Send + Sync + ?Sized>(
    client: Arc<T>,
    funding_key: &Keypair,
    num_addresses: usize,
) -> Result<Pubkey> {
    let (transaction, lookup_table_address) = build_create_lookup_table_tx(
        funding_key,
        client.get_slot().unwrap_or(0),
        client.get_latest_blockhash().unwrap(),
    );
    send_and_confirm_transaction(client.clone(), transaction, &lookup_table_address);

    // Due to legacy transaction's size limits, the number of addresses to be added by one
    // `extend_lookup_table` transaction is also limited. If `num_addresses` is greater
    // than NUMBER_OF_ADDRESSES_PER_EXTEND, multiple extending are required.
    const NUMBER_OF_ADDRESSES_PER_EXTEND: usize = 16;

    let mut i: usize = 0;
    while i < num_addresses {
        let extend_num_addresses = NUMBER_OF_ADDRESSES_PER_EXTEND.min(num_addresses - i);
        i += extend_num_addresses;

        let transaction = build_extend_lookup_table_tx(
            &lookup_table_address,
            funding_key,
            extend_num_addresses,
            client.get_latest_blockhash().unwrap(),
        );
        send_and_confirm_transaction(client.clone(), transaction, &lookup_table_address);
    }

    Ok(lookup_table_address)
}

fn build_create_lookup_table_tx(
    funding_key: &Keypair,
    recent_slot: Slot,
    recent_blockhash: Hash,
) -> (Transaction, Pubkey) {
    let (create_lookup_table_ix, lookup_table_address) = create_lookup_table(
        funding_key.pubkey(), // authority
        funding_key.pubkey(), // payer
        recent_slot,          // slot
    );

    (
        Transaction::new_signed_with_payer(
            &[create_lookup_table_ix],
            Some(&funding_key.pubkey()),
            &[funding_key],
            recent_blockhash,
        ),
        lookup_table_address,
    )
}

fn build_extend_lookup_table_tx(
    lookup_table_address: &Pubkey,
    funding_key: &Keypair,
    num_addresses: usize,
    recent_blockhash: Hash,
) -> Transaction {
    // generates random addresses to populate lookup table account
    let addresses = (0..num_addresses)
        .map(|_| Pubkey::new_unique())
        .collect_vec();
    let extend_lookup_table_ix = extend_lookup_table(
        *lookup_table_address,
        funding_key.pubkey(),       // authority
        Some(funding_key.pubkey()), // payer
        addresses,
    );

    Transaction::new_signed_with_payer(
        &[extend_lookup_table_ix],
        Some(&funding_key.pubkey()),
        &[funding_key],
        recent_blockhash,
    )
}

fn send_and_confirm_transaction<T: 'static + BenchTpsClient + Send + Sync + ?Sized>(
    client: Arc<T>,
    transaction: Transaction,
    lookup_table_address: &Pubkey,
) {
    let _tx_sig = client.send_transaction(transaction).unwrap();

    // Sleep a few slots to allow transactions to process
    sleep(Duration::from_secs(1));

    // confirm tx
    let lookup_table_account = client
        .get_account_with_commitment(lookup_table_address, CommitmentConfig::processed())
        .unwrap();
    let lookup_table = AddressLookupTable::deserialize(&lookup_table_account.data).unwrap();
    trace!("lookup table: {:?}", lookup_table);
}
