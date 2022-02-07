#![feature(test)]
#![allow(clippy::integer_arithmetic)]

extern crate test;

use {
    solana_runtime::{accounts_db::ErrorCounters, bank::Bank, transaction_batch::TransactionBatch},
    solana_sdk::{
        account::{Account, AccountSharedData},
        clock::Clock,
        genesis_config::create_genesis_config,
        message::Message,
        pubkey::Pubkey,
        transaction::{SanitizedTransaction, Transaction},
    },
    solana_vote_program::{
        vote_instruction,
        vote_state::{Vote, VoteInit, VoteState, VoteStateVersions},
    },
    std::{
        borrow::Cow,
        sync::{Arc, RwLock},
    },
    test::Bencher,
};

fn setup_vote_transaction_batch(bank: &Bank, num_transactions: usize) -> TransactionBatch {
    let txs = setup_vote_accounts_and_transactions(bank, num_transactions);

    let sanitized_txs = txs
        .into_iter()
        .map(|tx| SanitizedTransaction::try_from_legacy_transaction(tx).unwrap())
        .collect();

    TransactionBatch::new(
        vec![Ok(()); num_transactions],
        bank,
        Cow::Owned(sanitized_txs),
    )
}

fn setup_vote_accounts_and_transactions(bank: &Bank, num_transactions: usize) -> Vec<Transaction> {
    let recent_blockhash = bank.last_blockhash();
    let vote_bank_hash = bank.parent_hash();

    let payer_account_data = AccountSharedData::from(Account {
        lamports: 27_000_000,
        data: vec![],
        owner: Pubkey::default(),
        executable: false,
        rent_epoch: 1,
    });

    let vote_authority_key = Pubkey::new_unique();
    let vote_account_data = {
        let mut vote_account_initial_data =
            bincode::serialize(&VoteStateVersions::new_current(VoteState::new(
                &VoteInit {
                    node_pubkey: Pubkey::default(),
                    authorized_voter: vote_authority_key,
                    authorized_withdrawer: Pubkey::default(),
                    commission: 100u8,
                },
                &Clock::default(),
            )))
            .unwrap();
        vote_account_initial_data.resize(VoteState::size_of(), 0);
        AccountSharedData::from(Account {
            lamports: 26_858_640,
            data: vote_account_initial_data.clone(),
            owner: solana_vote_program::id(),
            executable: false,
            rent_epoch: 1,
        })
    };

    (0..num_transactions)
        .map(|_| {
            // Setup payer account
            let payer_key = Pubkey::new_unique();
            bank.store_account(&payer_key, &payer_account_data);

            // Setup vote account
            let vote_key = Pubkey::new_unique();
            bank.store_account(&vote_key, &vote_account_data);

            let vote_ix = vote_instruction::vote(
                &vote_key,
                &vote_authority_key,
                Vote {
                    slots: vec![0],
                    hash: vote_bank_hash,
                    timestamp: None,
                },
            );
            let message =
                Message::new_with_blockhash(&[vote_ix], Some(&payer_key), &recent_blockhash);
            Transaction::new_unsigned(message)
        })
        .collect()
}

#[bench]
fn bench_load_transaction_batch(bencher: &mut Bencher) {
    let (genesis_config, ..) = create_genesis_config(100_000_000_000_000);
    let bank = Arc::new(Bank::new_for_benches(&genesis_config));
    let bank = Bank::new_from_parent(&bank, &Pubkey::default(), 1);

    let thread_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap();

    let tx_batch_size = 100;
    let tx_batch = setup_vote_transaction_batch(&bank, tx_batch_size);
    let sanitized_txs = tx_batch.sanitized_transactions();
    thread_pool.install(|| {
        bencher.iter(|| {
            let lock_results: Vec<_> = tx_batch
                .lock_results()
                .iter()
                .map(|result| (result.clone(), None))
                .collect();
            let loaded_account_results = tx_batch.bank().load_transaction_batch_accounts(
                sanitized_txs,
                lock_results,
                &RwLock::new(ErrorCounters::default()),
            );
            assert_eq!(
                loaded_account_results
                    .into_iter()
                    .try_for_each(|(result, ..)| result.map(|_| ())),
                Ok(())
            );
        });
    });
}
