use crate::cli::CliError;
use solana_client::{
    client_error::{ClientError, Result as ClientResult},
    rpc_client::RpcClient,
};
use solana_sdk::{
    fee_calculator::FeeCalculator, message::Message, native_token::lamports_to_sol, pubkey::Pubkey,
};

pub fn check_account_for_fee(
    rpc_client: &RpcClient,
    account_pubkey: &Pubkey,
    fee_calculator: &FeeCalculator,
    message: &Message,
) -> Result<(), CliError> {
    check_account_for_multiple_fees(rpc_client, account_pubkey, fee_calculator, &[message])
}

pub fn check_account_for_multiple_fees(
    rpc_client: &RpcClient,
    account_pubkey: &Pubkey,
    fee_calculator: &FeeCalculator,
    messages: &[&Message],
) -> Result<(), CliError> {
    let fee = calculate_fee(fee_calculator, messages);
    if !check_account_for_balance(rpc_client, account_pubkey, fee)
        .map_err(Into::<ClientError>::into)?
    {
        return Err(CliError::InsufficientFundsForFee(lamports_to_sol(fee)));
    }
    Ok(())
}

pub fn calculate_fee(fee_calculator: &FeeCalculator, messages: &[&Message]) -> u64 {
    messages
        .iter()
        .map(|message| fee_calculator.calculate_fee(message))
        .sum()
}

pub fn check_account_for_balance(
    rpc_client: &RpcClient,
    account_pubkey: &Pubkey,
    balance: u64,
) -> ClientResult<bool> {
    let lamports = rpc_client.get_balance(account_pubkey)?;
    if lamports != 0 && lamports >= balance {
        return Ok(true);
    }
    Ok(false)
}

pub fn check_unique_pubkeys(
    pubkey0: (&Pubkey, String),
    pubkey1: (&Pubkey, String),
) -> Result<(), CliError> {
    if pubkey0.0 == pubkey1.0 {
        Err(CliError::BadParameter(format!(
            "Identical pubkeys found: `{}` and `{}` must be unique",
            pubkey0.1, pubkey1.1
        )))
    } else {
        Ok(())
    }
}
