use {
    crate::TransactionTokenBalance,
    solana_account_decoder::parse_token::{
        pubkey_from_spl_token_v2_0, spl_token_id_v2_0, spl_token_v2_0_native_mint,
        token_amount_to_ui_amount, UiTokenAmount,
    },
    solana_measure::measure::Measure,
    solana_metrics::datapoint_debug,
    solana_runtime::{bank::Bank, transaction_batch::TransactionBatch},
    solana_sdk::{account::ReadableAccount, pubkey::Pubkey},
    spl_token_v2_0::{
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

fn is_token_program(program_id: &Pubkey) -> bool {
    program_id == &spl_token_id_v2_0()
}

fn get_mint_decimals(bank: &Bank, mint: &Pubkey) -> Option<u8> {
    if mint == &spl_token_v2_0_native_mint() {
        Some(spl_token_v2_0::native_mint::DECIMALS)
    } else {
        let mint_account = bank.get_account(mint)?;

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
        let has_token_program = transaction
            .message()
            .account_keys_iter()
            .any(is_token_program);

        let mut transaction_balances: Vec<TransactionTokenBalance> = vec![];
        if has_token_program {
            for (index, account_id) in transaction.message().account_keys_iter().enumerate() {
                if transaction.message().is_invoked(index) || is_token_program(account_id) {
                    continue;
                }

                if let Some(TokenBalanceData {
                    mint,
                    ui_token_amount,
                    owner,
                }) = collect_token_balance_from_account(bank, account_id, mint_decimals)
                {
                    transaction_balances.push(TransactionTokenBalance {
                        account_index: index as u8,
                        mint,
                        ui_token_amount,
                        owner,
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

struct TokenBalanceData {
    mint: String,
    owner: String,
    ui_token_amount: UiTokenAmount,
}

fn collect_token_balance_from_account(
    bank: &Bank,
    account_id: &Pubkey,
    mint_decimals: &mut HashMap<Pubkey, u8>,
) -> Option<TokenBalanceData> {
    let account = bank.get_account(account_id)?;

    let token_account = TokenAccount::unpack(account.data()).ok()?;
    let mint = pubkey_from_spl_token_v2_0(&token_account.mint);

    let decimals = mint_decimals.get(&mint).cloned().or_else(|| {
        let decimals = get_mint_decimals(bank, &mint)?;
        mint_decimals.insert(mint, decimals);
        Some(decimals)
    })?;

    Some(TokenBalanceData {
        mint: token_account.mint.to_string(),
        owner: token_account.owner.to_string(),
        ui_token_amount: token_amount_to_ui_amount(token_account.amount, decimals),
    })
}
