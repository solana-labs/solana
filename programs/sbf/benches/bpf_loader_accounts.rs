#![feature(test)]
#![cfg(feature = "sbf_rust")]

use solana_sbf_rust_bench::instructions::Recurse;

extern crate test;

use {
    solana_bpf_loader_program::solana_bpf_loader_program,
    solana_runtime::{
        bank::Bank,
        bank_client::BankClient,
        genesis_utils::{create_genesis_config, GenesisConfigInfo},
        loader_utils::load_program,
    },
    solana_sbf_rust_bench::instructions::{BenchInstruction, ReadAccounts, WriteAccounts},
    solana_sdk::{
        account::AccountSharedData,
        bpf_loader,
        client::SyncClient,
        instruction::{AccountMeta, Instruction},
        message::Message,
        pubkey::Pubkey,
        signature::Signer,
    },
    std::sync::Arc,
    test::Bencher,
};

fn bench_accounts(
    bencher: &mut Bencher,
    num_accounts: usize,
    account_size: usize,
    instruction: BenchInstruction,
) {
    solana_logger::setup();

    let GenesisConfigInfo {
        genesis_config,
        mint_keypair,
        ..
    } = create_genesis_config(50);
    let mut bank = Bank::new_for_benches(&genesis_config);
    let (name, id, entrypoint) = solana_bpf_loader_program!();
    bank.add_builtin(&name, &id, entrypoint);
    let bank = Arc::new(bank);
    let bank_client = BankClient::new_shared(&bank);

    let program_id = load_program(
        &bank_client,
        &bpf_loader::id(),
        &mint_keypair,
        "solana_sbf_rust_bench",
    );

    let accounts = (0..num_accounts)
        .map(|_| {
            (
                Pubkey::new_unique(),
                AccountSharedData::new(10, account_size, &program_id),
            )
        })
        .collect::<Vec<_>>();

    for (pubkey, account) in &accounts {
        bank.store_account(pubkey, account);
    }

    let mint_pubkey = mint_keypair.pubkey();
    let mut account_metas = accounts
        .iter()
        .map(|(pubkey, _)| AccountMeta::new(*pubkey, false))
        .collect::<Vec<_>>();
    account_metas.push(AccountMeta::new(program_id, false));

    let instruction = Instruction {
        program_id,
        accounts: account_metas,
        data: instruction.to_bytes(),
    };
    let message = Message::new(&[instruction], Some(&mint_pubkey));

    bank_client
        .send_and_confirm_message(&[&mint_keypair], message.clone())
        .unwrap();

    bencher.iter(|| {
        bank.clear_signatures();
        bank_client
            .send_and_confirm_message(&[&mint_keypair], message.clone())
            .unwrap();
    });
}

macro_rules! bench_program_execute_read_accounts {
    ($($name:ident: $num_accounts:expr, $account_size:expr, $read_size:expr),+) => {
        $(
            #[bench]
            fn $name(bencher: &mut Bencher) {
                bench_accounts(bencher, $num_accounts, $account_size, BenchInstruction::ReadAccounts(ReadAccounts {
                    num_accounts: $num_accounts,
                    size: $read_size,
                }))
            }
        )+
    };
}

bench_program_execute_read_accounts!(
    bench_program_execute_read_1_1k_account: 1, 1024, 1,
    bench_program_execute_read_1_100k_account: 1, 100 * 1024, 1,
    bench_program_execute_read_1_1mb_account: 1, 1024 * 1024, 1,
    bench_program_execute_read_1_10mb_account: 1, 10 * 1024 * 1024, 1,

    bench_program_execute_read_10_1k_account: 10, 1024, 1,
    bench_program_execute_read_10_100k_account: 10, 100 * 1024, 1,
    bench_program_execute_read_10_1mb_account: 10, 1024 * 1024, 1,

    bench_program_execute_read_126_1k_account: 126, 1024, 1,
    bench_program_execute_read_126_100k_account: 126, 100 * 1024, 1,
    bench_program_execute_read_126_500k_account: 126, 500 * 1024, 1
);

macro_rules! bench_program_execute_write_accounts {
    ($($name:ident: $num_accounts:expr, $account_size:expr, $write_size:expr),+) => {
        $(
            #[bench]
            fn $name(bencher: &mut Bencher) {
                bench_accounts(bencher, $num_accounts, $account_size, BenchInstruction::WriteAccounts(WriteAccounts {
                    num_accounts: 1,
                    size: $write_size,
                }))
            }
        )+
    };
}

bench_program_execute_write_accounts!(
    bench_program_execute_write_1_1k_account: 1, 1024, 1,
    bench_program_execute_write_1_100k_account: 1, 100 * 1024, 1,
    bench_program_execute_write_1_1mb_account: 1, 1024 * 1024, 1,
    bench_program_execute_write_1_10mb_account: 1, 10 * 1024 * 1024, 1,

    bench_program_execute_write_10_1k_account: 10, 1024, 1,
    bench_program_execute_write_10_100k_account: 10, 100 * 1024, 1,
    bench_program_execute_write_10_1mb_account: 10, 1024 * 1024, 1,

    bench_program_execute_write_126_1k_account: 126, 1024, 1,
    bench_program_execute_write_126_100k_account: 126, 100 * 1024, 1,
    bench_program_execute_write_126_500k_account: 126, 500 * 1024, 1
);
macro_rules! bench_program_execute_recurse {
    ($($name:ident: $num_accounts:expr, $account_size:expr, $n:expr),+) => {
        $(
            #[bench]
            fn $name(bencher: &mut Bencher) {
                bench_accounts(bencher, $num_accounts, $account_size, BenchInstruction::Recurse(Recurse {
					n: $n
                }))
            }
        )+
    };
}

bench_program_execute_recurse!(
    bench_program_execute_recurse_1_1k_account: 1, 1024, 4,
    bench_program_execute_recurse_1_100k_account: 1, 100 * 1024, 4,
    bench_program_execute_recurse_1_1mb_account: 1, 1024 * 1024, 4,
    bench_program_execute_recurse_1_10mb_account: 1, 10 * 1024 * 1024, 4,

    bench_program_execute_recurse_10_1k_account: 10, 1024, 4,
    bench_program_execute_recurse_10_100k_account: 10, 100 * 1024, 4,

    bench_program_execute_recurse_50_1k_account: 50, 1024, 1,
    bench_program_execute_recurse_50_100k_account: 50, 100 * 1024, 1
);
