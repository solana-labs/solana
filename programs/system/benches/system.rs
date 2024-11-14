#[allow(deprecated)]
use {
    criterion::{criterion_group, criterion_main, Criterion},
    solana_program_runtime::invoke_context::mock_process_instruction,
    solana_sdk::{
        account::{self, AccountSharedData, WritableAccount},
        hash::Hash,
        instruction::AccountMeta,
        nonce::{
            state::{DurableNonce, Versions},
            State,
        },
        pubkey::Pubkey,
        system_instruction::SystemInstruction,
        system_program,
        sysvar::{
            recent_blockhashes::{self, IterItem, RecentBlockhashes, MAX_ENTRIES},
            rent::{self, Rent},
        },
    },
};

const SEED: &str = "bench test";
const ACCOUNT_BALANCE: u64 = u64::MAX / 4;

#[derive(Default)]
struct TestSetup {
    owner_address: Pubkey,
    base_address: Pubkey,
    funding_address: Pubkey,
    derived_address: Pubkey,
    transaction_accounts: Vec<(Pubkey, AccountSharedData)>,

    instruction_accounts: Vec<AccountMeta>,
    instruction_data: Vec<u8>,
}

impl TestSetup {
    fn new() -> Self {
        let owner_address = system_program::id();
        let base_address = Pubkey::new_unique();
        let funding_address = Pubkey::new_unique();
        let derived_address =
            Pubkey::create_with_seed(&base_address, SEED, &owner_address).unwrap();

        let transaction_accounts = vec![
            (
                funding_address,
                AccountSharedData::new(ACCOUNT_BALANCE, 0, &owner_address),
            ),
            (
                derived_address,
                AccountSharedData::new(0, 0, &owner_address),
            ),
            (
                base_address,
                AccountSharedData::new(ACCOUNT_BALANCE, 0, &owner_address),
            ),
        ];

        Self {
            owner_address,
            base_address,
            funding_address,
            derived_address,
            transaction_accounts,
            ..TestSetup::default()
        }
    }

    fn prep_create_account(&mut self) {
        self.instruction_accounts = vec![
            AccountMeta {
                pubkey: self.funding_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.derived_address,
                is_signer: true,
                is_writable: true,
            },
        ];

        self.instruction_data = bincode::serialize(&SystemInstruction::CreateAccount {
            lamports: 1,
            space: 2,
            owner: self.owner_address,
        })
        .unwrap();
    }

    fn prep_create_account_with_seed(&mut self) {
        self.instruction_accounts = vec![
            AccountMeta {
                pubkey: self.funding_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.derived_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.base_address,
                is_signer: true,
                is_writable: false,
            },
        ];

        self.instruction_data = bincode::serialize(&SystemInstruction::CreateAccountWithSeed {
            base: self.base_address,
            seed: SEED.to_string(),
            lamports: 1,
            space: 2,
            owner: self.owner_address,
        })
        .unwrap();
    }

    fn prep_allocate(&mut self) {
        self.instruction_accounts = vec![AccountMeta {
            pubkey: self.funding_address,
            is_signer: true,
            is_writable: true,
        }];

        self.instruction_data =
            bincode::serialize(&SystemInstruction::Allocate { space: 2 }).unwrap();
    }

    fn prep_allocate_with_seed(&mut self) {
        self.instruction_accounts = vec![
            AccountMeta {
                pubkey: self.derived_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.base_address,
                is_signer: true,
                is_writable: false,
            },
        ];

        self.instruction_data = bincode::serialize(&SystemInstruction::AllocateWithSeed {
            base: self.base_address,
            seed: SEED.to_string(),
            space: 2,
            owner: self.owner_address,
        })
        .unwrap();
    }

    fn prep_assign(&mut self) {
        self.instruction_accounts = vec![AccountMeta {
            pubkey: self.funding_address,
            is_signer: true,
            is_writable: true,
        }];

        self.instruction_data = bincode::serialize(&SystemInstruction::Assign {
            owner: self.owner_address,
        })
        .unwrap();
    }

    fn prep_assign_with_seed(&mut self) {
        self.instruction_accounts = vec![
            AccountMeta {
                pubkey: self.derived_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.base_address,
                is_signer: true,
                is_writable: false,
            },
        ];

        self.instruction_data = bincode::serialize(&SystemInstruction::AssignWithSeed {
            base: self.base_address,
            seed: SEED.to_string(),
            owner: self.owner_address,
        })
        .unwrap();
    }

    fn prep_transfer(&mut self) {
        self.instruction_accounts = vec![
            AccountMeta {
                pubkey: self.funding_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.derived_address,
                is_signer: false,
                is_writable: true,
            },
        ];

        self.instruction_data =
            bincode::serialize(&SystemInstruction::Transfer { lamports: 1 }).unwrap();
    }

    fn prep_transfer_with_seed(&mut self) {
        // fund address derived from base and seed.
        self.transaction_accounts[1].1.set_lamports(ACCOUNT_BALANCE);

        self.instruction_accounts = vec![
            AccountMeta {
                pubkey: self.derived_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: self.base_address,
                is_signer: true,
                is_writable: false,
            },
            AccountMeta {
                pubkey: self.funding_address,
                is_signer: false,
                is_writable: true,
            },
        ];

        self.instruction_data = bincode::serialize(&SystemInstruction::TransferWithSeed {
            lamports: 1,
            from_seed: SEED.to_string(),
            from_owner: self.owner_address,
        })
        .unwrap();
    }

    #[allow(deprecated)]
    fn prep_initialize_nonce_account(&mut self) {
        let nonce_address = Pubkey::new_unique();
        let nonce_account = AccountSharedData::new_data_with_space(
            ACCOUNT_BALANCE,
            &Versions::new(State::Uninitialized),
            State::size(),
            &self.owner_address,
        )
        .unwrap();

        let blockhash_id = recent_blockhashes::id();
        let rent_id = rent::id();

        self.transaction_accounts = vec![
            (nonce_address, nonce_account),
            (
                blockhash_id,
                account::create_account_shared_data_for_test(
                    // create a populated RecentBlockhashes sysvar account
                    &RecentBlockhashes::from_iter(vec![
                        IterItem(0u64, &Hash::default(), 0);
                        MAX_ENTRIES
                    ]),
                ),
            ),
            (
                rent_id,
                account::create_account_shared_data_for_test(&Rent::free()),
            ),
        ];

        self.instruction_accounts = vec![
            AccountMeta {
                pubkey: nonce_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: blockhash_id,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: rent_id,
                is_signer: false,
                is_writable: false,
            },
        ];

        self.instruction_data =
            bincode::serialize(&SystemInstruction::InitializeNonceAccount(nonce_address)).unwrap();
    }

    #[allow(deprecated)]
    fn prep_authorize_nonce_account(&mut self) {
        let nonce_address = Pubkey::new_unique();
        let nonce_account = AccountSharedData::new_data_with_space(
            ACCOUNT_BALANCE,
            &Versions::new(State::new_initialized(
                &nonce_address,
                DurableNonce::default(),
                0,
            )),
            State::size(),
            &self.owner_address,
        )
        .unwrap();

        self.transaction_accounts = vec![(nonce_address, nonce_account)];

        self.instruction_accounts = vec![AccountMeta {
            pubkey: nonce_address,
            is_signer: true,
            is_writable: true,
        }];

        self.instruction_data =
            bincode::serialize(&SystemInstruction::AuthorizeNonceAccount(nonce_address)).unwrap();
    }

    #[allow(deprecated)]
    fn prep_advance_nonce_account(&mut self) {
        let nonce_address = Pubkey::new_unique();
        let nonce_account = AccountSharedData::new_data_with_space(
            ACCOUNT_BALANCE,
            &Versions::new(State::new_initialized(
                &nonce_address,
                DurableNonce::default(),
                0,
            )),
            State::size(),
            &self.owner_address,
        )
        .unwrap();

        let blockhash_id = recent_blockhashes::id();

        self.transaction_accounts = vec![
            (nonce_address, nonce_account),
            (
                blockhash_id,
                account::create_account_shared_data_for_test(
                    // create a populated RecentBlockhashes sysvar account
                    &RecentBlockhashes::from_iter(vec![
                        IterItem(0u64, &Hash::default(), 0);
                        MAX_ENTRIES
                    ]),
                ),
            ),
        ];

        self.instruction_accounts = vec![
            AccountMeta {
                pubkey: nonce_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: blockhash_id,
                is_signer: false,
                is_writable: false,
            },
        ];

        self.instruction_data =
            bincode::serialize(&SystemInstruction::AdvanceNonceAccount).unwrap();
    }

    #[allow(deprecated)]
    fn prep_upgrade_nonce_account(&mut self) {
        let nonce_address = Pubkey::new_unique();
        let nonce_account = AccountSharedData::new_data(
            ACCOUNT_BALANCE,
            &Versions::Legacy(Box::new(State::new_initialized(
                &nonce_address,
                DurableNonce::default(),
                0,
            ))),
            &self.owner_address,
        )
        .unwrap();

        self.transaction_accounts = vec![(nonce_address, nonce_account)];

        self.instruction_accounts = vec![AccountMeta {
            pubkey: nonce_address,
            is_signer: true,
            is_writable: true,
        }];

        self.instruction_data =
            bincode::serialize(&SystemInstruction::UpgradeNonceAccount).unwrap();
    }

    #[allow(deprecated)]
    fn prep_withdraw_nonce_account(&mut self) {
        let nonce_address = Pubkey::new_unique();
        let nonce_account = AccountSharedData::new_data(
            ACCOUNT_BALANCE,
            &Versions::Legacy(Box::new(State::new_initialized(
                &nonce_address,
                DurableNonce::default(),
                0,
            ))),
            &self.owner_address,
        )
        .unwrap();

        let recipient_address = Pubkey::new_unique();
        let blockhash_id = recent_blockhashes::id();
        let rent_id = rent::id();

        self.transaction_accounts = vec![
            (nonce_address, nonce_account),
            (
                recipient_address,
                AccountSharedData::new(0, 0, &recipient_address),
            ),
            (
                blockhash_id,
                account::create_account_shared_data_for_test(
                    // create a populated RecentBlockhashes sysvar account
                    &RecentBlockhashes::from_iter(vec![
                        IterItem(0u64, &Hash::default(), 0);
                        MAX_ENTRIES
                    ]),
                ),
            ),
            (
                rent_id,
                account::create_account_shared_data_for_test(&Rent::free()),
            ),
        ];

        self.instruction_accounts = vec![
            AccountMeta {
                pubkey: nonce_address,
                is_signer: true,
                is_writable: true,
            },
            AccountMeta {
                pubkey: recipient_address,
                is_signer: false,
                is_writable: true,
            },
            AccountMeta {
                pubkey: blockhash_id,
                is_signer: false,
                is_writable: false,
            },
            AccountMeta {
                pubkey: rent_id,
                is_signer: false,
                is_writable: false,
            },
        ];

        self.instruction_data =
            bincode::serialize(&SystemInstruction::WithdrawNonceAccount(1)).unwrap();
    }

    fn run(&self) {
        mock_process_instruction(
            &solana_system_program::id(),
            Vec::new(),
            &self.instruction_data,
            self.transaction_accounts.clone(),
            self.instruction_accounts.clone(),
            Ok(()), //expected_result,
            solana_system_program::system_processor::Entrypoint::vm,
            |_invoke_context| {},
            |_invoke_context| {},
        );
    }
}

fn bench_create_account(c: &mut Criterion) {
    let mut test_setup = TestSetup::new();
    test_setup.prep_create_account();

    c.bench_function("create_account", |bencher| {
        bencher.iter(|| test_setup.run())
    });
}

fn bench_create_account_with_seed(c: &mut Criterion) {
    let mut test_setup = TestSetup::new();
    test_setup.prep_create_account_with_seed();

    c.bench_function("create_account_with_seed", |bencher| {
        bencher.iter(|| test_setup.run())
    });
}

fn bench_allocate(c: &mut Criterion) {
    let mut test_setup = TestSetup::new();
    test_setup.prep_allocate();

    c.bench_function("allocate", |bencher| bencher.iter(|| test_setup.run()));
}

fn bench_allocate_with_seed(c: &mut Criterion) {
    let mut test_setup = TestSetup::new();
    test_setup.prep_allocate_with_seed();

    c.bench_function("allocate_with_seed", |bencher| {
        bencher.iter(|| test_setup.run())
    });
}

fn bench_assign(c: &mut Criterion) {
    let mut test_setup = TestSetup::new();
    test_setup.prep_assign();

    c.bench_function("assign", |bencher| bencher.iter(|| test_setup.run()));
}

fn bench_assign_with_seed(c: &mut Criterion) {
    let mut test_setup = TestSetup::new();
    test_setup.prep_assign_with_seed();

    c.bench_function("assign_with_seed", |bencher| {
        bencher.iter(|| test_setup.run())
    });
}

fn bench_transfer(c: &mut Criterion) {
    let mut test_setup = TestSetup::new();
    test_setup.prep_transfer();

    c.bench_function("transfer", |bencher| bencher.iter(|| test_setup.run()));
}

fn bench_transfer_with_seed(c: &mut Criterion) {
    let mut test_setup = TestSetup::new();
    test_setup.prep_transfer_with_seed();

    c.bench_function("transfer_with_seed", |bencher| {
        bencher.iter(|| test_setup.run())
    });
}

fn bench_initialize_nonce_account(c: &mut Criterion) {
    let mut test_setup = TestSetup::new();
    test_setup.prep_initialize_nonce_account();

    c.bench_function("initialize_nonce_account", |bencher| {
        bencher.iter(|| test_setup.run())
    });
}

fn bench_authorize_nonce_account(c: &mut Criterion) {
    let mut test_setup = TestSetup::new();
    test_setup.prep_authorize_nonce_account();

    c.bench_function("authorize_nonce_account", |bencher| {
        bencher.iter(|| test_setup.run())
    });
}

fn bench_advance_nonce_account(c: &mut Criterion) {
    let mut test_setup = TestSetup::new();
    test_setup.prep_advance_nonce_account();

    c.bench_function("advance_nonce_account", |bencher| {
        bencher.iter(|| test_setup.run())
    });
}

fn bench_upgrade_nonce_account(c: &mut Criterion) {
    let mut test_setup = TestSetup::new();
    test_setup.prep_upgrade_nonce_account();

    c.bench_function("upgrade_nonce_account", |bencher| {
        bencher.iter(|| test_setup.run())
    });
}

fn bench_withdraw_nonce_account(c: &mut Criterion) {
    let mut test_setup = TestSetup::new();
    test_setup.prep_withdraw_nonce_account();

    c.bench_function("withdraw_nonce_account", |bencher| {
        bencher.iter(|| test_setup.run())
    });
}

criterion_group!(
    benches,
    bench_create_account,
    bench_create_account_with_seed,
    bench_allocate,
    bench_allocate_with_seed,
    bench_assign,
    bench_assign_with_seed,
    bench_transfer,
    bench_transfer_with_seed,
    bench_initialize_nonce_account,
    bench_authorize_nonce_account,
    bench_advance_nonce_account,
    bench_upgrade_nonce_account,
    bench_withdraw_nonce_account,
);
criterion_main!(benches);
