use crate::TransactionTokenBalance;
use solana_account_decoder::parse_token::{
    spl_token_id_v2_0, spl_token_v2_0_native_mint, token_amount_to_ui_amount, UiTokenAmount,
};
use solana_runtime::{
    bank::Bank, transaction_batch::TransactionBatch, transaction_utils::OrderedIterator,
};
use solana_sdk::{account::ReadableAccount, pubkey::Pubkey};
use spl_token_v2_0::{
    solana_program::program_pack::Pack,
    state::{Account as TokenAccount, Mint},
};
use std::{collections::HashMap, str::FromStr};

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

        let decimals = Mint::unpack(&mint_account.data())
            .map(|mint| mint.decimals)
            .ok()?;

        Some(decimals)
    }
}

pub fn collect_token_balances(
    bank: &Bank,
    batch: &TransactionBatch,
    mut mint_decimals: &mut HashMap<Pubkey, u8>,
) -> TransactionTokenBalances {
    let mut balances: TransactionTokenBalances = vec![];

    for (_, transaction) in OrderedIterator::new(batch.transactions(), batch.iteration_order()) {
        let account_keys = &transaction.message.account_keys;
        let mut fetch_account_hash: HashMap<u8, bool> = HashMap::new();
        for instruction in transaction.message.instructions.iter() {
            if let Some(program_id) = account_keys.get(instruction.program_id_index as usize) {
                if is_token_program(&program_id) {
                    for account in &instruction.accounts {
                        fetch_account_hash.insert(*account, true);
                    }
                }
            }
        }

        let mut transaction_balances: Vec<TransactionTokenBalance> = vec![];
        for index in fetch_account_hash.keys() {
            if let Some(account_id) = account_keys.get(*index as usize) {
                if let Some((mint, ui_token_amount)) =
                    collect_token_balance_from_account(&bank, account_id, &mut mint_decimals)
                {
                    transaction_balances.push(TransactionTokenBalance {
                        account_index: *index,
                        mint,
                        ui_token_amount,
                    });
                }
            }
        }
        balances.push(transaction_balances);
    }
    balances
}

pub fn collect_token_balance_from_account(
    bank: &Bank,
    account_id: &Pubkey,
    mint_decimals: &mut HashMap<Pubkey, u8>,
) -> Option<(String, UiTokenAmount)> {
    let account = bank.get_account(account_id)?;

    let token_account = TokenAccount::unpack(&account.data()).ok()?;
    let mint_string = &token_account.mint.to_string();
    let mint = &Pubkey::from_str(&mint_string).unwrap_or_default();

    let decimals = mint_decimals.get(&mint).cloned().or_else(|| {
        let decimals = get_mint_decimals(bank, &mint)?;
        mint_decimals.insert(*mint, decimals);
        Some(decimals)
    })?;

    Some((
        mint_string.to_string(),
        token_amount_to_ui_amount(token_account.amount, decimals),
    ))
}
