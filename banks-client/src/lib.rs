//! A client for the ledger state, from the perspective of an arbitrary validator.
//!
//! Use start_tcp_client() to create a client and then import BanksClientExt to
//! access its methods. Additional "*_with_context" methods are also available,
//! but they are undocumented, may change over time, and are generally more
//! cumbersome to use.

use async_trait::async_trait;
use futures::future::join_all;
pub use solana_banks_interface::{BanksClient, TransactionStatus};
use solana_banks_interface::{BanksRequest, BanksResponse};
use solana_sdk::{
    account::Account, clock::Slot, commitment_config::CommitmentLevel,
    fee_calculator::FeeCalculator, hash::Hash, pubkey::Pubkey, signature::Signature,
    transaction::Transaction, transport,
};
use std::io::{self, Error, ErrorKind};
use tarpc::{
    client, context,
    rpc::{transport::channel::UnboundedChannel, ClientMessage, Response},
    serde_transport::tcp,
};
use tokio::{net::ToSocketAddrs, time::Duration};
use tokio_serde::formats::Bincode;

#[async_trait]
pub trait BanksClientExt {
    /// Send a transaction and return immediately. The server will resend the
    /// transaction until either it is accepted by the cluster or the transaction's
    /// blockhash expires.
    async fn send_transaction(&mut self, transaction: Transaction) -> io::Result<()>;

    /// Return a recent, rooted blockhash from the server. The cluster will only accept
    /// transactions with a blockhash that has not yet expired. Use the `get_fees`
    /// method to get both a blockhash and the blockhash's last valid slot.
    async fn get_recent_blockhash(&mut self) -> io::Result<Hash>;

    /// Return the fee parameters associated with a recent, rooted blockhash. The cluster
    /// will use the transaction's blockhash to look up these same fee parameters and
    /// use them to calculate the transaction fee.
    async fn get_fees(&mut self) -> io::Result<(FeeCalculator, Hash, Slot)>;

    /// Send a transaction and return after the transaction has been rejected or
    /// reached the given level of commitment.
    async fn process_transaction_with_commitment(
        &mut self,
        transaction: Transaction,
        commitment: CommitmentLevel,
    ) -> transport::Result<()>;

    /// Send a transaction and return after the transaction has been finalized or rejected.
    async fn process_transaction(&mut self, transaction: Transaction) -> transport::Result<()>;

    /// Return the status of a transaction with a signature matching the transaction's first
    /// signature. Return None if the transaction is not found, which may be because the
    /// blockhash was expired or the fee-paying account had insufficient funds to pay the
    /// transaction fee. Note that servers rarely store the full transaction history. This
    /// method may return None if the transaction status has been discarded.
    async fn get_transaction_status(
        &mut self,
        signature: Signature,
    ) -> io::Result<Option<TransactionStatus>>;

    /// Same as get_transaction_status, but for multiple transactions.
    async fn get_transaction_statuses(
        &mut self,
        signatures: Vec<Signature>,
    ) -> io::Result<Vec<Option<TransactionStatus>>>;

    /// Return the most recent rooted slot height. All transactions at or below this height
    /// are said to be finalized. The cluster will not fork to a higher slot height.
    async fn get_root_slot(&mut self) -> io::Result<Slot>;

    /// Return the account at the given address at the slot corresponding to the given
    /// commitment level. If the account is not found, None is returned.
    async fn get_account_with_commitment(
        &mut self,
        address: Pubkey,
        commitment: CommitmentLevel,
    ) -> io::Result<Option<Account>>;

    /// Return the account at the given address at the time of the most recent root slot.
    /// If the account is not found, None is returned.
    async fn get_account(&mut self, address: Pubkey) -> io::Result<Option<Account>>;

    /// Return the balance in lamports of an account at the given address at the slot
    /// corresponding to the given commitment level.
    async fn get_balance_with_commitment(
        &mut self,
        address: Pubkey,
        commitment: CommitmentLevel,
    ) -> io::Result<u64>;

    /// Return the balance in lamports of an account at the given address at the time
    /// of the most recent root slot.
    async fn get_balance(&mut self, address: Pubkey) -> io::Result<u64>;
}

#[async_trait]
impl BanksClientExt for BanksClient {
    async fn send_transaction(&mut self, transaction: Transaction) -> io::Result<()> {
        self.send_transaction_with_context(context::current(), transaction)
            .await
    }

    async fn get_fees(&mut self) -> io::Result<(FeeCalculator, Hash, Slot)> {
        self.get_fees_with_commitment_and_context(context::current(), CommitmentLevel::Root)
            .await
    }

    async fn get_recent_blockhash(&mut self) -> io::Result<Hash> {
        Ok(self.get_fees().await?.1)
    }

    async fn process_transaction_with_commitment(
        &mut self,
        transaction: Transaction,
        commitment: CommitmentLevel,
    ) -> transport::Result<()> {
        let mut ctx = context::current();
        ctx.deadline += Duration::from_secs(50);
        let result = self
            .process_transaction_with_commitment_and_context(ctx, transaction, commitment)
            .await?;
        match result {
            None => Err(Error::new(ErrorKind::TimedOut, "invalid blockhash or fee-payer").into()),
            Some(transaction_result) => Ok(transaction_result?),
        }
    }

    async fn process_transaction(&mut self, transaction: Transaction) -> transport::Result<()> {
        self.process_transaction_with_commitment(transaction, CommitmentLevel::default())
            .await
    }

    async fn get_root_slot(&mut self) -> io::Result<Slot> {
        self.get_slot_with_context(context::current(), CommitmentLevel::Root)
            .await
    }

    async fn get_account_with_commitment(
        &mut self,
        address: Pubkey,
        commitment: CommitmentLevel,
    ) -> io::Result<Option<Account>> {
        self.get_account_with_commitment_and_context(context::current(), address, commitment)
            .await
    }

    async fn get_account(&mut self, address: Pubkey) -> io::Result<Option<Account>> {
        self.get_account_with_commitment(address, CommitmentLevel::default())
            .await
    }

    async fn get_balance_with_commitment(
        &mut self,
        address: Pubkey,
        commitment: CommitmentLevel,
    ) -> io::Result<u64> {
        let account = self
            .get_account_with_commitment_and_context(context::current(), address, commitment)
            .await?;
        Ok(account.map(|x| x.lamports).unwrap_or(0))
    }

    async fn get_balance(&mut self, address: Pubkey) -> io::Result<u64> {
        self.get_balance_with_commitment(address, CommitmentLevel::default())
            .await
    }

    async fn get_transaction_status(
        &mut self,
        signature: Signature,
    ) -> io::Result<Option<TransactionStatus>> {
        self.get_transaction_status_with_context(context::current(), signature)
            .await
    }

    async fn get_transaction_statuses(
        &mut self,
        signatures: Vec<Signature>,
    ) -> io::Result<Vec<Option<TransactionStatus>>> {
        // tarpc futures oddly hold a mutable reference back to the client so clone the client upfront
        let mut clients_and_signatures: Vec<_> = signatures
            .into_iter()
            .map(|signature| (self.clone(), signature))
            .collect();

        let futs = clients_and_signatures
            .iter_mut()
            .map(|(client, signature)| client.get_transaction_status(*signature));

        let statuses = join_all(futs).await;

        // Convert Vec<Result<_, _>> to Result<Vec<_>>
        statuses.into_iter().collect()
    }
}

pub async fn start_client(
    transport: UnboundedChannel<Response<BanksResponse>, ClientMessage<BanksRequest>>,
) -> io::Result<BanksClient> {
    BanksClient::new(client::Config::default(), transport).spawn()
}

pub async fn start_tcp_client<T: ToSocketAddrs>(addr: T) -> io::Result<BanksClient> {
    let transport = tcp::connect(addr, Bincode::default).await?;
    BanksClient::new(client::Config::default(), transport).spawn()
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_banks_server::banks_server::start_local_server;
    use solana_runtime::{bank::Bank, bank_forks::BankForks, genesis_utils::create_genesis_config};
    use solana_sdk::{message::Message, signature::Signer, system_instruction};
    use std::sync::{Arc, RwLock};
    use tarpc::transport;
    use tokio::{runtime::Runtime, time::sleep};

    #[test]
    fn test_banks_client_new() {
        let (client_transport, _server_transport) = transport::channel::unbounded();
        BanksClient::new(client::Config::default(), client_transport);
    }

    #[test]
    fn test_banks_server_transfer_via_server() -> io::Result<()> {
        // This test shows the preferred way to interact with BanksServer.
        // It creates a runtime explicitly (no globals via tokio macros) and calls
        // `runtime.block_on()` just once, to run all the async code.

        let genesis = create_genesis_config(10);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(Bank::new(
            &genesis.genesis_config,
        ))));

        let bob_pubkey = solana_sdk::pubkey::new_rand();
        let mint_pubkey = genesis.mint_keypair.pubkey();
        let instruction = system_instruction::transfer(&mint_pubkey, &bob_pubkey, 1);
        let message = Message::new(&[instruction], Some(&mint_pubkey));

        Runtime::new()?.block_on(async {
            let client_transport = start_local_server(&bank_forks).await;
            let mut banks_client =
                BanksClient::new(client::Config::default(), client_transport).spawn()?;

            let recent_blockhash = banks_client.get_recent_blockhash().await?;
            let transaction = Transaction::new(&[&genesis.mint_keypair], message, recent_blockhash);
            banks_client.process_transaction(transaction).await.unwrap();
            assert_eq!(banks_client.get_balance(bob_pubkey).await?, 1);
            Ok(())
        })
    }

    #[test]
    fn test_banks_server_transfer_via_client() -> io::Result<()> {
        // The caller may not want to hold the connection open until the transaction
        // is processed (or blockhash expires). In this test, we verify the
        // server-side functionality is available to the client.

        let genesis = create_genesis_config(10);
        let bank_forks = Arc::new(RwLock::new(BankForks::new(Bank::new(
            &genesis.genesis_config,
        ))));

        let mint_pubkey = &genesis.mint_keypair.pubkey();
        let bob_pubkey = solana_sdk::pubkey::new_rand();
        let instruction = system_instruction::transfer(&mint_pubkey, &bob_pubkey, 1);
        let message = Message::new(&[instruction], Some(&mint_pubkey));

        Runtime::new()?.block_on(async {
            let client_transport = start_local_server(&bank_forks).await;
            let mut banks_client =
                BanksClient::new(client::Config::default(), client_transport).spawn()?;
            let (_, recent_blockhash, last_valid_slot) = banks_client.get_fees().await?;
            let transaction = Transaction::new(&[&genesis.mint_keypair], message, recent_blockhash);
            let signature = transaction.signatures[0];
            banks_client.send_transaction(transaction).await?;

            let mut status = banks_client.get_transaction_status(signature).await?;

            while status.is_none() {
                let root_slot = banks_client.get_root_slot().await?;
                if root_slot > last_valid_slot {
                    break;
                }
                sleep(Duration::from_millis(100)).await;
                status = banks_client.get_transaction_status(signature).await?;
            }
            assert!(status.unwrap().err.is_none());
            assert_eq!(banks_client.get_balance(bob_pubkey).await?, 1);
            Ok(())
        })
    }
}
