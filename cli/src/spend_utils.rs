use {
    crate::{
        checks::{check_account_for_balance_with_commitment, get_fee_for_messages},
        cli::CliError,
    },
    clap::ArgMatches,
    solana_clap_utils::{input_parsers::lamports_of_sol, offline::SIGN_ONLY_ARG},
    solana_rpc_client::rpc_client::RpcClient,
    solana_sdk::{
        commitment_config::CommitmentConfig, hash::Hash, message::Message,
        native_token::lamports_to_sol, pubkey::Pubkey,
    },
};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum SpendAmount {
    All,
    Some(u64),
    RentExempt,
}

impl Default for SpendAmount {
    fn default() -> Self {
        Self::Some(u64::default())
    }
}

impl SpendAmount {
    pub fn new(amount: Option<u64>, sign_only: bool) -> Self {
        match amount {
            Some(lamports) => Self::Some(lamports),
            None if !sign_only => Self::All,
            _ => panic!("ALL amount not supported for sign-only operations"),
        }
    }

    pub fn new_from_matches(matches: &ArgMatches<'_>, name: &str) -> Self {
        let amount = lamports_of_sol(matches, name);
        let sign_only = matches.is_present(SIGN_ONLY_ARG.name);
        SpendAmount::new(amount, sign_only)
    }
}

struct SpendAndFee {
    spend: u64,
    fee: u64,
}

pub fn resolve_spend_tx_and_check_account_balance<F>(
    rpc_client: &RpcClient,
    sign_only: bool,
    amount: SpendAmount,
    blockhash: &Hash,
    from_pubkey: &Pubkey,
    build_message: F,
    commitment: CommitmentConfig,
) -> Result<(Message, u64), CliError>
where
    F: Fn(u64) -> Message,
{
    resolve_spend_tx_and_check_account_balances(
        rpc_client,
        sign_only,
        amount,
        blockhash,
        from_pubkey,
        from_pubkey,
        build_message,
        commitment,
    )
}

pub fn resolve_spend_tx_and_check_account_balances<F>(
    rpc_client: &RpcClient,
    sign_only: bool,
    amount: SpendAmount,
    blockhash: &Hash,
    from_pubkey: &Pubkey,
    fee_pubkey: &Pubkey,
    build_message: F,
    commitment: CommitmentConfig,
) -> Result<(Message, u64), CliError>
where
    F: Fn(u64) -> Message,
{
    if sign_only {
        let (message, SpendAndFee { spend, fee: _ }) = resolve_spend_message(
            rpc_client,
            amount,
            None,
            0,
            from_pubkey,
            fee_pubkey,
            0,
            build_message,
        )?;
        Ok((message, spend))
    } else {
        let from_balance = rpc_client
            .get_balance_with_commitment(from_pubkey, commitment)?
            .value;
        let from_rent_exempt_minimum = if amount == SpendAmount::RentExempt {
            let data = rpc_client.get_account_data(from_pubkey)?;
            rpc_client.get_minimum_balance_for_rent_exemption(data.len())?
        } else {
            0
        };
        let (message, SpendAndFee { spend, fee }) = resolve_spend_message(
            rpc_client,
            amount,
            Some(blockhash),
            from_balance,
            from_pubkey,
            fee_pubkey,
            from_rent_exempt_minimum,
            build_message,
        )?;
        if from_pubkey == fee_pubkey {
            if from_balance == 0 || from_balance < spend + fee {
                return Err(CliError::InsufficientFundsForSpendAndFee(
                    lamports_to_sol(spend),
                    lamports_to_sol(fee),
                    *from_pubkey,
                ));
            }
        } else {
            if from_balance < spend {
                return Err(CliError::InsufficientFundsForSpend(
                    lamports_to_sol(spend),
                    *from_pubkey,
                ));
            }
            if !check_account_for_balance_with_commitment(rpc_client, fee_pubkey, fee, commitment)?
            {
                return Err(CliError::InsufficientFundsForFee(
                    lamports_to_sol(fee),
                    *fee_pubkey,
                ));
            }
        }
        Ok((message, spend))
    }
}

fn resolve_spend_message<F>(
    rpc_client: &RpcClient,
    amount: SpendAmount,
    blockhash: Option<&Hash>,
    from_balance: u64,
    from_pubkey: &Pubkey,
    fee_pubkey: &Pubkey,
    from_rent_exempt_minimum: u64,
    build_message: F,
) -> Result<(Message, SpendAndFee), CliError>
where
    F: Fn(u64) -> Message,
{
    let fee = match blockhash {
        Some(blockhash) => {
            let mut dummy_message = build_message(0);
            dummy_message.recent_blockhash = *blockhash;
            get_fee_for_messages(rpc_client, &[&dummy_message])?
        }
        None => 0, // Offline, cannot calulate fee
    };

    match amount {
        SpendAmount::Some(lamports) => Ok((
            build_message(lamports),
            SpendAndFee {
                spend: lamports,
                fee,
            },
        )),
        SpendAmount::All => {
            let lamports = if from_pubkey == fee_pubkey {
                from_balance.saturating_sub(fee)
            } else {
                from_balance
            };
            Ok((
                build_message(lamports),
                SpendAndFee {
                    spend: lamports,
                    fee,
                },
            ))
        }
        SpendAmount::RentExempt => {
            let mut lamports = if from_pubkey == fee_pubkey {
                from_balance.saturating_sub(fee)
            } else {
                from_balance
            };
            lamports = lamports.saturating_sub(from_rent_exempt_minimum);
            Ok((
                build_message(lamports),
                SpendAndFee {
                    spend: lamports,
                    fee,
                },
            ))
        }
    }
}
