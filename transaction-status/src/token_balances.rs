use {
    crate::TransactionTokenBalance,
    solana_account_decoder::parse_token::{
        is_known_spl_token_id, pubkey_from_spl_token, spl_token_native_mint,
        token_amount_to_ui_amount, UiTokenAmount,
    },
    solana_measure::measure::Measure,
    solana_metrics::datapoint_debug,
    solana_runtime::{bank::Bank, transaction_batch::TransactionBatch},
    solana_sdk::{account::ReadableAccount, pubkey::Pubkey},
    spl_token::{
        solana_program::program_pack::Pack,
        state::{Account as TokenAccount, Mint},
    },
    std::collections::HashMap,
};

pub type TransactionTokenBalances = Vec<Vec<TransactionTokenBalance>>;

pub struct TransactionTokenBalancesSet {
    pub pre_token_balances: TransactionTokenBalances,
    pub post_token_balances: TransactionTokenBalances,
}

impl TransactionTokenBalancesSet {
    pub fn new(
        pre_token_balances: TransactionTokenBalances,
        post_token_balances: TransactionTokenBalances,
    ) -> Self {
        assert_eq!(pre_token_balances.len(), post_token_balances.len());
        Self {
            pre_token_balances,
            post_token_balances,
        }
    }
}

fn get_mint_decimals(bank: &Bank, mint: &Pubkey) -> Option<u8> {
    if mint == &spl_token_native_mint() {
        Some(spl_token::native_mint::DECIMALS)
    } else {
        let mint_account = bank.get_account(mint)?;

        if !is_known_spl_token_id(mint_account.owner()) {
            return None;
        }

        let decimals = Mint::unpack(mint_account.data())
            .map(|mint| mint.decimals)
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

    let token_account = TokenAccount::unpack(account.data()).ok()?;
    let mint = pubkey_from_spl_token(&token_account.mint);

    let decimals = mint_decimals.get(&mint).cloned().or_else(|| {
        let decimals = get_mint_decimals(bank, &mint)?;
        mint_decimals.insert(mint, decimals);
        Some(decimals)
    })?;

    Some(TokenBalanceData {
        mint: token_account.mint.to_string(),
        owner: token_account.owner.to_string(),
        ui_token_amount: token_amount_to_ui_amount(token_account.amount, decimals),
        program_id: account.owner().to_string(),
    })
}

#[cfg(test)]
mod test {
    use {
        super::*,
        solana_account_decoder::parse_token::{pubkey_from_spl_token, spl_token_pubkey},
        solana_sdk::{account::Account, genesis_config::create_genesis_config},
        spl_token::solana_program::program_option::COption,
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
            owner: pubkey_from_spl_token(&spl_token::id()),
            executable: false,
            rent_epoch: 0,
        };
        let other_mint_pubkey = Pubkey::new_unique();
        let other_mint = Account {
            lamports: 100,
            data: data.to_vec(),
            owner: Pubkey::new_unique(),
            executable: false,
            rent_epoch: 0,
        };

        let token_owner = Pubkey::new_unique();
        let token_data = TokenAccount {
            mint: spl_token_pubkey(&mint_pubkey),
            owner: spl_token_pubkey(&token_owner),
            amount: 42,
            delegate: COption::None,
            state: spl_token::state::AccountState::Initialized,
            is_native: COption::Some(100),
            delegated_amount: 0,
            close_authority: COption::None,
        };
        let mut data = [0; TokenAccount::LEN];
        TokenAccount::pack(token_data, &mut data).unwrap();

        let spl_token_account = Account {
            lamports: 100,
            data: data.to_vec(),
            owner: pubkey_from_spl_token(&spl_token::id()),
            executable: false,
            rent_epoch: 0,
        };
        let other_account = Account {
            lamports: 100,
            data: data.to_vec(),
            owner: Pubkey::new_unique(),
            executable: false,
            rent_epoch: 0,
        };

        let other_mint_data = TokenAccount {
            mint: spl_token_pubkey(&other_mint_pubkey),
            owner: spl_token_pubkey(&token_owner),
            amount: 42,
            delegate: COption::None,
            state: spl_token::state::AccountState::Initialized,
            is_native: COption::Some(100),
            delegated_amount: 0,
            close_authority: COption::None,
        };
        let mut data = [0; TokenAccount::LEN];
        TokenAccount::pack(other_mint_data, &mut data).unwrap();

        let other_mint_token_account = Account {
            lamports: 100,
            data: data.to_vec(),
            owner: pubkey_from_spl_token(&spl_token::id()),
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

        assert_eq!(
            collect_token_balance_from_account(&bank, &account_pubkey, &mut mint_decimals),
            None
        );

        assert_eq!(
            collect_token_balance_from_account(&bank, &mint_pubkey, &mut mint_decimals),
            None
        );

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

        assert_eq!(
            collect_token_balance_from_account(&bank, &other_account_pubkey, &mut mint_decimals),
            None
        );

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
