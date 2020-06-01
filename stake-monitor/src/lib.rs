use log::*;
use serde::{Deserialize, Serialize};
use solana_client::{client_error::Result as ClientResult, rpc_client::RpcClient};
use solana_metrics::{datapoint_error, datapoint_info};
use solana_sdk::{
    clock::Slot, native_token::LAMPORTS_PER_SOL, program_utils::limited_deserialize,
    pubkey::Pubkey, signature::Signature, transaction::Transaction,
};
use solana_stake_program::{stake_instruction::StakeInstruction, stake_state::Lockup};
use solana_transaction_status::{ConfirmedBlock, RpcTransactionStatusMeta, TransactionEncoding};
use std::{collections::HashMap, thread::sleep, time::Duration};

pub type PubkeyString = String;
pub type SignatureString = String;

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub enum AccountOperation {
    Initialize,
    Withdraw,
    SplitSource,
    SplitDestination,
    SystemAccountEnroll,
    FailedToMaintainMinimumBalance,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AccountTransactionInfo {
    pub op: AccountOperation,
    pub slot: Slot,                 // Slot the transaction completed in
    pub signature: SignatureString, // Transaction signature
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AccountInfo {
    pub compliant_since: Option<Slot>, // The slot when the account was first in compliance
    pub lamports: u64,                 // Account balance
    pub transactions: Vec<AccountTransactionInfo>, // Transactions affecting the account
}

#[derive(Serialize, Deserialize, Default, Debug)]
pub struct AccountsInfo {
    pub slot: Slot, // Latest processed slot
    pub account_info: HashMap<PubkeyString, AccountInfo>,
}

impl AccountsInfo {
    // System accounts must be explicitly enrolled
    pub fn enroll_system_account(&mut self, account_address: &Pubkey, slot: Slot, lamports: u64) {
        self.account_info.insert(
            account_address.to_string(),
            AccountInfo {
                compliant_since: Some(slot),
                lamports,
                transactions: vec![AccountTransactionInfo {
                    op: AccountOperation::SystemAccountEnroll,
                    slot,
                    signature: Signature::default().to_string(),
                }],
            },
        );
    }
}

fn process_transaction(
    slot: Slot,
    transaction: &Transaction,
    meta: &RpcTransactionStatusMeta,
    accounts: &mut HashMap<PubkeyString, AccountInfo>,
) {
    let mut last_instruction = true;
    let message = &transaction.message;
    let signature = transaction.signatures[0].to_string();

    // Look for stake operations
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
                match stake_instruction {
                    StakeInstruction::Initialize(_authorized, lockup) => {
                        // The initialized stake account is at instruction account 0
                        let stake_account_index = instruction.accounts[0] as usize;

                        let stake_pubkey = message.account_keys[stake_account_index].to_string();

                        // The amount staked is the stake account's post balance
                        let lamports = meta.post_balances[stake_account_index];

                        accounts.insert(
                            stake_pubkey,
                            AccountInfo {
                                compliant_since: if lockup != Lockup::default() {
                                    None // Initialize with a lockup or custodian is non-compliant
                                } else {
                                    Some(slot)
                                },
                                lamports,
                                transactions: vec![AccountTransactionInfo {
                                    op: AccountOperation::Initialize,
                                    slot,
                                    signature: signature.clone(),
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

                        if let Some(mut source_account_info) =
                            accounts.get_mut(&source_stake_pubkey)
                        {
                            if source_account_info.compliant_since.is_some() {
                                source_account_info
                                    .transactions
                                    .push(AccountTransactionInfo {
                                        op: AccountOperation::SplitSource,
                                        slot,
                                        signature: signature.clone(),
                                    });
                                source_account_info.lamports -= lamports;

                                let split_account_info = AccountInfo {
                                    compliant_since: source_account_info.compliant_since,
                                    lamports,
                                    transactions: vec![AccountTransactionInfo {
                                        op: AccountOperation::SplitDestination,
                                        slot,
                                        signature: signature.clone(),
                                    }],
                                };
                                accounts.insert(split_stake_pubkey, split_account_info);
                            }
                        }
                    }
                    StakeInstruction::Withdraw(_) => {
                        // Withdrawing is not permitted

                        let stake_account_index = instruction.accounts[0] as usize;
                        let stake_pubkey = message.account_keys[stake_account_index].to_string();

                        if let Some(mut account_info) = accounts.get_mut(&stake_pubkey) {
                            if account_info.compliant_since.is_some() {
                                account_info.compliant_since = None;
                                account_info.transactions.push(AccountTransactionInfo {
                                    op: AccountOperation::Withdraw,
                                    slot,
                                    signature: signature.clone(),
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

    // Ensure the balances of all monitored accounts remain in compliance
    for (index, account_pubkey) in message.account_keys.iter().enumerate() {
        if let Some(mut account_info) = accounts.get_mut(&account_pubkey.to_string()) {
            let post_balance = meta.post_balances[index];
            if account_info.compliant_since.is_some()
                && post_balance <= account_info.lamports.saturating_sub(LAMPORTS_PER_SOL)
            {
                account_info.compliant_since = None;
                account_info.transactions.push(AccountTransactionInfo {
                    op: AccountOperation::FailedToMaintainMinimumBalance,
                    slot,
                    signature: signature.clone(),
                });
            }
        }
    }
}

fn process_confirmed_block(
    slot: Slot,
    confirmed_block: ConfirmedBlock,
    accounts: &mut HashMap<PubkeyString, AccountInfo>,
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
                            process_transaction(slot, &transaction, &meta, accounts);
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

pub fn process_slots(rpc_client: &RpcClient, accounts_info: &mut AccountsInfo, batch_size: u64) {
    let end_slot = accounts_info.slot + batch_size;
    loop {
        let start_slot = accounts_info.slot + 1;
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

        if accounts_info.slot >= latest_available_slot {
            info!("Waiting for a slot greater than {}...", accounts_info.slot);
            sleep(Duration::from_secs(5));
            continue;
        }

        match load_blocks(&rpc_client, start_slot, end_slot) {
            Ok(blocks) => {
                info!("Loaded {} blocks", blocks.len());

                if blocks.is_empty() && end_slot < latest_available_slot {
                    accounts_info.slot = end_slot;
                } else {
                    for (slot, block) in blocks.into_iter() {
                        process_confirmed_block(slot, block, &mut accounts_info.account_info);
                        accounts_info.slot = slot;
                    }
                }
                datapoint_info!("stake-monitor-slot", ("slot", accounts_info.slot, i64));
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
                sleep(Duration::from_secs(1));
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use serial_test_derive::serial;
    use solana_client::rpc_config::RpcSendTransactionConfig;
    use solana_core::{rpc::JsonRpcConfig, validator::ValidatorConfig};
    use solana_local_cluster::local_cluster::{ClusterConfig, LocalCluster};
    use solana_sdk::{
        commitment_config::CommitmentConfig,
        genesis_config::OperatingMode,
        message::Message,
        native_token::sol_to_lamports,
        signature::{Keypair, Signer},
        system_transaction,
        transaction::Transaction,
    };
    use solana_stake_program::{stake_instruction, stake_state::Authorized};

    #[test]
    #[serial]
    fn test_record() {
        solana_logger::setup();
        let mut accounts_info = AccountsInfo::default();

        let one_sol = sol_to_lamports(1.0);
        let cluster = LocalCluster::new(&ClusterConfig {
            operating_mode: OperatingMode::Stable,
            node_stakes: vec![10; 1],
            cluster_lamports: sol_to_lamports(1_000_000_000.0),
            validator_configs: vec![ValidatorConfig {
                rpc_config: JsonRpcConfig {
                    enable_rpc_transaction_history: true,
                    ..JsonRpcConfig::default()
                },
                ..ValidatorConfig::default()
            }],
            ..ClusterConfig::default()
        });

        let payer = &cluster.funding_keypair;

        let rpc_client = RpcClient::new_socket(cluster.entry_point_info.rpc);

        let (blockhash, _fee_calculator) = rpc_client.get_recent_blockhash().unwrap();

        // Configure stake1
        let stake1_keypair = Keypair::new();
        let stake1_signature = rpc_client
            .send_transaction(&Transaction::new_signed_instructions(
                &[&payer, &stake1_keypair],
                &stake_instruction::create_account(
                    &payer.pubkey(),
                    &stake1_keypair.pubkey(),
                    &Authorized::auto(&payer.pubkey()),
                    &Lockup::default(),
                    one_sol,
                ),
                blockhash,
            ))
            .unwrap();

        rpc_client
            .poll_for_signature_with_commitment(&stake1_signature, CommitmentConfig::recent())
            .unwrap();

        // A balance increase by system transfer is ignored
        rpc_client
            .send_transaction(&system_transaction::transfer(
                &payer,
                &stake1_keypair.pubkey(),
                one_sol,
                blockhash,
            ))
            .unwrap();

        // Configure stake2 with non-compliant lockup
        let stake2_keypair = Keypair::new();
        let stake2_signature = rpc_client
            .send_transaction(&Transaction::new_signed_instructions(
                &[&payer, &stake2_keypair],
                &stake_instruction::create_account(
                    &payer.pubkey(),
                    &stake2_keypair.pubkey(),
                    &Authorized::auto(&payer.pubkey()),
                    &Lockup {
                        custodian: payer.pubkey(),
                        ..Lockup::default()
                    },
                    one_sol,
                ),
                blockhash,
            ))
            .unwrap();

        // Configure stake3
        let stake3_keypair = Keypair::new();
        let stake3_initialize_signature = rpc_client
            .send_transaction(&Transaction::new_signed_instructions(
                &[&payer, &stake3_keypair],
                &stake_instruction::create_account(
                    &payer.pubkey(),
                    &stake3_keypair.pubkey(),
                    &Authorized::auto(&payer.pubkey()),
                    &Lockup::default(),
                    one_sol,
                ),
                blockhash,
            ))
            .unwrap();

        rpc_client
            .poll_for_signature_with_commitment(
                &stake3_initialize_signature,
                CommitmentConfig::recent(),
            )
            .unwrap();

        // Withdraw instruction causes non-compliance
        let stake3_withdraw_signature = rpc_client
            .send_transaction_with_config(
                &Transaction::new(
                    &[&payer, &stake3_keypair],
                    Message::new_with_payer(
                        &[stake_instruction::withdraw(
                            &stake3_keypair.pubkey(),
                            &stake3_keypair.pubkey(),
                            &payer.pubkey(),
                            one_sol,
                            None,
                        )],
                        Some(&payer.pubkey()),
                    ),
                    blockhash,
                ),
                RpcSendTransactionConfig {
                    skip_preflight: true,
                },
            )
            .unwrap();

        rpc_client
            .poll_for_signature_with_commitment(
                &stake3_withdraw_signature,
                CommitmentConfig::recent(),
            )
            .unwrap();

        // Configure stake4
        let stake4_keypair = Keypair::new();
        let stake4_initialize_signature = rpc_client
            .send_transaction(&Transaction::new_signed_instructions(
                &[&payer, &stake4_keypair],
                &stake_instruction::create_account(
                    &payer.pubkey(),
                    &stake4_keypair.pubkey(),
                    &Authorized::auto(&payer.pubkey()),
                    &Lockup::default(),
                    2 * one_sol,
                ),
                blockhash,
            ))
            .unwrap();

        rpc_client
            .poll_for_signature_with_commitment(
                &stake4_initialize_signature,
                CommitmentConfig::recent(),
            )
            .unwrap();

        // Split stake4 into stake5
        let stake5_keypair = Keypair::new();
        let stake45_split_signature = rpc_client
            .send_transaction_with_config(
                &Transaction::new(
                    &[&payer, &stake5_keypair],
                    Message::new_with_payer(
                        &stake_instruction::split(
                            &stake4_keypair.pubkey(),
                            &payer.pubkey(),
                            one_sol,
                            &stake5_keypair.pubkey(),
                        ),
                        Some(&payer.pubkey()),
                    ),
                    blockhash,
                ),
                RpcSendTransactionConfig {
                    skip_preflight: true,
                },
            )
            .unwrap();

        rpc_client
            .poll_for_signature_with_commitment(
                &stake45_split_signature,
                CommitmentConfig::recent(),
            )
            .unwrap();

        // System transfer 1
        let system1_keypair = Keypair::new();

        // Fund system1
        let fund_system1_signature = rpc_client
            .send_transaction(&system_transaction::transfer(
                &payer,
                &system1_keypair.pubkey(),
                2 * one_sol,
                blockhash,
            ))
            .unwrap();
        rpc_client
            .poll_for_signature_with_commitment(&fund_system1_signature, CommitmentConfig::recent())
            .unwrap();
        accounts_info.enroll_system_account(
            &system1_keypair.pubkey(),
            rpc_client
                .get_slot_with_commitment(CommitmentConfig::recent())
                .unwrap(),
            2 * one_sol,
        );

        // Withdraw 1 sol from system 1 to make it non-compliant
        rpc_client
            .send_transaction_with_config(
                &system_transaction::transfer(
                    &system1_keypair,
                    &payer.pubkey(),
                    one_sol,
                    blockhash,
                ),
                RpcSendTransactionConfig {
                    skip_preflight: true,
                },
            )
            .unwrap();

        // System transfer 2
        let system2_keypair = Keypair::new();

        // Fund system2
        let fund_system2_signature = rpc_client
            .send_transaction(&system_transaction::transfer(
                &payer,
                &system2_keypair.pubkey(),
                2 * one_sol,
                blockhash,
            ))
            .unwrap();
        rpc_client
            .poll_for_signature_with_commitment(&fund_system2_signature, CommitmentConfig::recent())
            .unwrap();
        accounts_info.enroll_system_account(
            &system2_keypair.pubkey(),
            rpc_client
                .get_slot_with_commitment(CommitmentConfig::recent())
                .unwrap(),
            2 * one_sol,
        );

        // Withdraw 1 sol - 1 lamport from system 2, it's still compliant
        rpc_client
            .send_transaction_with_config(
                &system_transaction::transfer(
                    &system2_keypair,
                    &payer.pubkey(),
                    one_sol - 1,
                    blockhash,
                ),
                RpcSendTransactionConfig {
                    skip_preflight: true,
                },
            )
            .unwrap();

        // Process all the transactions
        let current_slot = rpc_client
            .get_slot_with_commitment(CommitmentConfig::recent())
            .unwrap();
        process_slots(&rpc_client, &mut accounts_info, current_slot + 1);

        //
        // Check that `accounts_info` was populated with the expected results
        //

        info!("Check the data recorded for stake1");
        let account_info = accounts_info
            .account_info
            .get(&stake1_keypair.pubkey().to_string())
            .unwrap();
        assert!(account_info.compliant_since.is_some());
        assert_eq!(account_info.lamports, one_sol);
        assert_eq!(account_info.transactions.len(), 1);
        assert_eq!(
            account_info.transactions[0].op,
            AccountOperation::Initialize
        );
        assert_eq!(
            account_info.transactions[0].signature,
            stake1_signature.to_string()
        );

        info!("Check the data recorded for stake2");
        let account_info = accounts_info
            .account_info
            .get(&stake2_keypair.pubkey().to_string())
            .unwrap();
        assert!(account_info.compliant_since.is_none());
        assert_eq!(account_info.lamports, one_sol);
        assert_eq!(account_info.transactions.len(), 1);
        assert_eq!(
            account_info.transactions[0].op,
            AccountOperation::Initialize
        );
        assert_eq!(
            account_info.transactions[0].signature,
            stake2_signature.to_string()
        );

        info!("Check the data recorded for stake3");
        let account_info = accounts_info
            .account_info
            .get(&stake3_keypair.pubkey().to_string())
            .unwrap();
        assert!(account_info.compliant_since.is_none());
        assert_eq!(account_info.lamports, one_sol);
        assert_eq!(account_info.transactions.len(), 2);
        assert_eq!(
            account_info.transactions[0].op,
            AccountOperation::Initialize
        );
        assert_eq!(
            account_info.transactions[0].signature,
            stake3_initialize_signature.to_string()
        );
        assert_eq!(account_info.transactions[1].op, AccountOperation::Withdraw,);
        assert_eq!(
            account_info.transactions[1].signature,
            stake3_withdraw_signature.to_string()
        );

        info!("Check the data recorded for stake4");
        let account_info = accounts_info
            .account_info
            .get(&stake4_keypair.pubkey().to_string())
            .unwrap();
        assert!(account_info.compliant_since.is_some());
        assert_eq!(account_info.lamports, one_sol);
        assert_eq!(account_info.transactions.len(), 2);
        assert_eq!(
            account_info.transactions[0].op,
            AccountOperation::Initialize
        );
        assert_eq!(
            account_info.transactions[0].signature,
            stake4_initialize_signature.to_string()
        );
        assert_eq!(
            account_info.transactions[1].op,
            AccountOperation::SplitSource,
        );
        assert_eq!(
            account_info.transactions[1].signature,
            stake45_split_signature.to_string()
        );

        info!("Check the data recorded for stake5");
        let account_info = accounts_info
            .account_info
            .get(&stake5_keypair.pubkey().to_string())
            .unwrap();
        assert!(account_info.compliant_since.is_some());
        assert_eq!(account_info.lamports, one_sol);
        assert_eq!(account_info.transactions.len(), 1);
        assert_eq!(
            account_info.transactions[0].op,
            AccountOperation::SplitDestination,
        );
        assert_eq!(
            account_info.transactions[0].signature,
            stake45_split_signature.to_string()
        );

        info!("Check the data recorded for system1");
        let account_info = accounts_info
            .account_info
            .get(&system1_keypair.pubkey().to_string())
            .unwrap();
        assert!(account_info.compliant_since.is_none());
        assert_eq!(account_info.lamports, 2 * one_sol);
        assert_eq!(account_info.transactions.len(), 2);
        assert_eq!(
            account_info.transactions[0].op,
            AccountOperation::SystemAccountEnroll,
        );
        assert_eq!(
            account_info.transactions[1].op,
            AccountOperation::FailedToMaintainMinimumBalance,
        );

        info!("Check the data recorded for system2");
        let account_info = accounts_info
            .account_info
            .get(&system2_keypair.pubkey().to_string())
            .unwrap();
        assert!(account_info.compliant_since.is_some());
        assert_eq!(account_info.lamports, 2 * one_sol);
        assert_eq!(account_info.transactions.len(), 1);
        assert_eq!(
            account_info.transactions[0].op,
            AccountOperation::SystemAccountEnroll,
        );
    }
}
