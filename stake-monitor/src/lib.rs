use log::*;
use serde::{Deserialize, Serialize};
use solana_client::{client_error::Result as ClientResult, rpc_client::RpcClient};
use solana_metrics::{datapoint_error, datapoint_info};
use solana_sdk::{clock::Slot, program_utils::limited_deserialize, transaction::Transaction};
use solana_stake_program::{stake_instruction::StakeInstruction, stake_state::Lockup};
use solana_transaction_status::{ConfirmedBlock, RpcTransactionStatusMeta, TransactionEncoding};
use std::{collections::HashMap, thread::sleep, time::Duration};

pub type PubkeyString = String;
pub type SignatureString = String;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum StakeAccountOperation {
    Initialize,
    Withdraw,
    SplitSource,
    SplitDestination,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StakeAccountTransactionInfo {
    pub op: StakeAccountOperation,
    pub slot: Slot,                 // Slot the transaction completed in
    pub signature: SignatureString, // Transaction signature
}

#[derive(Serialize, Deserialize, Debug)]
pub struct StakeAccountInfo {
    pub compliant_since: Option<Slot>, // The slot when the account was first in compliance
    pub lamports: u64,                 // Account balance
    pub transactions: Vec<StakeAccountTransactionInfo>, // Transactions affecting the account
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct StakeAccountsInfo {
    pub slot: Slot, // Latest processed slot
    pub account_info: HashMap<PubkeyString, StakeAccountInfo>,
}

fn process_transaction(
    slot: Slot,
    transaction: &Transaction,
    meta: &RpcTransactionStatusMeta,
    stake_accounts: &mut HashMap<PubkeyString, StakeAccountInfo>,
) {
    let mut last_instruction = true;
    let message = &transaction.message;
    for instruction in message.instructions.iter().rev() {
        let program_pubkey = message.account_keys[instruction.program_id_index as usize];
        if program_pubkey != solana_stake_program::id() {
            continue;
        }

        // Only look for stake instructions in the last instruction of a
        // transaction.  This ensures that the `meta.post_balances` for the
        // transaction reflects the account balances after the stake instruction
        // executed.  At this time the `solana` cli will only create transactions with the stake
        // instruction as the last instruction.
        if !last_instruction {
            datapoint_error!(
                "stake-monitor-failure",
                ("slot", slot, i64),
                ("err", "Stake instruction ignored", String)
            );
            continue;
        }
        last_instruction = false;

        match limited_deserialize::<StakeInstruction>(&instruction.data) {
            Err(err) => datapoint_error!(
                "stake-monitor-failure",
                ("slot", slot, i64),
                (
                    "err",
                    format!("Failed to deserialize stake instruction: {}", err),
                    String
                )
            ),
            Ok(stake_instruction) => {
                let signature = transaction.signatures[0].to_string();

                match stake_instruction {
                    StakeInstruction::Initialize(_authorized, lockup) => {
                        // The initialized stake account is at instruction account 0
                        let stake_account_index = instruction.accounts[0] as usize;

                        let stake_pubkey = message.account_keys[stake_account_index].to_string();

                        // The amount staked is the stake account's post balance
                        let lamports = meta.post_balances[stake_account_index];

                        stake_accounts.insert(
                            stake_pubkey,
                            StakeAccountInfo {
                                compliant_since: if lockup != Lockup::default() {
                                    None // Initialize with a lockup or custodian is non-compliant
                                } else {
                                    Some(slot)
                                },
                                lamports,
                                transactions: vec![StakeAccountTransactionInfo {
                                    op: StakeAccountOperation::Initialize,
                                    slot,
                                    signature,
                                }],
                            },
                        );
                    }
                    StakeInstruction::Authorize(_, _)
                    | StakeInstruction::DelegateStake
                    | StakeInstruction::Deactivate => {
                        // These instructions are always permitted
                    }
                    StakeInstruction::Split(lamports) => {
                        // Split is permitted and propagates compliance
                        let source_stake_account_index = instruction.accounts[0] as usize;
                        let split_stake_account_index = instruction.accounts[1] as usize;

                        let source_stake_pubkey =
                            message.account_keys[source_stake_account_index].to_string();
                        let split_stake_pubkey =
                            message.account_keys[split_stake_account_index].to_string();

                        if let Some(mut source_stake_account_info) =
                            stake_accounts.get_mut(&source_stake_pubkey)
                        {
                            if source_stake_account_info.compliant_since.is_some() {
                                source_stake_account_info.transactions.push(
                                    StakeAccountTransactionInfo {
                                        op: StakeAccountOperation::SplitSource,
                                        slot,
                                        signature: signature.clone(),
                                    },
                                );
                                source_stake_account_info.lamports -= lamports;

                                let split_stake_account_info = StakeAccountInfo {
                                    compliant_since: source_stake_account_info.compliant_since,
                                    lamports,
                                    transactions: vec![StakeAccountTransactionInfo {
                                        op: StakeAccountOperation::SplitDestination,
                                        slot,
                                        signature,
                                    }],
                                };
                                stake_accounts.insert(split_stake_pubkey, split_stake_account_info);
                            }
                        }
                    }
                    StakeInstruction::Withdraw(_) => {
                        // Withdrawing is not permitted

                        let stake_account_index = instruction.accounts[0] as usize;
                        let stake_pubkey = message.account_keys[stake_account_index].to_string();

                        if let Some(mut stake_account_info) = stake_accounts.get_mut(&stake_pubkey)
                        {
                            if stake_account_info.compliant_since.is_some() {
                                stake_account_info.compliant_since = None;
                                stake_account_info
                                    .transactions
                                    .push(StakeAccountTransactionInfo {
                                        op: StakeAccountOperation::Withdraw,
                                        slot,
                                        signature,
                                    });
                            }
                        }
                    }
                    StakeInstruction::SetLockup(_lockup_args) => {
                        // No processing is required because SetLockup requires a custodian key,
                        // and this is already blocked in the StakeInstruction::Initialize
                        // processing
                    }
                }
            }
        }
    }
}

fn process_confirmed_block(
    slot: Slot,
    confirmed_block: ConfirmedBlock,
    stake_accounts: &mut HashMap<PubkeyString, StakeAccountInfo>,
) {
    for rpc_transaction in confirmed_block.transactions {
        match rpc_transaction.meta {
            None => {
                datapoint_error!(
                    "stake-monitor-failure",
                    ("slot", slot, i64),
                    ("err", "Transaction meta not available", String)
                );
            }
            Some(meta) => {
                if meta.err.is_none() {
                    if let Some(transaction) = rpc_transaction.transaction.decode() {
                        if transaction.verify().is_ok() {
                            process_transaction(slot, &transaction, &meta, stake_accounts);
                        } else {
                            datapoint_error!(
                                "stake-monitor-failure",
                                ("slot", slot, i64),
                                ("err", "Transaction signature verification failed", String)
                            );
                        }
                    }
                }
            }
        }
    }
}

fn load_blocks(
    rpc_client: &RpcClient,
    start_slot: Slot,
    end_slot: Slot,
) -> ClientResult<Vec<(Slot, ConfirmedBlock)>> {
    info!(
        "Loading confirmed blocks between slots: {} - {}",
        start_slot, end_slot
    );

    let slots = rpc_client.get_confirmed_blocks(start_slot, Some(end_slot))?;

    let mut blocks = vec![];
    for slot in slots.into_iter() {
        let block =
            rpc_client.get_confirmed_block_with_encoding(slot, TransactionEncoding::Binary)?;
        blocks.push((slot, block));
    }
    Ok(blocks)
}

pub fn process_slots(
    rpc_client: &RpcClient,
    stake_accounts_info: &mut StakeAccountsInfo,
    batch_size: u64,
) {
    let end_slot = stake_accounts_info.slot + batch_size;
    loop {
        let start_slot = stake_accounts_info.slot + 1;
        info!("start_slot:{} - end_slot:{}", start_slot, end_slot);
        if start_slot >= end_slot {
            break;
        }
        let latest_available_slot = rpc_client.get_slot().unwrap_or_else(|err| {
            datapoint_error!(
                "stake-monitor-failure",
                ("err", format!("get_slot() failed: {}", err), String)
            );
            0
        });

        if stake_accounts_info.slot >= latest_available_slot {
            info!(
                "Waiting for a slot greater than {}...",
                stake_accounts_info.slot
            );
            sleep(Duration::from_secs(5));
            continue;
        }

        match load_blocks(&rpc_client, start_slot, end_slot) {
            Ok(blocks) => {
                info!("Loaded {} blocks", blocks.len());

                if blocks.is_empty() && end_slot < latest_available_slot {
                    stake_accounts_info.slot = end_slot;
                } else {
                    for (slot, block) in blocks.into_iter() {
                        process_confirmed_block(slot, block, &mut stake_accounts_info.account_info);
                        stake_accounts_info.slot = slot;
                    }
                }
                datapoint_info!(
                    "stake-monitor-slot",
                    ("slot", stake_accounts_info.slot, i64)
                );
            }
            Err(err) => {
                datapoint_error!(
                    "stake-monitor-failure",
                    (
                        "err",
                        format!(
                            "failed to get blocks in range ({},{}): {}",
                            start_slot, end_slot, err
                        ),
                        String
                    )
                );
            }
        }
    }
}
