#![feature(test)]

extern crate test;

use {
    solana_program_runtime::{invoke_context::InvokeContext, sysvar_cache::SysvarCache},
    solana_sdk::{
        account::{create_account_for_test, Account, AccountSharedData},
        clock::{Clock, Slot},
        feature_set::FeatureSet,
        hash::Hash,
        instruction::{AccountMeta, Instruction},
        pubkey::Pubkey,
        slot_hashes::{SlotHashes, MAX_ENTRIES},
        sysvar,
        transaction_context::{InstructionAccount, TransactionContext},
    },
    solana_vote_program::{
        vote_instruction::VoteInstruction,
        vote_state::{Vote, VoteInit, VoteState, VoteStateVersions, MAX_LOCKOUT_HISTORY},
    },
    std::sync::Arc,
    test::Bencher,
};

/// `feature` can be used to change vote program behavior per bench run.
fn do_bench(bencher: &mut Bencher, feature: Option<Pubkey>) {
    // vote accounts are usually almost full of votes in normal operation
    let num_initial_votes = MAX_LOCKOUT_HISTORY;
    let num_vote_slots: usize = 4;
    let last_vote_slot = num_initial_votes
        .saturating_add(num_vote_slots)
        .saturating_sub(1);
    let last_vote_hash = Hash::new_unique();

    let clock = Clock::default();
    let mut slot_hashes = SlotHashes::new(&[]);
    for i in 0..MAX_ENTRIES {
        // slot hashes is full in normal operation
        slot_hashes.add(
            i as Slot,
            if i == last_vote_slot {
                last_vote_hash
            } else {
                Hash::default()
            },
        );
    }

    let vote_pubkey = Pubkey::new_unique();
    let authority_pubkey = Pubkey::new_unique();
    let vote_account = {
        let mut vote_state = VoteState::new(
            &VoteInit {
                node_pubkey: authority_pubkey,
                authorized_voter: authority_pubkey,
                authorized_withdrawer: authority_pubkey,
                commission: 0,
            },
            &clock,
        );

        for next_vote_slot in 0..num_initial_votes as u64 {
            vote_state.process_next_vote_slot(next_vote_slot, 0);
        }

        let mut vote_account_data: Vec<u8> = vec![0; VoteState::size_of()];
        let versioned = VoteStateVersions::new_current(vote_state);
        VoteState::serialize(&versioned, &mut vote_account_data).unwrap();

        Account {
            lamports: 1,
            data: vote_account_data,
            owner: solana_vote_program::id(),
            executable: false,
            rent_epoch: 0,
        }
    };
    let slot_hashes_account = create_account_for_test(&slot_hashes);
    let clock_account = create_account_for_test(&clock);
    let authority_account = Account::default();

    let mut sysvar_cache = SysvarCache::default();
    sysvar_cache.set_clock(clock);
    sysvar_cache.set_slot_hashes(slot_hashes);

    let mut feature_set = FeatureSet::all_enabled();
    if let Some(feature) = feature {
        feature_set.activate(&feature, 0);
    }
    let feature_set = Arc::new(feature_set);

    let vote_ix_data = bincode::serialize(&VoteInstruction::Vote(Vote::new(
        (num_initial_votes as u64..).take(num_vote_slots).collect(),
        last_vote_hash,
    )))
    .unwrap();

    let instruction = Instruction {
        program_id: solana_vote_program::id(),
        data: vote_ix_data,
        accounts: vec![
            AccountMeta::new(vote_pubkey, false),
            AccountMeta::new_readonly(sysvar::slot_hashes::id(), false),
            AccountMeta::new_readonly(sysvar::clock::id(), false),
            AccountMeta::new_readonly(authority_pubkey, true),
        ],
    };

    let program_indices = vec![4];
    let instruction_accounts = instruction
        .accounts
        .iter()
        .enumerate()
        .map(|(index_in_transaction, account_meta)| InstructionAccount {
            index_in_transaction,
            index_in_caller: index_in_transaction,
            is_signer: account_meta.is_signer,
            is_writable: account_meta.is_writable,
        })
        .collect::<Vec<_>>();

    bencher.iter(|| {
        let mut transaction_context = TransactionContext::new(
            vec![
                (vote_pubkey, AccountSharedData::from(vote_account.clone())),
                (
                    sysvar::slot_hashes::id(),
                    AccountSharedData::from(slot_hashes_account.clone()),
                ),
                (
                    sysvar::clock::id(),
                    AccountSharedData::from(clock_account.clone()),
                ),
                (
                    authority_pubkey,
                    AccountSharedData::from(authority_account.clone()),
                ),
                (solana_vote_program::id(), AccountSharedData::default()),
            ],
            1,
        );

        let mut invoke_context = InvokeContext::new_mock_with_sysvars_and_features(
            &mut transaction_context,
            &sysvar_cache,
            feature_set.clone(),
        );

        invoke_context
            .push(&instruction_accounts, &program_indices, &[])
            .unwrap();

        let first_instruction_account = 1;
        assert_eq!(
            solana_vote_program::vote_instruction::process_instruction(
                first_instruction_account,
                &instruction.data,
                &mut invoke_context
            ),
            Ok(())
        );
    });
}

#[bench]
fn bench_process_vote_instruction(bencher: &mut Bencher) {
    do_bench(bencher, None);
}
