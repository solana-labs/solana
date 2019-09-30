use crate::client_error::ClientError;
use crate::generic_rpc_client_request::GenericRpcClientRequest;
use crate::mock_rpc_client_request::MockRpcClientRequest;
use crate::rpc_client_request::RpcClientRequest;
use crate::rpc_request::{RpcEpochInfo, RpcRequest};
use bincode::serialize;
use log::*;
use serde_json::{json, Value};
use solana_sdk::{
    account::Account,
    clock::{Slot, DEFAULT_TICKS_PER_SECOND, DEFAULT_TICKS_PER_SLOT},
    fee_calculator::FeeCalculator,
    hash::Hash,
    inflation::Inflation,
    pubkey::Pubkey,
    signature::{KeypairUtil, Signature},
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

    pub fn new_socket(addr: SocketAddr) -> Self {
        Self::new(get_rpc_request_str(addr, false))
    }

    pub fn new_socket_with_timeout(addr: SocketAddr, timeout: Duration) -> Self {
        let url = get_rpc_request_str(addr, false);
        Self {
            client: Box::new(RpcClientRequest::new_with_timeout(url, timeout)),
        }
    }

    pub fn send_transaction(&self, transaction: &Transaction) -> Result<String, ClientError> {
        let serialized = serialize(transaction).unwrap();
        let params = json!([serialized]);
        let signature = self
            .client
            .send(&RpcRequest::SendTransaction, Some(params), 5)?;
        if signature.as_str().is_none() {
            Err(io::Error::new(
                io::ErrorKind::Other,
                "Received result of an unexpected type",
            ))?;
        }
        Ok(signature.as_str().unwrap().to_string())
    }

    pub fn get_signature_status(
        &self,
        signature: &str,
    ) -> Result<Option<transaction::Result<()>>, ClientError> {
        let params = json!([signature.to_string()]);
        let signature_status =
            self.client
                .send(&RpcRequest::GetSignatureStatus, Some(params), 5)?;
        let result: Option<transaction::Result<()>> =
            serde_json::from_value(signature_status).unwrap();
        Ok(result)
    }

    pub fn get_slot(&self) -> io::Result<Slot> {
        let response = self
            .client
            .send(&RpcRequest::GetSlot, None, 0)
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

    pub fn get_epoch_info(&self) -> io::Result<RpcEpochInfo> {
        let response = self
            .client
            .send(&RpcRequest::GetEpochInfo, None, 0)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetEpochInfo request failure: {:?}", err),
                )
            })?;

        serde_json::from_value(response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetEpochInfo parse failure: {}", err),
            )
        })
    }

    pub fn get_inflation(&self) -> io::Result<Inflation> {
        let response = self
            .client
            .send(&RpcRequest::GetInflation, None, 0)
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

    pub fn get_version(&self) -> io::Result<String> {
        let response = self
            .client
            .send(&RpcRequest::GetVersion, None, 0)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetVersion request failure: {:?}", err),
                )
            })?;

        serde_json::to_string(&response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetVersion parse failure: {}", err),
            )
        })
    }

    pub fn send_and_confirm_transaction<T: KeypairUtil>(
        &self,
        transaction: &mut Transaction,
        signer_keys: &[&T],
    ) -> Result<String, ClientError> {
        let mut send_retries = 5;
        loop {
            let mut status_retries = 4;
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
                    // Retry ~twice during a slot
                    sleep(Duration::from_millis(
                        500 * DEFAULT_TICKS_PER_SLOT / DEFAULT_TICKS_PER_SECOND,
                    ));
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
                if status.is_some() {
                    status.unwrap()?
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::Other,
                        format!("Transaction {:?} failed: {:?}", signature_str, status),
                    ))?;
                }
            }
        }
    }

    pub fn send_and_confirm_transactions<T: KeypairUtil>(
        &self,
        mut transactions: Vec<Transaction>,
        signer_keys: &[&T],
    ) -> Result<(), Box<dyn error::Error>> {
        let mut send_retries = 5;
        loop {
            let mut status_retries = 4;

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
                    // Retry ~twice during a slot
                    sleep(Duration::from_millis(
                        500 * DEFAULT_TICKS_PER_SLOT / DEFAULT_TICKS_PER_SECOND,
                    ));
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
                Err(io::Error::new(io::ErrorKind::Other, "Transactions failed"))?;
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

    pub fn resign_transaction<T: KeypairUtil>(
        &self,
        tx: &mut Transaction,
        signer_keys: &[&T],
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
        let params = json!([format!("{}", pubkey)]);
        let res = self
            .client
            .send(&RpcRequest::GetBalance, Some(params), retries)?
            .as_u64();
        Ok(res)
    }

    pub fn get_account(&self, pubkey: &Pubkey) -> io::Result<Account> {
        let params = json!([format!("{}", pubkey)]);
        let response = self
            .client
            .send(&RpcRequest::GetAccountInfo, Some(params), 0);

        response
            .and_then(|account_json| {
                let account: Account = serde_json::from_value(account_json)?;
                trace!("Response account {:?} {:?}", pubkey, account);
                Ok(account)
            })
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("AccountNotFound: pubkey={}: {}", pubkey, err),
                )
            })
    }

    pub fn get_account_data(&self, pubkey: &Pubkey) -> io::Result<Vec<u8>> {
        self.get_account(pubkey).map(|account| account.data)
    }

    pub fn get_minimum_balance_for_rent_exemption(&self, data_len: usize) -> io::Result<u64> {
        let params = json!([data_len]);
        let minimum_balance_json = self
            .client
            .send(
                &RpcRequest::GetMinimumBalanceForRentExemption,
                Some(params),
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

        let minimum_balance: u64 =
            serde_json::from_value(minimum_balance_json).expect("deserialize minimum_balance");
        trace!(
            "Response minimum balance {:?} {:?}",
            data_len,
            minimum_balance
        );
        Ok(minimum_balance)
    }

    /// Request the balance of the user holding `pubkey`. This method blocks
    /// until the server sends a response. If the response packet is dropped
    /// by the network, this method will hang indefinitely.
    pub fn get_balance(&self, pubkey: &Pubkey) -> io::Result<u64> {
        self.get_account(pubkey).map(|account| account.lamports)
    }

    pub fn get_program_accounts(&self, pubkey: &Pubkey) -> io::Result<Vec<(Pubkey, Account)>> {
        let params = json!([format!("{}", pubkey)]);
        let response = self
            .client
            .send(&RpcRequest::GetProgramAccounts, Some(params), 0)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("AccountNotFound: pubkey={}: {}", pubkey, err),
                )
            })?;

        let accounts: Vec<(String, Account)> =
            serde_json::from_value::<Vec<(String, Account)>>(response).map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetProgramAccounts parse failure: {:?}", err),
                )
            })?;

        let mut pubkey_accounts: Vec<(Pubkey, Account)> = Vec::new();
        for (string, account) in accounts.into_iter() {
            let pubkey = string.parse().map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetProgramAccounts parse failure: {:?}", err),
                )
            })?;
            pubkey_accounts.push((pubkey, account));
        }
        Ok(pubkey_accounts)
    }

    /// Request the transaction count.  If the response packet is dropped by the network,
    /// this method will try again 5 times.
    pub fn get_transaction_count(&self) -> io::Result<u64> {
        let response = self
            .client
            .send(&RpcRequest::GetTransactionCount, None, 0)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetTransactionCount request failure: {:?}", err),
                )
            })?;

        serde_json::from_value(response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetTransactionCount parse failure: {}", err),
            )
        })
    }

    pub fn get_recent_blockhash(&self) -> io::Result<(Hash, FeeCalculator)> {
        let response = self
            .client
            .send(&RpcRequest::GetRecentBlockhash, None, 0)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetRecentBlockhash request failure: {:?}", err),
                )
            })?;

        let (blockhash, fee_calculator) =
            serde_json::from_value::<(String, FeeCalculator)>(response).map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetRecentBlockhash parse failure: {:?}", err),
                )
            })?;

        let blockhash = blockhash.parse().map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetRecentBlockhash hash parse failure: {:?}", err),
            )
        })?;
        Ok((blockhash, fee_calculator))
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

    pub fn get_genesis_blockhash(&self) -> io::Result<Hash> {
        let response = self
            .client
            .send(&RpcRequest::GetGenesisBlockhash, None, 0)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("GetGenesisBlockhash request failure: {:?}", err),
                )
            })?;

        let blockhash = serde_json::from_value::<String>(response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetGenesisBlockhash parse failure: {:?}", err),
            )
        })?;

        let blockhash = blockhash.parse().map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("GetGenesisBlockhash hash parse failure: {:?}", err),
            )
        })?;
        Ok(blockhash)
    }

    pub fn poll_balance_with_timeout(
        &self,
        pubkey: &Pubkey,
        polling_frequency: &Duration,
        timeout: &Duration,
    ) -> io::Result<u64> {
        let now = Instant::now();
        loop {
            match self.get_balance(&pubkey) {
                Ok(bal) => {
                    return Ok(bal);
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

    pub fn poll_get_balance(&self, pubkey: &Pubkey) -> io::Result<u64> {
        self.poll_balance_with_timeout(pubkey, &Duration::from_millis(100), &Duration::from_secs(1))
    }

    pub fn wait_for_balance(&self, pubkey: &Pubkey, expected_balance: Option<u64>) -> Option<u64> {
        const LAST: usize = 30;
        for run in 0..LAST {
            let balance_result = self.poll_get_balance(pubkey);
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
        let now = Instant::now();
        while !self.check_signature(signature) {
            if now.elapsed().as_secs() > 15 {
                // TODO: Return a better error.
                return Err(io::Error::new(io::ErrorKind::Other, "signature not found"));
            }
            sleep(Duration::from_millis(250));
        }
        Ok(())
    }

    /// Check a signature in the bank.
    pub fn check_signature(&self, signature: &Signature) -> bool {
        trace!("check_signature: {:?}", signature);
        let params = json!([format!("{}", signature)]);

        for _ in 0..30 {
            let response =
                self.client
                    .send(&RpcRequest::ConfirmTransaction, Some(params.clone()), 0);

            match response {
                Ok(confirmation) => {
                    let signature_status = confirmation.as_bool().unwrap();
                    if signature_status {
                        trace!("Response found signature");
                    } else {
                        trace!("Response signature not found");
                    }

                    return signature_status;
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
            if now.elapsed().as_secs() > 15 {
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
                    // TODO: Return a better error.
                    return Err(io::Error::new(io::ErrorKind::Other, "signature not found"));
                }
            }
            sleep(Duration::from_millis(250));
        }
        Ok(confirmed_blocks)
    }

    pub fn get_num_blocks_since_signature_confirmation(
        &self,
        sig: &Signature,
    ) -> io::Result<usize> {
        let params = json!([format!("{}", sig)]);
        let response = self
            .client
            .send(
                &RpcRequest::GetNumBlocksSinceSignatureConfirmation,
                Some(params.clone()),
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

    pub fn fullnode_exit(&self) -> io::Result<bool> {
        let response = self
            .client
            .send(&RpcRequest::FullnodeExit, None, 0)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::Other,
                    format!("FullnodeExit request failure: {:?}", err),
                )
            })?;
        serde_json::from_value(response).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("FullnodeExit parse failure: {:?}", err),
            )
        })
    }

    // TODO: Remove
    pub fn retry_make_rpc_request(
        &self,
        request: &RpcRequest,
        params: Option<Value>,
        retries: usize,
    ) -> Result<Value, ClientError> {
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
    use jsonrpc_core::{Error, IoHandler, Params};
    use jsonrpc_http_server::{AccessControlAllowOrigin, DomainsValidation, ServerBuilder};
    use serde_json::Number;
    use solana_logger;
    use solana_sdk::signature::{Keypair, KeypairUtil};
    use solana_sdk::system_transaction;
    use solana_sdk::transaction::TransactionError;
    use std::sync::mpsc::channel;
    use std::thread;

    #[test]
    fn test_make_rpc_request() {
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

        let balance = rpc_client.retry_make_rpc_request(
            &RpcRequest::GetBalance,
            Some(json!(["deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhx"])),
            0,
        );
        assert_eq!(balance.unwrap().as_u64().unwrap(), 50);

        let blockhash = rpc_client.retry_make_rpc_request(&RpcRequest::GetRecentBlockhash, None, 0);
        assert_eq!(
            blockhash.unwrap().as_str().unwrap(),
            "deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhx"
        );

        // Send erroneous parameter
        let blockhash = rpc_client.retry_make_rpc_request(
            &RpcRequest::GetRecentBlockhash,
            Some(json!("parameter")),
            0,
        );
        assert_eq!(blockhash.is_err(), true);
    }

    #[test]
    fn test_retry_make_rpc_request() {
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

        let balance = rpc_client.retry_make_rpc_request(
            &RpcRequest::GetBalance,
            Some(json!(["deadbeefXjn8o3yroDHxUtKsZZgoy4GPkPPXfouKNHhw"])),
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
        let tx = system_transaction::create_user_account(&key, &to, 50, blockhash);

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
        let mut tx = system_transaction::create_user_account(&key, &to, 50, blockhash);

        let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&key]);
        result.unwrap();

        let rpc_client = RpcClient::new_mock("account_in_use".to_string());
        let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&key]);
        assert!(result.is_err());

        let rpc_client = RpcClient::new_mock("fails".to_string());
        let result = rpc_client.send_and_confirm_transaction(&mut tx, &[&key]);
        assert!(result.is_err());
    }

    #[test]
    fn test_resign_transaction() {
        let rpc_client = RpcClient::new_mock("succeeds".to_string());

        let key = Keypair::new();
        let to = Pubkey::new_rand();
        let blockhash: Hash = "HUu3LwEzGRsUkuJS121jzkPJW39Kq62pXCTmTa1F9jDL"
            .parse()
            .unwrap();
        let prev_tx = system_transaction::create_user_account(&key, &to, 50, blockhash);
        let mut tx = system_transaction::create_user_account(&key, &to, 50, blockhash);

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
