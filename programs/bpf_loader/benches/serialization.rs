#![feature(test)]

extern crate test;

use {
    solana_bpf_loader_program::serialization::{
        serialize_parameters_aligned, serialize_parameters_unaligned,
    },
    solana_sdk::{
        account::{Account, AccountSharedData},
        bpf_loader,
        keyed_account::KeyedAccount,
        pubkey::Pubkey,
    },
    std::cell::RefCell,
    test::Bencher,
};

fn create_inputs() -> (
    Pubkey,
    Vec<Pubkey>,
    Vec<RefCell<AccountSharedData>>,
    Vec<u8>,
) {
    let program_id = solana_sdk::pubkey::new_rand();
    let dup_key = solana_sdk::pubkey::new_rand();
    let dup_key2 = solana_sdk::pubkey::new_rand();
    let keys = vec![
        dup_key,
        dup_key,
        solana_sdk::pubkey::new_rand(),
        solana_sdk::pubkey::new_rand(),
        dup_key2,
        dup_key2,
        solana_sdk::pubkey::new_rand(),
        solana_sdk::pubkey::new_rand(),
    ];
    let accounts = vec![
        RefCell::new(AccountSharedData::from(Account {
            lamports: 1,
            data: vec![1u8, 2, 3, 4, 5],
            owner: bpf_loader::id(),
            executable: false,
            rent_epoch: 100,
        })),
        // dup
        RefCell::new(AccountSharedData::from(Account {
            lamports: 1,
            data: vec![1u8; 100000],
            owner: bpf_loader::id(),
            executable: false,
            rent_epoch: 100,
        })),
        RefCell::new(AccountSharedData::from(Account {
            lamports: 2,
            data: vec![11u8; 100000],
            owner: bpf_loader::id(),
            executable: true,
            rent_epoch: 200,
        })),
        RefCell::new(AccountSharedData::from(Account {
            lamports: 3,
            data: vec![],
            owner: bpf_loader::id(),
            executable: false,
            rent_epoch: 3100,
        })),
        RefCell::new(AccountSharedData::from(Account {
            lamports: 4,
            data: vec![1u8; 100000],
            owner: bpf_loader::id(),
            executable: false,
            rent_epoch: 100,
        })),
        // dup
        RefCell::new(AccountSharedData::from(Account {
            lamports: 4,
            data: vec![1u8; 1000000],
            owner: bpf_loader::id(),
            executable: false,
            rent_epoch: 100,
        })),
        RefCell::new(AccountSharedData::from(Account {
            lamports: 5,
            data: vec![11u8; 10000],
            owner: bpf_loader::id(),
            executable: true,
            rent_epoch: 200,
        })),
        RefCell::new(AccountSharedData::from(Account {
            lamports: 6,
            data: vec![],
            owner: bpf_loader::id(),
            executable: false,
            rent_epoch: 3100,
        })),
    ];

    let instruction_data = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11];

    (program_id, keys, accounts, instruction_data)
}

#[bench]
fn bench_serialize_unaligned(bencher: &mut Bencher) {
    let (program_id, keys, accounts, instruction_data) = create_inputs();
    let keyed_accounts: Vec<_> = keys
        .iter()
        .zip(&accounts)
        .enumerate()
        .map(|(i, (key, account))| {
            if i <= accounts.len() / 2 {
                KeyedAccount::new_readonly(key, false, account)
            } else {
                KeyedAccount::new(key, false, account)
            }
        })
        .collect();
    bencher.iter(|| {
        let _ = serialize_parameters_unaligned(&program_id, &keyed_accounts, &instruction_data)
            .unwrap();
    });
}

#[bench]
fn bench_serialize_aligned(bencher: &mut Bencher) {
    let (program_id, keys, accounts, instruction_data) = create_inputs();
    let keyed_accounts: Vec<_> = keys
        .iter()
        .zip(&accounts)
        .enumerate()
        .map(|(i, (key, account))| {
            if i <= accounts.len() / 2 {
                KeyedAccount::new_readonly(key, false, account)
            } else {
                KeyedAccount::new(key, false, account)
            }
        })
        .collect();
    bencher.iter(|| {
        let _ =
            serialize_parameters_aligned(&program_id, &keyed_accounts, &instruction_data).unwrap();
    });
}
