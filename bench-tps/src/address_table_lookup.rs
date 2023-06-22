use {
    crate::{
        bench_tps_client::*,
        cli::{Config, InstructionPaddingConfig},
        perf_utils::{sample_txs, SampleStats},
        send_batch::*,
    },
    log::*,
    rand::distributions::{Distribution, Uniform},
    rayon::prelude::*,
    solana_address_lookup_table_program::{
        id,
        instruction::{create_lookup_table,},
        state::AddressLookupTable,
    },
    solana_client::{nonce_utils, rpc_request::MAX_MULTIPLE_ACCOUNTS},
    solana_metrics::{self, datapoint_info},
    solana_sdk::{
        account::Account,
        clock::{DEFAULT_MS_PER_SLOT, DEFAULT_S_PER_SLOT, MAX_PROCESSING_AGE},
        commitment_config::CommitmentConfig,
        compute_budget::ComputeBudgetInstruction,
        hash::Hash,
        instruction::{AccountMeta, Instruction},
        message::Message,
        native_token::Sol,
        pubkey::Pubkey,
        signature::{Keypair, Signer},
        system_instruction,
        timing::{duration_as_ms, duration_as_s, duration_as_us, timestamp},
        transaction::Transaction,
    },
    std::{
        sync::{
            Arc,
        },
        thread::sleep,
        time::{Duration, Instant},
    },
};

pub fn create_address_lookup_table_account<T: 'static + BenchTpsClient + Send + Sync + ?Sized>(
    client: Arc<T>,
    funding_key: &Keypair,
    number_of_accounts_in_atl: usize,
    keypairs: &Vec<Keypair>,
) /*-> Result<>*/ {
    let recent_slot = client.get_slot().unwrap_or(0);
    let (create_lookup_table_ix, lookup_table_address) = create_lookup_table(
        funding_key.pubkey(), // authority
        funding_key.pubkey(), // payer
        recent_slot, // slot
    );
    info!("==== {:?}, {:?}, {:?}", recent_slot, lookup_table_address, create_lookup_table_ix);

    {
        let recent_blockhash = client.get_latest_blockhash().unwrap();
        let transaction = Transaction::new_signed_with_payer(
            &[create_lookup_table_ix],
            Some(&funding_key.pubkey()),
            &[funding_key],
            recent_blockhash,
        );

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
}

