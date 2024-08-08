#![allow(unused)]

use {
    solana_sdk::{
        account::{Account, AccountSharedData, ReadableAccount},
        epoch_schedule::EpochSchedule,
        program_pack::Pack,
        pubkey::Pubkey,
        signature::Keypair,
        system_program,
    },
    solana_test_validator::{TestValidator, TestValidatorGenesis},
    spl_token::state::{Account as TokenAccount, Mint},
};

const SLOTS_PER_EPOCH: u64 = 50;

pub struct TestValidatorContext {
    pub test_validator: TestValidator,
    pub payer: Keypair,
}

impl TestValidatorContext {
    pub fn start_with_accounts(accounts: Vec<(Pubkey, AccountSharedData)>) -> Self {
        let epoch_schedule = EpochSchedule::custom(SLOTS_PER_EPOCH, SLOTS_PER_EPOCH, false);

        let (test_validator, payer) = TestValidatorGenesis::default()
            .epoch_schedule(epoch_schedule)
            .add_accounts(accounts)
            .start();

        Self {
            test_validator,
            payer,
        }
    }
}

pub fn get_token_account_balance(token_account: Account) -> u64 {
    let state = TokenAccount::unpack(token_account.data()).unwrap();
    state.amount
}

pub fn mint_account() -> AccountSharedData {
    let data = {
        let mut data = [0; Mint::LEN];
        Mint::pack(
            Mint {
                supply: 100_000_000,
                decimals: 0,
                is_initialized: true,
                ..Default::default()
            },
            &mut data,
        )
        .unwrap();
        data
    };
    let mut account = AccountSharedData::new(100_000_000, data.len(), &spl_token::id());
    account.set_data_from_slice(&data);
    account
}

pub fn system_account(lamports: u64) -> AccountSharedData {
    AccountSharedData::new(lamports, 0, &system_program::id())
}

pub fn token_account(owner: &Pubkey, mint: &Pubkey, amount: u64) -> AccountSharedData {
    let data = {
        let mut data = [0; TokenAccount::LEN];
        TokenAccount::pack(
            TokenAccount {
                mint: *mint,
                owner: *owner,
                amount,
                state: spl_token::state::AccountState::Initialized,
                ..Default::default()
            },
            &mut data,
        )
        .unwrap();
        data
    };
    let mut account = AccountSharedData::new(100_000_000, data.len(), &spl_token::id());
    account.set_data_from_slice(&data);
    account
}
