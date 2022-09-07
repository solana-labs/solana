#![feature(test)]

extern crate test;

use {
    solana_bpf_loader_program::serialization::serialize_parameters,
    solana_sdk::{
        account::{Account, AccountSharedData},
        bpf_loader, bpf_loader_deprecated,
        pubkey::Pubkey,
        sysvar::rent::Rent,
        transaction_context::{IndexOfAccount, InstructionAccount, TransactionContext},
    },
    test::Bencher,
};

fn create_inputs(owner: Pubkey, num_instruction_accounts: usize) -> TransactionContext {
    let program_id = solana_sdk::pubkey::new_rand();
    let transaction_accounts = vec![
        (
            program_id,
            AccountSharedData::from(Account {
                lamports: 0,
                data: vec![],
                owner,
                executable: true,
                rent_epoch: 0,
            }),
        ),
        (
            solana_sdk::pubkey::new_rand(),
            AccountSharedData::from(Account {
                lamports: 1,
                data: vec![1u8; 100000],
                owner,
                executable: false,
                rent_epoch: 100,
            }),
        ),
        (
            solana_sdk::pubkey::new_rand(),
            AccountSharedData::from(Account {
                lamports: 2,
                data: vec![11u8; 100000],
                owner,
                executable: true,
                rent_epoch: 200,
            }),
        ),
        (
            solana_sdk::pubkey::new_rand(),
            AccountSharedData::from(Account {
                lamports: 3,
                data: vec![],
                owner,
                executable: false,
                rent_epoch: 3100,
            }),
        ),
        (
            solana_sdk::pubkey::new_rand(),
            AccountSharedData::from(Account {
                lamports: 4,
                data: vec![1u8; 100000],
                owner,
                executable: false,
                rent_epoch: 100,
            }),
        ),
        (
            solana_sdk::pubkey::new_rand(),
            AccountSharedData::from(Account {
                lamports: 5,
                data: vec![11u8; 10000],
                owner,
                executable: true,
                rent_epoch: 200,
            }),
        ),
        (
            solana_sdk::pubkey::new_rand(),
            AccountSharedData::from(Account {
                lamports: 6,
                data: vec![],
                owner,
                executable: false,
                rent_epoch: 3100,
            }),
        ),
    ];
    let mut instruction_accounts: Vec<InstructionAccount> = Vec::new();
    for (instruction_account_index, index_in_transaction) in [1, 1, 2, 3, 4, 4, 5, 6]
        .into_iter()
        .cycle()
        .take(num_instruction_accounts)
        .enumerate()
    {
        let index_in_callee = instruction_accounts
            .iter()
            .position(|account| account.index_in_transaction == index_in_transaction)
            .unwrap_or(instruction_account_index) as IndexOfAccount;
        instruction_accounts.push(InstructionAccount {
            index_in_caller: instruction_account_index as IndexOfAccount,
            index_in_transaction,
            index_in_callee,
            is_signer: false,
            is_writable: instruction_account_index >= 4,
        });
    }

    let mut transaction_context =
        TransactionContext::new(transaction_accounts, Some(Rent::default()), 1, 1);
    let instruction_data = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];
    transaction_context
        .get_next_instruction_context()
        .unwrap()
        .configure(&[0], &instruction_accounts, &instruction_data);
    transaction_context.push().unwrap();
    transaction_context
}

#[bench]
fn bench_serialize_unaligned(bencher: &mut Bencher) {
    let transaction_context = create_inputs(bpf_loader_deprecated::id(), 7);
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .unwrap();
    bencher.iter(|| {
        let _ =
            serialize_parameters(&transaction_context, instruction_context, true, false).unwrap();
    });
}

#[bench]
fn bench_serialize_unaligned_copy_account_data(bencher: &mut Bencher) {
    let transaction_context = create_inputs(bpf_loader_deprecated::id(), 7);
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .unwrap();
    bencher.iter(|| {
        let _ =
            serialize_parameters(&transaction_context, instruction_context, true, true).unwrap();
    });
}

#[bench]
fn bench_serialize_aligned(bencher: &mut Bencher) {
    let transaction_context = create_inputs(bpf_loader::id(), 7);
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .unwrap();

    bencher.iter(|| {
        let _ =
            serialize_parameters(&transaction_context, instruction_context, true, false).unwrap();
    });
}

#[bench]
fn bench_serialize_aligned_copy_account_data(bencher: &mut Bencher) {
    let transaction_context = create_inputs(bpf_loader::id(), 7);
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .unwrap();

    bencher.iter(|| {
        let _ =
            serialize_parameters(&transaction_context, instruction_context, true, true).unwrap();
    });
}

#[bench]
fn bench_serialize_unaligned_max_accounts(bencher: &mut Bencher) {
    let transaction_context = create_inputs(bpf_loader_deprecated::id(), 255);
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .unwrap();
    bencher.iter(|| {
        let _ =
            serialize_parameters(&transaction_context, instruction_context, true, false).unwrap();
    });
}

#[bench]
fn bench_serialize_aligned_max_accounts(bencher: &mut Bencher) {
    let transaction_context = create_inputs(bpf_loader::id(), 255);
    let instruction_context = transaction_context
        .get_current_instruction_context()
        .unwrap();

    bencher.iter(|| {
        let _ =
            serialize_parameters(&transaction_context, instruction_context, true, false).unwrap();
    });
}
