use {
    crate::{
        checks::{check_account_for_balance_with_commitment, get_fee_for_messages},
        cli::CliError,
        compute_budget::{simulate_and_update_compute_unit_limit, UpdateComputeUnitLimitResult},
    },
    clap::ArgMatches,
    solana_clap_utils::{
        compute_budget::ComputeUnitLimit, input_parsers::lamports_of_sol, offline::SIGN_ONLY_ARG,
    },
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
    AllForAccountCreation { create_account_min_balance: u64 },
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
    compute_unit_limit: ComputeUnitLimit,
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
        compute_unit_limit,
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
    compute_unit_limit: ComputeUnitLimit,
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
            compute_unit_limit,
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
            compute_unit_limit,
            build_message,
        )?;
        if from_pubkey == fee_pubkey {
            if from_balance == 0 || from_balance < spend.saturating_add(fee) {
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
    compute_unit_limit: ComputeUnitLimit,
    build_message: F,
) -> Result<(Message, SpendAndFee), CliError>
where
    F: Fn(u64) -> Message,
{
    let (fee, compute_unit_info) = match blockhash {
        Some(blockhash) => {
            // If the from account is the same as the fee payer, it's impossible
            // to give a correct amount for the simulation with `SpendAmount::All`
            // or `SpendAmount::RentExempt`.
            // To know how much to transfer, we need to know the transaction fee,
            // but the transaction fee is dependent on the amount of compute
            // units used, which requires simulation.
            // To get around this limitation, we simulate against an amount of
            // `0`, since there are few situations in which `SpendAmount` can
            // be `All` or `RentExempt` *and also* the from account is the fee
            // payer.
            let lamports = if from_pubkey == fee_pubkey {
                match amount {
                    SpendAmount::Some(lamports) => lamports,
                    SpendAmount::AllForAccountCreation {
                        create_account_min_balance,
                    } => create_account_min_balance,
                    SpendAmount::All | SpendAmount::RentExempt => 0,
                }
            } else {
                match amount {
                    SpendAmount::Some(lamports) => lamports,
                    SpendAmount::AllForAccountCreation { .. } | SpendAmount::All => from_balance,
                    SpendAmount::RentExempt => {
                        from_balance.saturating_sub(from_rent_exempt_minimum)
                    }
                }
            };
            let mut dummy_message = build_message(lamports);

            dummy_message.recent_blockhash = *blockhash;
            let compute_unit_info = if compute_unit_limit == ComputeUnitLimit::Simulated {
                // Simulate for correct compute units
                if let UpdateComputeUnitLimitResult::UpdatedInstructionIndex(ix_index) =
                    simulate_and_update_compute_unit_limit(rpc_client, &mut dummy_message)?
                {
                    Some((ix_index, dummy_message.instructions[ix_index].data.clone()))
                } else {
                    None
                }
            } else {
                None
            };
            (
                get_fee_for_messages(rpc_client, &[&dummy_message])?,
                compute_unit_info,
            )
        }
        None => (0, None), // Offline, cannot calculate fee
    };

    let (mut message, spend_and_fee) = match amount {
        SpendAmount::Some(lamports) => (
            build_message(lamports),
            SpendAndFee {
                spend: lamports,
                fee,
            },
        ),
        SpendAmount::All | SpendAmount::AllForAccountCreation { .. } => {
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
        SpendAmount::RentExempt => {
            let mut lamports = if from_pubkey == fee_pubkey {
                from_balance.saturating_sub(fee)
            } else {
                from_balance
            };
            lamports = lamports.saturating_sub(from_rent_exempt_minimum);
            (
                build_message(lamports),
                SpendAndFee {
                    spend: lamports,
                    fee,
                },
            )
        }
    };
    // After build message, update with correct compute units
    if let Some((ix_index, ix_data)) = compute_unit_info {
        message.instructions[ix_index].data = ix_data;
    }
    Ok((message, spend_and_fee))
}
