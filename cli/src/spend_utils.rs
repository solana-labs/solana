use crate::cli::{calculate_fee, check_account_for_balance, CliError, TransferAmount};
use solana_client::rpc_client::RpcClient;
use solana_sdk::{
    fee_calculator::FeeCalculator, message::Message, native_token::lamports_to_sol, pubkey::Pubkey,
    transaction::Transaction,
};
use std::error;

struct SpendAndFee {
    spend: u64,
    fee: u64,
}

pub fn resolve_spend_tx_and_check_account_balance<F, G>(
    rpc_client: &RpcClient,
    sign_only: bool,
    amount: TransferAmount,
    fee_calculator: &FeeCalculator,
    from_pubkey: &Pubkey,
    build_message: F,
    additional_online_checks: G,
) -> Result<Transaction, Box<dyn error::Error>>
where
    F: Fn(u64) -> Message,
    G: Fn(u64) -> Result<(), Box<dyn error::Error>>,
{
    resolve_spend_tx_and_check_account_balances(
        rpc_client,
        sign_only,
        amount,
        fee_calculator,
        from_pubkey,
        from_pubkey,
        build_message,
        additional_online_checks,
    )
}

pub fn resolve_spend_tx_and_check_account_balances<F, G>(
    rpc_client: &RpcClient,
    sign_only: bool,
    amount: TransferAmount,
    fee_calculator: &FeeCalculator,
    from_pubkey: &Pubkey,
    fee_pubkey: &Pubkey,
    build_message: F,
    additional_online_checks: G,
) -> Result<Transaction, Box<dyn error::Error>>
where
    F: Fn(u64) -> Message,
    G: Fn(u64) -> Result<(), Box<dyn error::Error>>,
{
    let message = if sign_only {
        let (message, _) = resolve_spend_message(
            amount,
            fee_calculator,
            0,
            from_pubkey,
            fee_pubkey,
            build_message,
        );
        message
    } else {
        let from_balance = rpc_client
            .retry_get_balance(&from_pubkey, 5)?
            .unwrap_or_default();
        let (message, SpendAndFee { spend, fee }) = resolve_spend_message(
            amount,
            fee_calculator,
            from_balance,
            from_pubkey,
            fee_pubkey,
            build_message,
        );
        if from_pubkey == fee_pubkey {
            if from_balance == 0 || from_balance < spend + fee {
                return Err(CliError::InsufficientFundsForSpendAndFee(
                    lamports_to_sol(spend),
                    lamports_to_sol(fee),
                )
                .into());
            }
        } else {
            if from_balance < spend {
                return Err(CliError::InsufficientFundsForSpend(lamports_to_sol(spend)).into());
            }
            if !check_account_for_balance(rpc_client, fee_pubkey, fee)? {
                return Err(CliError::InsufficientFundsForFee(lamports_to_sol(fee)).into());
            }
        }
        additional_online_checks(spend)?;
        message
    };
    Ok(Transaction::new_unsigned(message))
}

fn resolve_spend_message<F>(
    amount: TransferAmount,
    fee_calculator: &FeeCalculator,
    from_balance: u64,
    from_pubkey: &Pubkey,
    fee_pubkey: &Pubkey,
    build_message: F,
) -> (Message, SpendAndFee)
where
    F: Fn(u64) -> Message,
{
    match amount {
        TransferAmount::Some(lamports) => {
            let message = build_message(lamports);
            let fee = calculate_fee(fee_calculator, &[&message]);
            (
                message,
                SpendAndFee {
                    spend: lamports,
                    fee,
                },
            )
        }
        TransferAmount::All => {
            let dummy_message = build_message(0);
            let fee = calculate_fee(fee_calculator, &[&dummy_message]);
            let lamports = if from_pubkey == fee_pubkey {
                from_balance.saturating_sub(fee)
            } else {
                from_balance
            };
            (
                build_message(lamports),
                SpendAndFee {
                    spend: lamports,
                    fee,
                },
            )
        }
    }
}
