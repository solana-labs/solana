use {
    crate::{
        bench_tps_client::*,
    },
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
    std::{
        sync::Arc,
        thread::sleep,
        time::Duration,
    },
};

// Number of pubkeys to be included in single extend_lookup_table transaction that not exceeds MTU
const NUMBER_OF_ADDRESSES_PER_EXTEND: usize = 16;

pub fn create_address_lookup_table_account<T: 'static + BenchTpsClient + Send + Sync + ?Sized>(
    client: Arc<T>,
    funding_key: &Keypair,
    num_addresses: usize,
    keypairs: &Vec<Keypair>,
) -> Result<Pubkey> {
    let (transaction, lookup_table_address) = build_create_lookup_table_tx(
        funding_key,
        client.get_slot().unwrap_or(0),
        client.get_latest_blockhash().unwrap(),
    );
    send_and_confirm_transaction(client.clone(), transaction, &lookup_table_address);

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
    let mut addresses = Vec::with_capacity(num_addresses);
    // TODO - replace new_unique with keys from keypairs,
    //        Or maybe not necessary? Should log and check what accounts.rs does with new_unique
    //        keys when loading
    addresses.resize_with(num_addresses, Pubkey::new_unique);
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
    info!("==== {:?}", transaction);
    let tx_sig = client.send_transaction(transaction).unwrap();
    info!("==== {:?}", tx_sig);

    // Sleep a few slots to allow transactions to process
    sleep(Duration::from_secs(1));

    // confirm tx
    let lookup_table_account = client
        .get_account_with_commitment(&lookup_table_address, CommitmentConfig::processed())
        .unwrap();
    info!("==== {:?}", lookup_table_account);
    let lookup_table = AddressLookupTable::deserialize(&lookup_table_account.data).unwrap();
    info!("==== {:?}", lookup_table);
}
