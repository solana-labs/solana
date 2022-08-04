#![feature(test)]

extern crate test;

use {
    solana_program_runtime::invoke_context::InvokeContext,
    solana_sdk::{
        account::{create_account_for_test, Account, AccountSharedData},
        clock::{Clock, Slot},
        hash::Hash,
        pubkey::Pubkey,
        slot_hashes::{SlotHashes, MAX_ENTRIES},
        sysvar,
        transaction_context::{InstructionAccount, TransactionAccount, TransactionContext},
    },
    solana_vote_program::{
        vote_instruction::VoteInstruction,
        vote_state::{
            Vote, VoteInit, VoteState, VoteStateUpdate, VoteStateVersions, MAX_LOCKOUT_HISTORY,
        },
    },
    test::Bencher,
};

fn create_accounts() -> (
    Slot,
    SlotHashes,
    Vec<TransactionAccount>,
    Vec<InstructionAccount>,
) {
    // vote accounts are usually almost full of votes in normal operation
    let num_initial_votes = MAX_LOCKOUT_HISTORY as Slot;

    let clock = Clock::default();
    let mut slot_hashes = SlotHashes::new(&[]);
    for i in 0..MAX_ENTRIES {
        // slot hashes is full in normal operation
        slot_hashes.add(i as Slot, Hash::new_unique());
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

        for next_vote_slot in 0..num_initial_votes {
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

    let transaction_accounts = vec![
        (solana_vote_program::id(), AccountSharedData::default()),
        (vote_pubkey, AccountSharedData::from(vote_account)),
        (
            sysvar::slot_hashes::id(),
            AccountSharedData::from(create_account_for_test(&slot_hashes)),
        ),
        (
            sysvar::clock::id(),
            AccountSharedData::from(create_account_for_test(&clock)),
        ),
        (authority_pubkey, AccountSharedData::default()),
    ];
    let mut instruction_accounts = (0..4)
        .map(|index_in_callee| InstructionAccount {
            index_in_transaction: 1usize.saturating_add(index_in_callee),
            index_in_caller: index_in_callee,
            index_in_callee,
            is_signer: false,
            is_writable: false,
        })
        .collect::<Vec<InstructionAccount>>();
    instruction_accounts[0].is_writable = true;
    instruction_accounts[3].is_signer = true;

    (
        num_initial_votes,
        slot_hashes,
        transaction_accounts,
        instruction_accounts,
    )
}

fn bench_process_vote_instruction(
    bencher: &mut Bencher,
    transaction_accounts: Vec<TransactionAccount>,
    instruction_accounts: Vec<InstructionAccount>,
    instruction_data: Vec<u8>,
) {
    bencher.iter(|| {
        let mut transaction_context = TransactionContext::new(
            transaction_accounts.clone(),
            Some(sysvar::rent::Rent::default()),
            1,
            1,
        );
        let mut invoke_context = InvokeContext::new_mock(&mut transaction_context, &[]);
        invoke_context
            .push(&instruction_accounts, &[0], &instruction_data)
            .unwrap();
        assert!(
            solana_vote_program::vote_processor::process_instruction(1, &mut invoke_context)
                .is_ok()
        );
        invoke_context.pop().unwrap();
    });
}

#[bench]
fn bench_process_vote(bencher: &mut Bencher) {
    let (num_initial_votes, slot_hashes, transaction_accounts, instruction_accounts) =
        create_accounts();

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
    let instruction_data = bincode::serialize(&VoteInstruction::Vote(vote)).unwrap();

    bench_process_vote_instruction(
        bencher,
        transaction_accounts,
        instruction_accounts,
        instruction_data,
    );
}

#[bench]
fn bench_process_vote_state_update(bencher: &mut Bencher) {
    let (num_initial_votes, slot_hashes, transaction_accounts, instruction_accounts) =
        create_accounts();

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
    let instruction_data =
        bincode::serialize(&VoteInstruction::UpdateVoteState(vote_state_update)).unwrap();

    bench_process_vote_instruction(
        bencher,
        transaction_accounts,
        instruction_accounts,
        instruction_data,
    );
}
