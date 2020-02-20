use crate::{
    client_error::ClientError,
    generic_rpc_client_request::GenericRpcClientRequest,
    mock_rpc_client_request::{MockRpcClientRequest, Mocks},
    rpc_client_request::RpcClientRequest,
    rpc_request::RpcRequest,
    rpc_response::{
        Response, RpcAccount, RpcBlockhashFeeCalculator, RpcConfirmedBlock, RpcContactInfo,
        RpcEpochInfo, RpcKeyedAccount, RpcLeaderSchedule, RpcResponse, RpcVersionInfo,
        RpcVoteAccountStatus,
    },
};
use bincode::serialize;
use log::*;
use serde_json::{json, Value};
use solana_sdk::{
    account::Account,
    clock::{Slot, UnixTimestamp, DEFAULT_TICKS_PER_SECOND, DEFAULT_TICKS_PER_SLOT},
    commitment_config::CommitmentConfig,
    epoch_schedule::EpochSchedule,
    fee_calculator::FeeCalculator,
    hash::Hash,
    inflation::Inflation,
    pubkey::Pubkey,
    signature::Signature,
    signers::Signers,
    transaction::{self, Transaction, TransactionError},
};
use std::{
    error, io,
    net::SocketAddr,
    thread::sleep,
    time::{Duration, Instant},
};

pub struct RpcClient {
    client: Box<dyn GenericRpcClientRequest + Send + Sync>,
}

impl RpcClient {
    pub fn new(url: String) -> Self {
        Self {
            client: Box::new(RpcClientRequest::new(url)),
        }
    }

    pub fn new_mock(url: String) -> Self {
        Self {
            client: Box::new(MockRpcClientRequest::new(url)),
        }
    }

    pub fn new_mock_with_mocks(url: String, mocks: Mocks) -> Self {
        Self {
            client: Box::new(MockRpcClientRequest::new_with_mocks(url, mocks)),
        }
    }

    pub fn new_socket(addr: SocketAddr) -> Self {
        Self::new(get_rpc_request_str(addr, false))
    }

    pub fn new_socket_with_timeout(addr: SocketAddr, timeout: Duration) -> Self {
        let url = get_rpc_request_str(addr, false);
        Self {
            client: Box::new(RpcClientRequest::new_with_timeout(url, timeout)),
        }
    }

    pub fn confirm_transaction(&self, signature: &str) -> io::Result<bool> {
        Ok(self
            .confirm_transaction_with_commitment(signature, CommitmentConfig::default())?
            .value)
    }

    pub fn confirm_transaction_with_commitment(
        &self,
        signature: &str,
        commitment_config: CommitmentConfig,
    ) -> RpcResponse<bool> {
        let response = self
            .client
            .send(
                &RpcRequest::ConfirmTransaction,
                json!([signature, commitment_config]),
                0,
            )
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("ConfirmTransaction request failure {:?}", err),
                )
            })?;

        serde_json::from_value::<Response<bool>>(response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("Received result of an unexpected type {:?}", err),
            )
        })
    }

    pub fn send_transaction(&self, transaction: &Transaction) -> Result<String, ClientError> {
        let serialized_encoded = bs58::encode(serialize(transaction).unwrap()).into_string();
        let signature =
            self.client
                .send(&RpcRequest::SendTransaction, json!([serialized_encoded]), 5)?;
        if signature.as_str().is_none() {
            Err(io::Error::new(
                io::ErrorKind::Other,
                "Received result of an unexpected type",
            )
            .into())
        } else {
            Ok(signature.as_str().unwrap().to_string())
        }
    }

    pub fn get_signature_status(
        &self,
        signature: &str,
    ) -> Result<Option<transaction::Result<()>>, ClientError> {
        self.get_signature_status_with_commitment(signature, CommitmentConfig::default())
    }

    pub fn get_signature_status_with_commitment(
        &self,
        signature: &str,
        commitment_config: CommitmentConfig,
    ) -> Result<Option<transaction::Result<()>>, ClientError> {
        let signature_status = self.client.send(
            &RpcRequest::GetSignatureStatus,
            json!([signature.to_string(), commitment_config]),
            5,
        )?;
        let result: Option<transaction::Result<()>> =
            serde_json::from_value(signature_status).unwrap();
        Ok(result)
    }

    pub fn get_slot(&self) -> io::Result<Slot> {
        self.get_slot_with_commitment(CommitmentConfig::default())
    }

    pub fn get_slot_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> io::Result<Slot> {
        let response = self
            .client
            .send(&RpcRequest::GetSlot, json!([commitment_config]), 0)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetSlot request failure: {:?}", err),
                )
            })?;

        serde_json::from_value(response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetSlot parse failure: {}", err),
            )
        })
    }

    pub fn get_vote_accounts(&self) -> io::Result<RpcVoteAccountStatus> {
        let response = self
            .client
            .send(&RpcRequest::GetVoteAccounts, Value::Null, 0)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetVoteAccounts request failure: {:?}", err),
                )
            })?;

        serde_json::from_value(response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetVoteAccounts parse failure: {:?}", err),
            )
        })
    }

    pub fn get_cluster_nodes(&self) -> io::Result<Vec<RpcContactInfo>> {
        let response = self
            .client
            .send(&RpcRequest::GetClusterNodes, Value::Null, 0)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetClusterNodes request failure: {:?}", err),
                )
            })?;

        serde_json::from_value(response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetClusterNodes parse failure: {:?}", err),
            )
        })
    }

    pub fn get_confirmed_block(&self, slot: Slot) -> io::Result<RpcConfirmedBlock> {
        let response = self
            .client
            .send(&RpcRequest::GetConfirmedBlock, json!([slot]), 0)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetConfirmedBlock request failure: {:?}", err),
                )
            })?;

        serde_json::from_value(response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetConfirmedBlock parse failure: {:?}", err),
            )
        })
    }

    pub fn get_confirmed_blocks(
        &self,
        start_slot: Slot,
        end_slot: Option<Slot>,
    ) -> io::Result<Vec<Slot>> {
        let response = self
            .client
            .send(
                &RpcRequest::GetConfirmedBlocks,
                json!([start_slot, end_slot]),
                0,
            )
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetConfirmedBlocks request failure: {:?}", err),
                )
            })?;

        serde_json::from_value(response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetConfirmedBlocks parse failure: {:?}", err),
            )
        })
    }

    pub fn get_block_time(&self, slot: Slot) -> io::Result<UnixTimestamp> {
        let response = self
            .client
            .send(&RpcRequest::GetBlockTime, json!([slot]), 0);

        response
            .map(|result_json| {
                if result_json.is_null() {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("Block Not Found: slot={}", slot),
                    ));
                }
                let result = serde_json::from_value(result_json)?;
                trace!("Response block timestamp {:?} {:?}", slot, result);
                Ok(result)
            })
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetBlockTime request failure: {:?}", err),
                )
            })?
    }

    pub fn get_epoch_info(&self) -> io::Result<RpcEpochInfo> {
        self.get_epoch_info_with_commitment(CommitmentConfig::default())
    }

    pub fn get_epoch_info_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> io::Result<RpcEpochInfo> {
        let response = self
            .client
            .send(&RpcRequest::GetEpochInfo, json!([commitment_config]), 0)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetEpochInfo request failure: {:?}", err),
                )
            })?;

        serde_json::from_value(response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetEpochInfo parse failure: {:?}", err),
            )
        })
    }

    pub fn get_leader_schedule(&self, slot: Option<Slot>) -> io::Result<Option<RpcLeaderSchedule>> {
        self.get_leader_schedule_with_commitment(slot, CommitmentConfig::default())
    }

    pub fn get_leader_schedule_with_commitment(
        &self,
        slot: Option<Slot>,
        commitment_config: CommitmentConfig,
    ) -> io::Result<Option<RpcLeaderSchedule>> {
        let response = self
            .client
            .send(
                &RpcRequest::GetLeaderSchedule,
                json!([slot, commitment_config]),
                0,
            )
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetLeaderSchedule request failure: {:?}", err),
                )
            })?;

        serde_json::from_value(response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetLeaderSchedule failure: {:?}", err),
            )
        })
    }

    pub fn get_epoch_schedule(&self) -> io::Result<EpochSchedule> {
        let response = self
            .client
            .send(&RpcRequest::GetEpochSchedule, Value::Null, 0)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetEpochSchedule request failure: {:?}", err),
                )
            })?;

        serde_json::from_value(response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetEpochSchedule parse failure: {:?}", err),
            )
        })
    }

    pub fn get_inflation(&self) -> io::Result<Inflation> {
        let response = self
            .client
            .send(&RpcRequest::GetInflation, Value::Null, 0)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetInflation request failure: {:?}", err),
                )
            })?;

        serde_json::from_value(response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetInflation parse failure: {}", err),
            )
        })
    }

    pub fn get_version(&self) -> io::Result<RpcVersionInfo> {
        let response = self
            .client
            .send(&RpcRequest::GetVersion, Value::Null, 0)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetVersion request failure: {:?}", err),
                )
            })?;

        serde_json::from_value(response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetVersion parse failure: {:?}", err),
            )
        })
    }

    pub fn minimum_ledger_slot(&self) -> io::Result<Slot> {
        let response = self
            .client
            .send(&RpcRequest::MinimumLedgerSlot, Value::Null, 0)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("MinimumLedgerSlot request failure: {:?}", err),
                )
            })?;

        serde_json::from_value(response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("MinimumLedgerSlot parse failure: {:?}", err),
            )
        })
    }

    pub fn send_and_confirm_transaction<T: Signers>(
        &self,
        transaction: &mut Transaction,
        signer_keys: &T,
    ) -> Result<String, ClientError> {
        let mut send_retries = 20;
        loop {
            let mut status_retries = 15;
            let signature_str = self.send_transaction(transaction)?;
            let status = loop {
                let status = self.get_signature_status(&signature_str)?;
                if status.is_none() {
                    status_retries -= 1;
                    if status_retries == 0 {
                        break status;
                    }
                } else {
                    break status;
                }
                if cfg!(not(test)) {
                    // Retry twice a second
                    sleep(Duration::from_millis(500));
                }
            };
            send_retries = if let Some(result) = status.clone() {
                match result {
                    Ok(_) => return Ok(signature_str),
                    Err(TransactionError::AccountInUse) => {
                        // Fetch a new blockhash and re-sign the transaction before sending it again
                        self.resign_transaction(transaction, signer_keys)?;
                        send_retries - 1
                    }
                    Err(_) => 0,
                }
            } else {
                send_retries - 1
            };
            if send_retries == 0 {
                if let Some(err) = status {
                    return Err(err.unwrap_err().into());
                } else {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("Transaction {:?} failed: {:?}", signature_str, status),
                    )
                    .into());
                }
            }
        }
    }

    pub fn send_and_confirm_transactions<T: Signers>(
        &self,
        mut transactions: Vec<Transaction>,
        signer_keys: &T,
    ) -> Result<(), Box<dyn error::Error>> {
        let mut send_retries = 5;
        loop {
            let mut status_retries = 15;

            // Send all transactions
            let mut transactions_signatures = vec![];
            for transaction in transactions {
                if cfg!(not(test)) {
                    // Delay ~1 tick between write transactions in an attempt to reduce AccountInUse errors
                    // when all the write transactions modify the same program account (eg, deploying a
                    // new program)
                    sleep(Duration::from_millis(1000 / DEFAULT_TICKS_PER_SECOND));
                }

                let signature = self.send_transaction(&transaction).ok();
                transactions_signatures.push((transaction, signature))
            }

            // Collect statuses for all the transactions, drop those that are confirmed
            while status_retries > 0 {
                status_retries -= 1;

                if cfg!(not(test)) {
                    // Retry twice a second
                    sleep(Duration::from_millis(500));
                }

                transactions_signatures = transactions_signatures
                    .into_iter()
                    .filter(|(_transaction, signature)| {
                        if let Some(signature) = signature {
                            if let Ok(status) = self.get_signature_status(&signature) {
                                if status.is_none() {
                                    return false;
                                }
                                return status.unwrap().is_err();
                            }
                        }
                        true
                    })
                    .collect();

                if transactions_signatures.is_empty() {
                    return Ok(());
                }
            }

            if send_retries == 0 {
                return Err(io::Error::new(io::ErrorKind::Other, "Transactions failed").into());
            }
            send_retries -= 1;

            // Re-sign any failed transactions with a new blockhash and retry
            let (blockhash, _fee_calculator) =
                self.get_new_blockhash(&transactions_signatures[0].0.message().recent_blockhash)?;
            transactions = transactions_signatures
                .into_iter()
                .map(|(mut transaction, _)| {
                    transaction.sign(signer_keys, blockhash);
                    transaction
                })
                .collect();
        }
    }

    pub fn resign_transaction<T: Signers>(
        &self,
        tx: &mut Transaction,
        signer_keys: &T,
    ) -> Result<(), ClientError> {
        let (blockhash, _fee_calculator) =
            self.get_new_blockhash(&tx.message().recent_blockhash)?;
        tx.sign(signer_keys, blockhash);
        Ok(())
    }

    pub fn retry_get_balance(
        &self,
        pubkey: &Pubkey,
        retries: usize,
    ) -> Result<Option<u64>, Box<dyn error::Error>> {
        let balance_json = self
            .client
            .send(
                &RpcRequest::GetBalance,
                json!([pubkey.to_string()]),
                retries,
            )
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("RetryGetBalance request failure: {:?}", err),
                )
            })?;

        Ok(Some(
            serde_json::from_value::<Response<u64>>(balance_json)
                .map_err(|err| {
                    io::Error::new(
                        io::ErrorKind::Other,
                        format!("RetryGetBalance parse failure: {:?}", err),
                    )
                })?
                .value,
        ))
    }

    pub fn get_account(&self, pubkey: &Pubkey) -> io::Result<Account> {
        self.get_account_with_commitment(pubkey, CommitmentConfig::default())?
            .value
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("AccountNotFound: pubkey={}", pubkey),
                )
            })
    }

    pub fn get_account_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> RpcResponse<Option<Account>> {
        let response = self.client.send(
            &RpcRequest::GetAccountInfo,
            json!([pubkey.to_string(), commitment_config]),
            0,
        );

        response
            .map(|result_json| {
                if result_json.is_null() {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("AccountNotFound: pubkey={}", pubkey),
                    ));
                }
                let Response {
                    context,
                    value: rpc_account,
                } = serde_json::from_value::<Response<Option<RpcAccount>>>(result_json)?;
                trace!("Response account {:?} {:?}", pubkey, rpc_account);
                let account = rpc_account.and_then(|rpc_account| rpc_account.decode().ok());
                Ok(Response {
                    context,
                    value: account,
                })
            })
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("AccountNotFound: pubkey={}: {:?}", pubkey, err),
                )
            })?
    }

    pub fn get_account_data(&self, pubkey: &Pubkey) -> io::Result<Vec<u8>> {
        Ok(self.get_account(pubkey)?.data)
    }

    pub fn get_minimum_balance_for_rent_exemption(&self, data_len: usize) -> io::Result<u64> {
        let minimum_balance_json = self
            .client
            .send(
                &RpcRequest::GetMinimumBalanceForRentExemption,
                json!([data_len]),
                0,
            )
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!(
                        "GetMinimumBalanceForRentExemption request failure: {:?}",
                        err
                    ),
                )
            })?;

        let minimum_balance: u64 = serde_json::from_value(minimum_balance_json).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetMinimumBalanceForRentExemption parse failure: {:?}", err),
            )
        })?;
        trace!(
            "Response minimum balance {:?} {:?}",
            data_len,
            minimum_balance
        );
        Ok(minimum_balance)
    }

    /// Request the balance of the account `pubkey`.
    pub fn get_balance(&self, pubkey: &Pubkey) -> io::Result<u64> {
        Ok(self
            .get_balance_with_commitment(pubkey, CommitmentConfig::default())?
            .value)
    }

    pub fn get_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> RpcResponse<u64> {
        let balance_json = self
            .client
            .send(
                &RpcRequest::GetBalance,
                json!([pubkey.to_string(), commitment_config]),
                0,
            )
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetBalance request failure: {:?}", err),
                )
            })?;

        serde_json::from_value::<Response<u64>>(balance_json).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetBalance parse failure: {:?}", err),
            )
        })
    }

    pub fn get_program_accounts(&self, pubkey: &Pubkey) -> io::Result<Vec<(Pubkey, Account)>> {
        let response = self
            .client
            .send(
                &RpcRequest::GetProgramAccounts,
                json!([pubkey.to_string()]),
                0,
            )
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("AccountNotFound: pubkey={}: {:?}", pubkey, err),
                )
            })?;

        let accounts: Vec<RpcKeyedAccount> =
            serde_json::from_value::<Vec<RpcKeyedAccount>>(response).map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetProgramAccounts parse failure: {:?}", err),
                )
            })?;

        let mut pubkey_accounts: Vec<(Pubkey, Account)> = Vec::new();
        for RpcKeyedAccount { pubkey, account } in accounts.into_iter() {
            let pubkey = pubkey.parse().map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetProgramAccounts parse failure: {:?}", err),
                )
            })?;
            pubkey_accounts.push((pubkey, account.decode().unwrap()));
        }
        Ok(pubkey_accounts)
    }

    /// Request the transaction count.
    pub fn get_transaction_count(&self) -> io::Result<u64> {
        self.get_transaction_count_with_commitment(CommitmentConfig::default())
    }

    pub fn get_transaction_count_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> io::Result<u64> {
        let response = self
            .client
            .send(
                &RpcRequest::GetTransactionCount,
                json!([commitment_config]),
                0,
            )
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetTransactionCount request failure: {:?}", err),
                )
            })?;

        serde_json::from_value(response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetTransactionCount parse failure: {:?}", err),
            )
        })
    }

    pub fn get_recent_blockhash(&self) -> io::Result<(Hash, FeeCalculator)> {
        Ok(self
            .get_recent_blockhash_with_commitment(CommitmentConfig::default())?
            .value)
    }

    pub fn get_recent_blockhash_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> RpcResponse<(Hash, FeeCalculator)> {
        let response = self
            .client
            .send(
                &RpcRequest::GetRecentBlockhash,
                json!([commitment_config]),
                0,
            )
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetRecentBlockhash request failure: {:?}", err),
                )
            })?;

        let Response {
            context,
            value:
                RpcBlockhashFeeCalculator {
                    blockhash,
                    fee_calculator,
                },
        } = serde_json::from_value::<Response<RpcBlockhashFeeCalculator>>(response).map_err(
            |err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetRecentBlockhash parse failure: {:?}", err),
                )
            },
        )?;
        let blockhash = blockhash.parse().map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetRecentBlockhash hash parse failure: {:?}", err),
            )
        })?;
        Ok(Response {
            context,
            value: (blockhash, fee_calculator),
        })
    }

    pub fn get_new_blockhash(&self, blockhash: &Hash) -> io::Result<(Hash, FeeCalculator)> {
        let mut num_retries = 0;
        let start = Instant::now();
        while start.elapsed().as_secs() < 5 {
            if let Ok((new_blockhash, fee_calculator)) = self.get_recent_blockhash() {
                if new_blockhash != *blockhash {
                    return Ok((new_blockhash, fee_calculator));
                }
            }
            debug!("Got same blockhash ({:?}), will retry...", blockhash);

            // Retry ~twice during a slot
            sleep(Duration::from_millis(
                500 * DEFAULT_TICKS_PER_SLOT / DEFAULT_TICKS_PER_SECOND,
            ));
            num_retries += 1;
        }
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "Unable to get new blockhash after {}ms (retried {} times), stuck at {}",
                start.elapsed().as_millis(),
                num_retries,
                blockhash
            ),
        ))
    }

    pub fn get_genesis_hash(&self) -> io::Result<Hash> {
        let response = self
            .client
            .send(&RpcRequest::GetGenesisHash, Value::Null, 0)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetGenesisHash request failure: {:?}", err),
                )
            })?;

        let hash = serde_json::from_value::<String>(response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetGenesisHash parse failure: {:?}", err),
            )
        })?;

        let hash = hash.parse().map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetGenesisHash hash parse failure: {:?}", err),
            )
        })?;
        Ok(hash)
    }

    pub fn poll_balance_with_timeout_and_commitment(
        &self,
        pubkey: &Pubkey,
        polling_frequency: &Duration,
        timeout: &Duration,
        commitment_config: CommitmentConfig,
    ) -> io::Result<u64> {
        let now = Instant::now();
        loop {
            match self.get_balance_with_commitment(&pubkey, commitment_config.clone()) {
                Ok(bal) => {
                    return Ok(bal.value);
                }
                Err(e) => {
                    sleep(*polling_frequency);
                    if now.elapsed() > *timeout {
                        return Err(e);
                    }
                }
            };
        }
    }

    pub fn poll_get_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> io::Result<u64> {
        self.poll_balance_with_timeout_and_commitment(
            pubkey,
            &Duration::from_millis(100),
            &Duration::from_secs(1),
            commitment_config,
        )
    }

    pub fn wait_for_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        expected_balance: Option<u64>,
        commitment_config: CommitmentConfig,
    ) -> Option<u64> {
        const LAST: usize = 30;
        for run in 0..LAST {
            let balance_result =
                self.poll_get_balance_with_commitment(pubkey, commitment_config.clone());
            if expected_balance.is_none() {
                return balance_result.ok();
            }
            trace!(
                "retry_get_balance[{}] {:?} {:?}",
                run,
                balance_result,
                expected_balance
            );
            if let (Some(expected_balance), Ok(balance_result)) = (expected_balance, balance_result)
            {
                if expected_balance == balance_result {
                    return Some(balance_result);
                }
            }
        }
        None
    }

    /// Poll the server to confirm a transaction.
    pub fn poll_for_signature(&self, signature: &Signature) -> io::Result<()> {
        self.poll_for_signature_with_commitment(signature, CommitmentConfig::default())
    }

    /// Poll the server to confirm a transaction.
    pub fn poll_for_signature_with_commitment(
        &self,
        signature: &Signature,
        commitment_config: CommitmentConfig,
    ) -> io::Result<()> {
        let now = Instant::now();
        loop {
            if let Ok(Some(_)) = self.get_signature_status_with_commitment(
                &signature.to_string(),
                commitment_config.clone(),
            ) {
                break;
            }
            if now.elapsed().as_secs() > 15 {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    format!(
                        "signature not found after {} seconds",
                        now.elapsed().as_secs()
                    ),
                ));
            }
            sleep(Duration::from_millis(250));
        }
        Ok(())
    }

    /// Check a signature in the bank.
    pub fn check_signature(&self, signature: &Signature) -> bool {
        trace!("check_signature: {:?}", signature);

        for _ in 0..30 {
            let response = self.client.send(
                &RpcRequest::ConfirmTransaction,
                json!([signature.to_string(), CommitmentConfig::recent()]),
                0,
            );

            match response {
                Ok(Value::Bool(signature_status)) => {
                    if signature_status {
                        trace!("Response found signature");
                    } else {
                        trace!("Response signature not found");
                    }

                    return signature_status;
                }
                Ok(other) => {
                    debug!(
                        "check_signature request failed, expected bool, got: {:?}",
                        other
                    );
                }
                Err(err) => {
                    debug!("check_signature request failed: {:?}", err);
                }
            };
            sleep(Duration::from_millis(250));
        }

        panic!("Couldn't check signature of {}", signature);
    }

    /// Poll the server to confirm a transaction.
    pub fn poll_for_signature_confirmation(
        &self,
        signature: &Signature,
        min_confirmed_blocks: usize,
    ) -> io::Result<usize> {
        let mut now = Instant::now();
        let mut confirmed_blocks = 0;
        loop {
            let response = self.get_num_blocks_since_signature_confirmation(signature);
            match response {
                Ok(count) => {
                    if confirmed_blocks != count {
                        info!(
                            "signature {} confirmed {} out of {} after {} ms",
                            signature,
                            count,
                            min_confirmed_blocks,
                            now.elapsed().as_millis()
                        );
                        now = Instant::now();
                        confirmed_blocks = count;
                    }
                    if count >= min_confirmed_blocks {
                        break;
                    }
                }
                Err(err) => {
                    debug!("check_confirmations request failed: {:?}", err);
                }
            };
            if now.elapsed().as_secs() > 20 {
                info!(
                    "signature {} confirmed {} out of {} failed after {} ms",
                    signature,
                    confirmed_blocks,
                    min_confirmed_blocks,
                    now.elapsed().as_millis()
                );
                if confirmed_blocks > 0 {
                    return Ok(confirmed_blocks);
                } else {
                    return Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!(
                            "signature not found after {} seconds",
                            now.elapsed().as_secs()
                        ),
                    ));
                }
            }
            sleep(Duration::from_millis(250));
        }
        Ok(confirmed_blocks)
    }

    pub fn get_num_blocks_since_signature_confirmation(
        &self,
        signature: &Signature,
    ) -> io::Result<usize> {
        let response = self
            .client
            .send(
                &RpcRequest::GetNumBlocksSinceSignatureConfirmation,
                json!([signature.to_string(), CommitmentConfig::recent().ok()]),
                1,
            )
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!(
                        "GetNumBlocksSinceSignatureConfirmation request failure: {}",
                        err
                    ),
                )
            })?;
        serde_json::from_value(response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "GetNumBlocksSinceSignatureConfirmation parse failure: {}",
                    err
                ),
            )
        })
    }

    pub fn validator_exit(&self) -> io::Result<bool> {
        let response = self
            .client
            .send(&RpcRequest::ValidatorExit, Value::Null, 0)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("ValidatorExit request failure: {:?}", err),
                )
            })?;
        serde_json::from_value(response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("ValidatorExit parse failure: {:?}", err),
            )
        })
    }

    pub fn send(
        &self,
        request: &RpcRequest,
        params: Value,
        retries: usize,
    ) -> Result<Value, ClientError> {
        assert!(params.is_array() || params.is_null());
        self.client.send(request, params, retries)
    }
}

pub fn get_rpc_request_str(rpc_addr: SocketAddr, tls: bool) -> String {
    if tls {
        format!("https://{}", rpc_addr)
    } else {
        format!("http://{}", rpc_addr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock_rpc_client_request::{PUBKEY, SIGNATURE};
    use assert_matches::assert_matches;
    use jsonrpc_core::{Error, IoHandler, Params};
    use jsonrpc_http_server::{AccessControlAllowOrigin, DomainsValidation, ServerBuilder};
    use serde_json::Number;
    use solana_logger;
    use solana_sdk::{
        instruction::InstructionError, signature::Keypair, system_transaction,
        transaction::TransactionError,
    };
    use std::{sync::mpsc::channel, thread};

    #[test]
    fn test_send() {
        let (sender, receiver) = channel();
        thread::spawn(move || {
            let rpc_addr = "0.0.0.0:0".parse().unwrap();
            let mut io = IoHandler::default();
            // Successful request
            io.add_method("getBalance", |_params: Params| {
                Ok(Value::Number(Number::from(50)))
            });
            // Failed request
            io.add_method("getRecentBlockhash", |params: Params| {
                if params != Params::None {
                    Err(Error::invalid_request())
                } else {
                    Ok(Value::String(
                        "deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhx".to_string(),
                    ))
                }
            });

            let server = ServerBuilder::new(io)
                .threads(1)
                .cors(DomainsValidation::AllowOnly(vec![
                    AccessControlAllowOrigin::Any,
                ]))
                .start_http(&rpc_addr)
                .expect("Unable to start RPC server");
            sender.send(*server.address()).unwrap();
            server.wait();
        });

        let rpc_addr = receiver.recv().unwrap();
        let rpc_client = RpcClient::new_socket(rpc_addr);

        let balance = rpc_client.send(
            &RpcRequest::GetBalance,
            json!(["deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhx"]),
            0,
        );
        assert_eq!(balance.unwrap().as_u64().unwrap(), 50);

        let blockhash = rpc_client.send(&RpcRequest::GetRecentBlockhash, Value::Null, 0);
        assert_eq!(
            blockhash.unwrap().as_str().unwrap(),
            "deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhx"
        );

        // Send erroneous parameter
        let blockhash = rpc_client.send(&RpcRequest::GetRecentBlockhash, json!(["parameter"]), 0);
        assert_eq!(blockhash.is_err(), true);
    }

    #[test]
    fn test_retry_send() {
        solana_logger::setup();
        let (sender, receiver) = channel();
        thread::spawn(move || {
            // 1. Pick a random port
            // 2. Tell the client to start using it
            // 3. Delay for 1.5 seconds before starting the server to ensure the client will fail
            //    and need to retry
            let rpc_addr: SocketAddr = "0.0.0.0:4242".parse().unwrap();
            sender.send(rpc_addr.clone()).unwrap();
            sleep(Duration::from_millis(1500));

            let mut io = IoHandler::default();
            io.add_method("getBalance", move |_params: Params| {
                Ok(Value::Number(Number::from(5)))
            });
            let server = ServerBuilder::new(io)
                .threads(1)
                .cors(DomainsValidation::AllowOnly(vec![
                    AccessControlAllowOrigin::Any,
                ]))
                .start_http(&rpc_addr)
                .expect("Unable to start RPC server");
            server.wait();
        });

        let rpc_addr = receiver.recv().unwrap();
        let rpc_client = RpcClient::new_socket(rpc_addr);

        let balance = rpc_client.send(
            &RpcRequest::GetBalance,
            json!(["deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhw"]),
            10,
        );
        assert_eq!(balance.unwrap().as_u64().unwrap(), 5);
    }

    #[test]
    fn test_send_transaction() {
        let rpc_client = RpcClient::new_mock("succeeds".to_string());

        let key = Keypair::new();
        let to = Pubkey::new_rand();
        let blockhash = Hash::default();
        let tx = system_transaction::transfer(&key, &to, 50, blockhash);

        let signature = rpc_client.send_transaction(&tx);
        assert_eq!(signature.unwrap(), SIGNATURE.to_string());

        let rpc_client = RpcClient::new_mock("fails".to_string());

        let signature = rpc_client.send_transaction(&tx);
        assert!(signature.is_err());
    }
    #[test]
    fn test_get_recent_blockhash() {
        let rpc_client = RpcClient::new_mock("succeeds".to_string());

        let expected_blockhash: Hash = PUBKEY.parse().unwrap();

        let (blockhash, _fee_calculator) = rpc_client.get_recent_blockhash().expect("blockhash ok");
        assert_eq!(blockhash, expected_blockhash);

        let rpc_client = RpcClient::new_mock("fails".to_string());

        assert!(rpc_client.get_recent_blockhash().is_err());
    }

    #[test]
    fn test_get_signature_status() {
        let rpc_client = RpcClient::new_mock("succeeds".to_string());
        let signature = "good_signature";
        let status = rpc_client.get_signature_status(&signature).unwrap();
        assert_eq!(status, Some(Ok(())));

        let rpc_client = RpcClient::new_mock("sig_not_found".to_string());
        let signature = "sig_not_found";
        let status = rpc_client.get_signature_status(&signature).unwrap();
        assert_eq!(status, None);

        let rpc_client = RpcClient::new_mock("account_in_use".to_string());
        let signature = "account_in_use";
        let status = rpc_client.get_signature_status(&signature).unwrap();
        assert_eq!(status, Some(Err(TransactionError::AccountInUse)));
    }

    #[test]
    fn test_send_and_confirm_transaction() {
        let rpc_client = RpcClient::new_mock("succeeds".to_string());

        let key = Keypair::new();
        let to = Pubkey::new_rand();
        let blockhash = Hash::default();
        let mut tx = system_transaction::transfer(&key, &to, 50, blockhash);

        let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&key]);
        result.unwrap();

        let rpc_client = RpcClient::new_mock("account_in_use".to_string());
        let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&key]);
        assert!(result.is_err());

        let rpc_client = RpcClient::new_mock("instruction_error".to_string());
        let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&key]);
        assert_matches!(
            result.unwrap_err(),
            ClientError::TransactionError(TransactionError::InstructionError(
                0,
                InstructionError::UninitializedAccount
            ))
        );

        let rpc_client = RpcClient::new_mock("sig_not_found".to_string());
        let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&key]);
        if let ClientError::Io(err) = result.unwrap_err() {
            assert_eq!(err.kind(), io::ErrorKind::Other);
        }
    }

    #[test]
    fn test_resign_transaction() {
        let rpc_client = RpcClient::new_mock("succeeds".to_string());

        let key = Keypair::new();
        let to = Pubkey::new_rand();
        let blockhash: Hash = "HUu3LwEzGRsUkuJS121jzkPJW39Kq62pXCTmTa1F9jDL"
            .parse()
            .unwrap();
        let prev_tx = system_transaction::transfer(&key, &to, 50, blockhash);
        let mut tx = system_transaction::transfer(&key, &to, 50, blockhash);

        rpc_client.resign_transaction(&mut tx, &[&key]).unwrap();

        assert_ne!(prev_tx, tx);
        assert_ne!(prev_tx.signatures, tx.signatures);
        assert_ne!(
            prev_tx.message().recent_blockhash,
            tx.message().recent_blockhash
        );
    }

    #[test]
    fn test_rpc_client_thread() {
        let rpc_client = RpcClient::new_mock("succeeds".to_string());
        thread::spawn(move || rpc_client);
    }
}
