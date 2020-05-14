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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use solana_client::{
        rpc_request::RpcRequest,
        rpc_response::{Response, RpcResponseContext},
    };
    use solana_sdk::system_instruction;
    use std::collections::HashMap;

    #[test]
    fn test_check_account_for_fees() {
        let account_balance = 1;
        let account_balance_response = json!(Response {
            context: RpcResponseContext { slot: 1 },
            value: json!(account_balance),
        });
        let pubkey = Pubkey::new_rand();
        let fee_calculator = FeeCalculator::new(1);

        let pubkey0 = Pubkey::new(&[0; 32]);
        let pubkey1 = Pubkey::new(&[1; 32]);
        let ix0 = system_instruction::transfer(&pubkey0, &pubkey1, 1);
        let message0 = Message::new(&[ix0]);

        let ix0 = system_instruction::transfer(&pubkey0, &pubkey1, 1);
        let ix1 = system_instruction::transfer(&pubkey1, &pubkey0, 1);
        let message1 = Message::new(&[ix0, ix1]);

        let mut mocks = HashMap::new();
        mocks.insert(RpcRequest::GetBalance, account_balance_response.clone());
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert_eq!(
            check_account_for_fee(&rpc_client, &pubkey, &fee_calculator, &message0).unwrap(),
            ()
        );

        let mut mocks = HashMap::new();
        mocks.insert(RpcRequest::GetBalance, account_balance_response.clone());
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert!(check_account_for_fee(&rpc_client, &pubkey, &fee_calculator, &message1).is_err());

        let mut mocks = HashMap::new();
        mocks.insert(RpcRequest::GetBalance, account_balance_response);
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);
        assert!(check_account_for_multiple_fees(
            &rpc_client,
            &pubkey,
            &fee_calculator,
            &[&message0, &message0]
        )
        .is_err());

        let account_balance = 2;
        let account_balance_response = json!(Response {
            context: RpcResponseContext { slot: 1 },
            value: json!(account_balance),
        });

        let mut mocks = HashMap::new();
        mocks.insert(RpcRequest::GetBalance, account_balance_response);
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);

        assert_eq!(
            check_account_for_multiple_fees(
                &rpc_client,
                &pubkey,
                &fee_calculator,
                &[&message0, &message0]
            )
            .unwrap(),
            ()
        );
    }

    #[test]
    fn test_check_account_for_balance() {
        let account_balance = 50;
        let account_balance_response = json!(Response {
            context: RpcResponseContext { slot: 1 },
            value: json!(account_balance),
        });
        let pubkey = Pubkey::new_rand();

        let mut mocks = HashMap::new();
        mocks.insert(RpcRequest::GetBalance, account_balance_response);
        let rpc_client = RpcClient::new_mock_with_mocks("".to_string(), mocks);

        assert_eq!(
            check_account_for_balance(&rpc_client, &pubkey, 1).unwrap(),
            true
        );
        assert_eq!(
            check_account_for_balance(&rpc_client, &pubkey, account_balance).unwrap(),
            true
        );
        assert_eq!(
            check_account_for_balance(&rpc_client, &pubkey, account_balance + 1).unwrap(),
            false
        );
    }

    #[test]
    fn test_calculate_fee() {
        let fee_calculator = FeeCalculator::new(1);
        // No messages, no fee.
        assert_eq!(calculate_fee(&fee_calculator, &[]), 0);

        // No signatures, no fee.
        let message = Message::new(&[]);
        assert_eq!(calculate_fee(&fee_calculator, &[&message, &message]), 0);

        // One message w/ one signature, a fee.
        let pubkey0 = Pubkey::new(&[0; 32]);
        let pubkey1 = Pubkey::new(&[1; 32]);
        let ix0 = system_instruction::transfer(&pubkey0, &pubkey1, 1);
        let message0 = Message::new(&[ix0]);
        assert_eq!(calculate_fee(&fee_calculator, &[&message0]), 1);

        // Two messages, additive fees.
        let ix0 = system_instruction::transfer(&pubkey0, &pubkey1, 1);
        let ix1 = system_instruction::transfer(&pubkey1, &pubkey0, 1);
        let message1 = Message::new(&[ix0, ix1]);
        assert_eq!(calculate_fee(&fee_calculator, &[&message0, &message1]), 3);
    }

    #[test]
    fn test_check_unique_pubkeys() {
        let pubkey0 = Pubkey::new_rand();
        let pubkey_clone = pubkey0.clone();
        let pubkey1 = Pubkey::new_rand();

        assert_eq!(
            check_unique_pubkeys((&pubkey0, "foo".to_string()), (&pubkey1, "bar".to_string()))
                .unwrap(),
            ()
        );
        assert_eq!(
            check_unique_pubkeys((&pubkey0, "foo".to_string()), (&pubkey1, "foo".to_string()))
                .unwrap(),
            ()
        );
        assert!(check_unique_pubkeys(
            (&pubkey0, "foo".to_string()),
            (&pubkey_clone, "bar".to_string())
        )
        .is_err());
    }
}
