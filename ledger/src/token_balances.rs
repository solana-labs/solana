use {
    solana_account_decoder::parse_token::{
        is_known_spl_token_id, token_amount_to_ui_amount, UiTokenAmount,
    },
    solana_measure::measure::Measure,
    solana_metrics::datapoint_debug,
    solana_runtime::{bank::Bank, transaction_batch::TransactionBatch},
    solana_sdk::{account::ReadableAccount, pubkey::Pubkey},
    solana_transaction_status::{
        token_balances::TransactionTokenBalances, TransactionTokenBalance,
    },
    spl_token_2022::{
        extension::StateWithExtensions,
        state::{Account as TokenAccount, Mint},
    },
    std::collections::HashMap,
};

fn get_mint_decimals(bank: &Bank, mint: &Pubkey) -> Option<u8> {
    if mint == &spl_token::native_mint::id() {
        Some(spl_token::native_mint::DECIMALS)
    } else {
        let mint_account = bank.get_account(mint)?;

        if !is_known_spl_token_id(mint_account.owner()) {
            return None;
        }

        let decimals = StateWithExtensions::<Mint>::unpack(mint_account.data())
            .map(|mint| mint.base.decimals)
            .ok()?;

        Some(decimals)
    }
}

pub fn collect_token_balances(
    bank: &Bank,
    batch: &TransactionBatch,
    mint_decimals: &mut HashMap<Pubkey, u8>,
) -> TransactionTokenBalances {
    let mut balances: TransactionTokenBalances = vec![];
    let mut collect_time = Measure::start("collect_token_balances");

    for transaction in batch.sanitized_transactions() {
        let account_keys = transaction.message().account_keys();
        let has_token_program = account_keys.iter().any(is_known_spl_token_id);

        let mut transaction_balances: Vec<TransactionTokenBalance> = vec![];
        if has_token_program {
            for (index, account_id) in account_keys.iter().enumerate() {
                if transaction.message().is_invoked(index) || is_known_spl_token_id(account_id) {
                    continue;
                }

                if let Some(TokenBalanceData {
                    mint,
                    ui_token_amount,
                    owner,
                    program_id,
                }) = collect_token_balance_from_account(bank, account_id, mint_decimals)
                {
                    transaction_balances.push(TransactionTokenBalance {
                        account_index: index as u8,
                        mint,
                        ui_token_amount,
                        owner,
                        program_id,
                    });
                }
            }
        }
        balances.push(transaction_balances);
    }
    collect_time.stop();
    datapoint_debug!(
        "collect_token_balances",
        ("collect_time_us", collect_time.as_us(), i64),
    );
    balances
}

#[derive(Debug, PartialEq)]
struct TokenBalanceData {
    mint: String,
    owner: String,
    ui_token_amount: UiTokenAmount,
    program_id: String,
}

fn collect_token_balance_from_account(
    bank: &Bank,
    account_id: &Pubkey,
    mint_decimals: &mut HashMap<Pubkey, u8>,
) -> Option<TokenBalanceData> {
    let account = bank.get_account(account_id)?;

    if !is_known_spl_token_id(account.owner()) {
        return None;
    }

    let token_account = StateWithExtensions::<TokenAccount>::unpack(account.data()).ok()?;
    let mint = token_account.base.mint;

    let decimals = mint_decimals.get(&mint).cloned().or_else(|| {
        let decimals = get_mint_decimals(bank, &mint)?;
        mint_decimals.insert(mint, decimals);
        Some(decimals)
    })?;

    Some(TokenBalanceData {
        mint: token_account.base.mint.to_string(),
        owner: token_account.base.owner.to_string(),
        ui_token_amount: token_amount_to_ui_amount(token_account.base.amount, decimals),
        program_id: account.owner().to_string(),
    })
}

#[cfg(test)]
mod test {
    use {
        super::*,
        solana_sdk::{account::Account, genesis_config::create_genesis_config},
        spl_pod::optional_keys::OptionalNonZeroPubkey,
        spl_token_2022::{
            extension::{
                immutable_owner::ImmutableOwner, memo_transfer::MemoTransfer,
                mint_close_authority::MintCloseAuthority, ExtensionType, StateWithExtensionsMut,
            },
            solana_program::{program_option::COption, program_pack::Pack},
        },
        std::collections::BTreeMap,
    };

    #[test]
    fn test_collect_token_balance_from_account() {
        let (mut genesis_config, _mint_keypair) = create_genesis_config(500);

        // Add a variety of accounts, token and not
        let account = Account::new(42, 55, &Pubkey::new_unique());

        let mint_data = Mint {
            mint_authority: COption::None,
            supply: 4242,
            decimals: 2,
            is_initialized: true,
            freeze_authority: COption::None,
        };
        let mut data = [0; Mint::LEN];
        Mint::pack(mint_data, &mut data).unwrap();
        let mint_pubkey = Pubkey::new_unique();
        let mint = Account {
            lamports: 100,
            data: data.to_vec(),
            owner: spl_token::id(),
            executable: false,
            rent_epoch: 0,
        };
        let other_mint_pubkey = Pubkey::new_unique();
        let other_mint = Account {
            lamports: 100,
            data: data.to_vec(),
            owner: Pubkey::new_unique(), // !is_known_spl_token_id
            executable: false,
            rent_epoch: 0,
        };

        let token_owner = Pubkey::new_unique();
        let token_data = TokenAccount {
            mint: mint_pubkey,
            owner: token_owner,
            amount: 42,
            delegate: COption::None,
            state: spl_token_2022::state::AccountState::Initialized,
            is_native: COption::Some(100),
            delegated_amount: 0,
            close_authority: COption::None,
        };
        let mut data = [0; TokenAccount::LEN];
        TokenAccount::pack(token_data, &mut data).unwrap();

        let spl_token_account = Account {
            lamports: 100,
            data: data.to_vec(),
            owner: spl_token::id(),
            executable: false,
            rent_epoch: 0,
        };
        let other_account = Account {
            lamports: 100,
            data: data.to_vec(),
            owner: Pubkey::new_unique(), // !is_known_spl_token_id
            executable: false,
            rent_epoch: 0,
        };

        let other_mint_data = TokenAccount {
            mint: other_mint_pubkey,
            owner: token_owner,
            amount: 42,
            delegate: COption::None,
            state: spl_token_2022::state::AccountState::Initialized,
            is_native: COption::Some(100),
            delegated_amount: 0,
            close_authority: COption::None,
        };
        let mut data = [0; TokenAccount::LEN];
        TokenAccount::pack(other_mint_data, &mut data).unwrap();

        let other_mint_token_account = Account {
            lamports: 100,
            data: data.to_vec(),
            owner: spl_token::id(),
            executable: false,
            rent_epoch: 0,
        };

        let mut accounts = BTreeMap::new();

        let account_pubkey = Pubkey::new_unique();
        accounts.insert(account_pubkey, account);
        accounts.insert(mint_pubkey, mint);
        accounts.insert(other_mint_pubkey, other_mint);
        let spl_token_account_pubkey = Pubkey::new_unique();
        accounts.insert(spl_token_account_pubkey, spl_token_account);
        let other_account_pubkey = Pubkey::new_unique();
        accounts.insert(other_account_pubkey, other_account);
        let other_mint_account_pubkey = Pubkey::new_unique();
        accounts.insert(other_mint_account_pubkey, other_mint_token_account);

        genesis_config.accounts = accounts;

        let bank = Bank::new_for_tests(&genesis_config);
        let mut mint_decimals = HashMap::new();

        // Account is not owned by spl_token (nor does it have TokenAccount state)
        assert_eq!(
            collect_token_balance_from_account(&bank, &account_pubkey, &mut mint_decimals),
            None
        );

        // Mint does not have TokenAccount state
        assert_eq!(
            collect_token_balance_from_account(&bank, &mint_pubkey, &mut mint_decimals),
            None
        );

        // TokenAccount owned by spl_token::id() works
        assert_eq!(
            collect_token_balance_from_account(
                &bank,
                &spl_token_account_pubkey,
                &mut mint_decimals
            ),
            Some(TokenBalanceData {
                mint: mint_pubkey.to_string(),
                owner: token_owner.to_string(),
                ui_token_amount: UiTokenAmount {
                    ui_amount: Some(0.42),
                    decimals: 2,
                    amount: "42".to_string(),
                    ui_amount_string: "0.42".to_string(),
                },
                program_id: spl_token::id().to_string(),
            })
        );

        // TokenAccount is not owned by known spl-token program_id
        assert_eq!(
            collect_token_balance_from_account(&bank, &other_account_pubkey, &mut mint_decimals),
            None
        );

        // TokenAccount's mint is not owned by known spl-token program_id
        assert_eq!(
            collect_token_balance_from_account(
                &bank,
                &other_mint_account_pubkey,
                &mut mint_decimals
            ),
            None
        );
    }

    #[test]
    fn test_collect_token_balance_from_spl_token_2022_account() {
        let (mut genesis_config, _mint_keypair) = create_genesis_config(500);

        // Add a variety of accounts, token and not
        let account = Account::new(42, 55, &Pubkey::new_unique());

        let mint_authority = Pubkey::new_unique();
        let mint_size =
            ExtensionType::try_calculate_account_len::<Mint>(&[ExtensionType::MintCloseAuthority])
                .unwrap();
        let mint_base = Mint {
            mint_authority: COption::None,
            supply: 4242,
            decimals: 2,
            is_initialized: true,
            freeze_authority: COption::None,
        };
        let mut mint_data = vec![0; mint_size];
        let mut mint_state =
            StateWithExtensionsMut::<Mint>::unpack_uninitialized(&mut mint_data).unwrap();
        mint_state.base = mint_base;
        mint_state.pack_base();
        mint_state.init_account_type().unwrap();
        let mint_close_authority = mint_state
            .init_extension::<MintCloseAuthority>(true)
            .unwrap();
        mint_close_authority.close_authority =
            OptionalNonZeroPubkey::try_from(Some(mint_authority)).unwrap();

        let mint_pubkey = Pubkey::new_unique();
        let mint = Account {
            lamports: 100,
            data: mint_data.to_vec(),
            owner: spl_token_2022::id(),
            executable: false,
            rent_epoch: 0,
        };
        let other_mint_pubkey = Pubkey::new_unique();
        let other_mint = Account {
            lamports: 100,
            data: mint_data.to_vec(),
            owner: Pubkey::new_unique(),
            executable: false,
            rent_epoch: 0,
        };

        let token_owner = Pubkey::new_unique();
        let token_base = TokenAccount {
            mint: mint_pubkey,
            owner: token_owner,
            amount: 42,
            delegate: COption::None,
            state: spl_token_2022::state::AccountState::Initialized,
            is_native: COption::Some(100),
            delegated_amount: 0,
            close_authority: COption::None,
        };
        let account_size = ExtensionType::try_calculate_account_len::<TokenAccount>(&[
            ExtensionType::ImmutableOwner,
            ExtensionType::MemoTransfer,
        ])
        .unwrap();
        let mut account_data = vec![0; account_size];
        let mut account_state =
            StateWithExtensionsMut::<TokenAccount>::unpack_uninitialized(&mut account_data)
                .unwrap();
        account_state.base = token_base;
        account_state.pack_base();
        account_state.init_account_type().unwrap();
        account_state
            .init_extension::<ImmutableOwner>(true)
            .unwrap();
        let memo_transfer = account_state.init_extension::<MemoTransfer>(true).unwrap();
        memo_transfer.require_incoming_transfer_memos = true.into();

        let spl_token_account = Account {
            lamports: 100,
            data: account_data.to_vec(),
            owner: spl_token_2022::id(),
            executable: false,
            rent_epoch: 0,
        };
        let other_account = Account {
            lamports: 100,
            data: account_data.to_vec(),
            owner: Pubkey::new_unique(),
            executable: false,
            rent_epoch: 0,
        };

        let other_mint_token_base = TokenAccount {
            mint: other_mint_pubkey,
            owner: token_owner,
            amount: 42,
            delegate: COption::None,
            state: spl_token_2022::state::AccountState::Initialized,
            is_native: COption::Some(100),
            delegated_amount: 0,
            close_authority: COption::None,
        };
        let account_size = ExtensionType::try_calculate_account_len::<TokenAccount>(&[
            ExtensionType::ImmutableOwner,
            ExtensionType::MemoTransfer,
        ])
        .unwrap();
        let mut account_data = vec![0; account_size];
        let mut account_state =
            StateWithExtensionsMut::<TokenAccount>::unpack_uninitialized(&mut account_data)
                .unwrap();
        account_state.base = other_mint_token_base;
        account_state.pack_base();
        account_state.init_account_type().unwrap();
        account_state
            .init_extension::<ImmutableOwner>(true)
            .unwrap();
        let memo_transfer = account_state.init_extension::<MemoTransfer>(true).unwrap();
        memo_transfer.require_incoming_transfer_memos = true.into();

        let other_mint_token_account = Account {
            lamports: 100,
            data: account_data.to_vec(),
            owner: spl_token_2022::id(),
            executable: false,
            rent_epoch: 0,
        };

        let mut accounts = BTreeMap::new();

        let account_pubkey = Pubkey::new_unique();
        accounts.insert(account_pubkey, account);
        accounts.insert(mint_pubkey, mint);
        accounts.insert(other_mint_pubkey, other_mint);
        let spl_token_account_pubkey = Pubkey::new_unique();
        accounts.insert(spl_token_account_pubkey, spl_token_account);
        let other_account_pubkey = Pubkey::new_unique();
        accounts.insert(other_account_pubkey, other_account);
        let other_mint_account_pubkey = Pubkey::new_unique();
        accounts.insert(other_mint_account_pubkey, other_mint_token_account);

        genesis_config.accounts = accounts;

        let bank = Bank::new_for_tests(&genesis_config);
        let mut mint_decimals = HashMap::new();

        // Account is not owned by spl_token (nor does it have TokenAccount state)
        assert_eq!(
            collect_token_balance_from_account(&bank, &account_pubkey, &mut mint_decimals),
            None
        );

        // Mint does not have TokenAccount state
        assert_eq!(
            collect_token_balance_from_account(&bank, &mint_pubkey, &mut mint_decimals),
            None
        );

        // TokenAccount owned by spl_token_2022::id() works
        assert_eq!(
            collect_token_balance_from_account(
                &bank,
                &spl_token_account_pubkey,
                &mut mint_decimals
            ),
            Some(TokenBalanceData {
                mint: mint_pubkey.to_string(),
                owner: token_owner.to_string(),
                ui_token_amount: UiTokenAmount {
                    ui_amount: Some(0.42),
                    decimals: 2,
                    amount: "42".to_string(),
                    ui_amount_string: "0.42".to_string(),
                },
                program_id: spl_token_2022::id().to_string(),
            })
        );

        // TokenAccount is not owned by known spl-token program_id
        assert_eq!(
            collect_token_balance_from_account(&bank, &other_account_pubkey, &mut mint_decimals),
            None
        );

        // TokenAccount's mint is not owned by known spl-token program_id
        assert_eq!(
            collect_token_balance_from_account(
                &bank,
                &other_mint_account_pubkey,
                &mut mint_decimals
            ),
            None
        );
    }
}
