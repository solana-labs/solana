#![feature(test)]

extern crate test;

use {
    solana_program_runtime::invoke_context::InvokeContext,
    solana_sdk::{
        account::{create_account_for_test, Account, AccountSharedData},
        clock::{Clock, Slot},
        feature_set::FeatureSet,
        hash::Hash,
        instruction::{AccountMeta, Instruction},
        keyed_account::KeyedAccount,
        pubkey::Pubkey,
        slot_hashes::{SlotHashes, MAX_ENTRIES},
        sysvar,
        transaction_context::{InstructionAccount, TransactionContext},
    },
    solana_vote_program::{
        vote_instruction::VoteInstruction,
        vote_state::{
            self, Vote, VoteInit, VoteState, VoteStateUpdate, VoteStateVersions,
            MAX_LOCKOUT_HISTORY,
        },
    },
    std::{cell::RefCell, collections::HashSet, sync::Arc},
    test::Bencher,
};

struct VoteComponents {
    slot_hashes: SlotHashes,
    clock: Clock,
    signers: HashSet<Pubkey>,
    authority_pubkey: Pubkey,
    vote_pubkey: Pubkey,
    vote_account: Account,
}

fn create_components(num_initial_votes: Slot) -> VoteComponents {
    let clock = Clock::default();
    let mut slot_hashes = SlotHashes::new(&[]);
    for i in 0..MAX_ENTRIES {
        // slot hashes is full in normal operation
        slot_hashes.add(i as Slot, Hash::new_unique());
    }

    let vote_pubkey = Pubkey::new_unique();
    let authority_pubkey = Pubkey::new_unique();
    let signers: HashSet<Pubkey> = vec![authority_pubkey].into_iter().collect();
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

        for next_vote_slot in 0..num_initial_votes {
            vote_state.process_next_vote_slot(next_vote_slot, 0);
        }
        let mut vote_account_data: Vec<u8> = vec![0; VoteState::size_of()];
        let versioned = VoteStateVersions::new_current(vote_state.clone());
        VoteState::serialize(&versioned, &mut vote_account_data).unwrap();

        Account {
            lamports: 1,
            data: vote_account_data,
            owner: solana_vote_program::id(),
            executable: false,
            rent_epoch: 0,
        }
    };

    VoteComponents {
        slot_hashes,
        clock,
        signers,
        authority_pubkey,
        vote_pubkey,
        vote_account,
    }
}

/// `feature` can be used to change vote program behavior per bench run.
fn do_bench_process_vote_instruction(bencher: &mut Bencher, feature: Option<Pubkey>) {
    // vote accounts are usually almost full of votes in normal operation
    let num_initial_votes = MAX_LOCKOUT_HISTORY as Slot;

    let VoteComponents {
        slot_hashes,
        clock,
        authority_pubkey,
        vote_pubkey,
        vote_account,
        ..
    } = create_components(num_initial_votes);

    let slot_hashes_account = create_account_for_test(&slot_hashes);
    let clock_account = create_account_for_test(&clock);
    let authority_account = Account::default();

    let mut feature_set = FeatureSet::all_enabled();
    if let Some(feature) = feature {
        feature_set.activate(&feature, 0);
    }
    let feature_set = Arc::new(feature_set);

    let num_vote_slots = 4;
    let last_vote_slot = num_initial_votes
        .saturating_add(num_vote_slots)
        .saturating_sub(1);
    let last_vote_hash = slot_hashes
        .iter()
        .find(|(slot, _hash)| *slot == last_vote_slot)
        .unwrap()
        .1;

    let vote_ix_data = bincode::serialize(&VoteInstruction::Vote(Vote::new(
        (num_initial_votes..=last_vote_slot).collect(),
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
            1,
        );

        let mut invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        invoke_context.feature_set = feature_set.clone();
        invoke_context
            .push(&instruction_accounts, &program_indices, &[])
            .unwrap();

        let first_instruction_account = 1;
        assert!(solana_vote_program::vote_processor::process_instruction(
            first_instruction_account,
            &instruction.data,
            &mut invoke_context
        )
        .is_ok());
    });
}

/// `feature` can be used to change vote program behavior per bench run.
fn do_bench_process_vote(bencher: &mut Bencher, feature: Option<Pubkey>) {
    // vote accounts are usually almost full of votes in normal operation
    let num_initial_votes = MAX_LOCKOUT_HISTORY as Slot;

    let VoteComponents {
        slot_hashes,
        clock,
        signers,
        vote_pubkey,
        vote_account,
        ..
    } = create_components(num_initial_votes);

    let num_vote_slots = 4;
    let last_vote_slot = num_initial_votes
        .saturating_add(num_vote_slots)
        .saturating_sub(1);
    let last_vote_hash = slot_hashes
        .iter()
        .find(|(slot, _hash)| *slot == last_vote_slot)
        .unwrap()
        .1;

    let vote = Vote::new(
        (num_initial_votes..=last_vote_slot).collect(),
        last_vote_hash,
    );

    let mut feature_set = FeatureSet::all_enabled();
    if let Some(feature) = feature {
        feature_set.activate(&feature, 0);
    }
    let feature_set = Arc::new(feature_set);

    bencher.iter(|| {
        let vote_account = RefCell::new(AccountSharedData::from(vote_account.clone()));
        let keyed_account = KeyedAccount::new(&vote_pubkey, true, &vote_account);
        assert!(vote_state::process_vote(
            &keyed_account,
            &slot_hashes,
            &clock,
            &vote,
            &signers,
            &feature_set,
        )
        .is_ok());
    });
}

fn do_bench_process_vote_state_update(bencher: &mut Bencher) {
    // vote accounts are usually almost full of votes in normal operation
    let num_initial_votes = MAX_LOCKOUT_HISTORY as Slot;

    let VoteComponents {
        slot_hashes,
        clock,
        signers,
        vote_pubkey,
        vote_account,
        ..
    } = create_components(num_initial_votes);

    let num_vote_slots = MAX_LOCKOUT_HISTORY as Slot;
    let last_vote_slot = num_initial_votes
        .saturating_add(num_vote_slots)
        .saturating_sub(1);
    let last_vote_hash = slot_hashes
        .iter()
        .find(|(slot, _hash)| *slot == last_vote_slot)
        .unwrap()
        .1;

    let slots_and_lockouts: Vec<(Slot, u32)> =
        ((num_initial_votes.saturating_add(1)..=last_vote_slot).zip((1u32..=31).rev())).collect();

    let mut vote_state_update = VoteStateUpdate::from(slots_and_lockouts);
    vote_state_update.root = Some(num_initial_votes);

    vote_state_update.hash = last_vote_hash;

    bencher.iter(|| {
        let vote_account = RefCell::new(AccountSharedData::from(vote_account.clone()));
        let keyed_account = KeyedAccount::new(&vote_pubkey, true, &vote_account);
        let vote_state_update = vote_state_update.clone();
        assert!(vote_state::process_vote_state_update(
            &keyed_account,
            &slot_hashes,
            &clock,
            vote_state_update,
            &signers,
        )
        .is_ok());
    });
}

#[bench]
#[ignore]
fn bench_process_vote_instruction(bencher: &mut Bencher) {
    do_bench_process_vote_instruction(bencher, None);
}

// Benches a specific type of vote instruction
#[bench]
#[ignore]
fn bench_process_vote(bencher: &mut Bencher) {
    do_bench_process_vote(bencher, None);
}

// Benches a specific type of vote instruction
#[bench]
#[ignore]
fn bench_process_vote_state_update(bencher: &mut Bencher) {
    do_bench_process_vote_state_update(bencher);
}
