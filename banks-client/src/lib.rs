//! A client for the ledger state, from the perspective of an arbitrary validator.
//!
//! Use start_tcp_client() to create a client and then import BanksClientExt to
//! access its methods. Additional "*_with_context" methods are also available,
//! but they are undocumented, may change over time, and are generally more
//! cumbersome to use.

pub use {
    crate::error::BanksClientError,
    solana_banks_interface::{BanksClient as TarpcClient, TransactionStatus},
};
use {
    borsh::BorshDeserialize,
    futures::future::join_all,
    solana_banks_interface::{
        BanksRequest, BanksResponse, BanksTransactionResultWithMetadata,
        BanksTransactionResultWithSimulation,
    },
    solana_program::{
        clock::Slot, hash::Hash, program_pack::Pack, pubkey::Pubkey, rent::Rent, sysvar::Sysvar,
    },
    solana_sdk::{
        account::{from_account, Account},
        commitment_config::CommitmentLevel,
        message::Message,
        signature::Signature,
        transaction::{self, VersionedTransaction},
    },
    tarpc::{
        client::{self, NewClient, RequestDispatch},
        context::{self, Context},
        serde_transport::tcp,
        ClientMessage, Response, Transport,
    },
    tokio::net::ToSocketAddrs,
    tokio_serde::formats::Bincode,
};

mod error;

// This exists only for backward compatibility
pub trait BanksClientExt {}

#[derive(Clone)]
pub struct BanksClient {
    inner: TarpcClient,
}

impl BanksClient {
    #[allow(clippy::new_ret_no_self)]
    pub fn new<C>(
        config: client::Config,
        transport: C,
    ) -> NewClient<TarpcClient, RequestDispatch<BanksRequest, BanksResponse, C>>
    where
        C: Transport<ClientMessage<BanksRequest>, Response<BanksResponse>>,
    {
        TarpcClient::new(config, transport)
    }

    pub async fn send_transaction_with_context(
        &self,
        ctx: Context,
        transaction: impl Into<VersionedTransaction>,
    ) -> Result<(), BanksClientError> {
        self.inner
            .send_transaction_with_context(ctx, transaction.into())
            .await
            .map_err(Into::into)
    }

    pub async fn get_transaction_status_with_context(
        &self,
        ctx: Context,
        signature: Signature,
    ) -> Result<Option<TransactionStatus>, BanksClientError> {
        self.inner
            .get_transaction_status_with_context(ctx, signature)
            .await
            .map_err(Into::into)
    }

    pub async fn get_slot_with_context(
        &self,
        ctx: Context,
        commitment: CommitmentLevel,
    ) -> Result<Slot, BanksClientError> {
        self.inner
            .get_slot_with_context(ctx, commitment)
            .await
            .map_err(Into::into)
    }

    pub async fn get_block_height_with_context(
        &self,
        ctx: Context,
        commitment: CommitmentLevel,
    ) -> Result<Slot, BanksClientError> {
        self.inner
            .get_block_height_with_context(ctx, commitment)
            .await
            .map_err(Into::into)
    }

    pub async fn process_transaction_with_commitment_and_context(
        &self,
        ctx: Context,
        transaction: impl Into<VersionedTransaction>,
        commitment: CommitmentLevel,
    ) -> Result<Option<transaction::Result<()>>, BanksClientError> {
        self.inner
            .process_transaction_with_commitment_and_context(ctx, transaction.into(), commitment)
            .await
            .map_err(Into::into)
    }

    pub async fn process_transaction_with_preflight_and_commitment_and_context(
        &self,
        ctx: Context,
        transaction: impl Into<VersionedTransaction>,
        commitment: CommitmentLevel,
    ) -> Result<BanksTransactionResultWithSimulation, BanksClientError> {
        self.inner
            .process_transaction_with_preflight_and_commitment_and_context(
                ctx,
                transaction.into(),
                commitment,
            )
            .await
            .map_err(Into::into)
    }

    pub async fn process_transaction_with_metadata_and_context(
        &self,
        ctx: Context,
        transaction: impl Into<VersionedTransaction>,
    ) -> Result<BanksTransactionResultWithMetadata, BanksClientError> {
        self.inner
            .process_transaction_with_metadata_and_context(ctx, transaction.into())
            .await
            .map_err(Into::into)
    }

    pub async fn simulate_transaction_with_commitment_and_context(
        &self,
        ctx: Context,
        transaction: impl Into<VersionedTransaction>,
        commitment: CommitmentLevel,
    ) -> Result<BanksTransactionResultWithSimulation, BanksClientError> {
        self.inner
            .simulate_transaction_with_commitment_and_context(ctx, transaction.into(), commitment)
            .await
            .map_err(Into::into)
    }

    pub async fn get_account_with_commitment_and_context(
        &self,
        ctx: Context,
        address: Pubkey,
        commitment: CommitmentLevel,
    ) -> Result<Option<Account>, BanksClientError> {
        self.inner
            .get_account_with_commitment_and_context(ctx, address, commitment)
            .await
            .map_err(Into::into)
    }

    /// Send a transaction and return immediately. The server will resend the
    /// transaction until either it is accepted by the cluster or the transaction's
    /// blockhash expires.
    pub async fn send_transaction(
        &self,
        transaction: impl Into<VersionedTransaction>,
    ) -> Result<(), BanksClientError> {
        self.send_transaction_with_context(context::current(), transaction.into())
            .await
    }

    /// Return the cluster Sysvar
    pub async fn get_sysvar<T: Sysvar>(&self) -> Result<T, BanksClientError> {
        let sysvar = self
            .get_account(T::id())
            .await?
            .ok_or(BanksClientError::ClientError("Sysvar not present"))?;
        from_account::<T, _>(&sysvar).ok_or(BanksClientError::ClientError(
            "Failed to deserialize sysvar",
        ))
    }

    /// Return the cluster rent
    pub async fn get_rent(&self) -> Result<Rent, BanksClientError> {
        self.get_sysvar::<Rent>().await
    }

    /// Send a transaction and return after the transaction has been rejected or
    /// reached the given level of commitment.
    pub async fn process_transaction_with_commitment(
        &self,
        transaction: impl Into<VersionedTransaction>,
        commitment: CommitmentLevel,
    ) -> Result<(), BanksClientError> {
        let ctx = context::current();
        match self
            .process_transaction_with_commitment_and_context(ctx, transaction, commitment)
            .await?
        {
            None => Err(BanksClientError::ClientError(
                "invalid blockhash or fee-payer",
            )),
            Some(transaction_result) => Ok(transaction_result?),
        }
    }

    /// Process a transaction and return the result with metadata.
    pub async fn process_transaction_with_metadata(
        &self,
        transaction: impl Into<VersionedTransaction>,
    ) -> Result<BanksTransactionResultWithMetadata, BanksClientError> {
        let ctx = context::current();
        self.process_transaction_with_metadata_and_context(ctx, transaction.into())
            .await
    }

    /// Send a transaction and return any preflight (sanitization or simulation) errors, or return
    /// after the transaction has been rejected or reached the given level of commitment.
    pub async fn process_transaction_with_preflight_and_commitment(
        &self,
        transaction: impl Into<VersionedTransaction>,
        commitment: CommitmentLevel,
    ) -> Result<(), BanksClientError> {
        let ctx = context::current();
        match self
            .process_transaction_with_preflight_and_commitment_and_context(
                ctx,
                transaction,
                commitment,
            )
            .await?
        {
            BanksTransactionResultWithSimulation {
                result: None,
                simulation_details: _,
            } => Err(BanksClientError::ClientError(
                "invalid blockhash or fee-payer",
            )),
            BanksTransactionResultWithSimulation {
                result: Some(Err(err)),
                simulation_details: Some(simulation_details),
            } => Err(BanksClientError::SimulationError {
                err,
                logs: simulation_details.logs,
                units_consumed: simulation_details.units_consumed,
                return_data: simulation_details.return_data,
            }),
            BanksTransactionResultWithSimulation {
                result: Some(result),
                simulation_details: _,
            } => result.map_err(Into::into),
        }
    }

    /// Send a transaction and return any preflight (sanitization or simulation) errors, or return
    /// after the transaction has been finalized or rejected.
    pub async fn process_transaction_with_preflight(
        &self,
        transaction: impl Into<VersionedTransaction>,
    ) -> Result<(), BanksClientError> {
        self.process_transaction_with_preflight_and_commitment(
            transaction,
            CommitmentLevel::default(),
        )
        .await
    }

    /// Send a transaction and return until the transaction has been finalized or rejected.
    pub async fn process_transaction(
        &self,
        transaction: impl Into<VersionedTransaction>,
    ) -> Result<(), BanksClientError> {
        self.process_transaction_with_commitment(transaction, CommitmentLevel::default())
            .await
    }

    pub async fn process_transactions_with_commitment<T: Into<VersionedTransaction>>(
        &self,
        transactions: Vec<T>,
        commitment: CommitmentLevel,
    ) -> Result<(), BanksClientError> {
        let mut clients: Vec<_> = transactions.iter().map(|_| self.clone()).collect();
        let futures = clients
            .iter_mut()
            .zip(transactions)
            .map(|(client, transaction)| {
                client.process_transaction_with_commitment(transaction, commitment)
            });
        let statuses = join_all(futures).await;
        statuses.into_iter().collect() // Convert Vec<Result<_, _>> to Result<Vec<_>>
    }

    /// Send transactions and return until the transaction has been finalized or rejected.
    pub async fn process_transactions<'a, T: Into<VersionedTransaction> + 'a>(
        &'a self,
        transactions: Vec<T>,
    ) -> Result<(), BanksClientError> {
        self.process_transactions_with_commitment(transactions, CommitmentLevel::default())
            .await
    }

    /// Simulate a transaction at the given commitment level
    pub async fn simulate_transaction_with_commitment(
        &self,
        transaction: impl Into<VersionedTransaction>,
        commitment: CommitmentLevel,
    ) -> Result<BanksTransactionResultWithSimulation, BanksClientError> {
        self.simulate_transaction_with_commitment_and_context(
            context::current(),
            transaction,
            commitment,
        )
        .await
    }

    /// Simulate a transaction at the default commitment level
    pub async fn simulate_transaction(
        &self,
        transaction: impl Into<VersionedTransaction>,
    ) -> Result<BanksTransactionResultWithSimulation, BanksClientError> {
        self.simulate_transaction_with_commitment(transaction, CommitmentLevel::default())
            .await
    }

    /// Return the most recent rooted slot. All transactions at or below this slot
    /// are said to be finalized. The cluster will not fork to a higher slot.
    pub async fn get_root_slot(&self) -> Result<Slot, BanksClientError> {
        self.get_slot_with_context(context::current(), CommitmentLevel::default())
            .await
    }

    /// Return the most recent rooted block height. All transactions at or below this height
    /// are said to be finalized. The cluster will not fork to a higher block height.
    pub async fn get_root_block_height(&self) -> Result<Slot, BanksClientError> {
        self.get_block_height_with_context(context::current(), CommitmentLevel::default())
            .await
    }

    /// Return the account at the given address at the slot corresponding to the given
    /// commitment level. If the account is not found, None is returned.
    pub async fn get_account_with_commitment(
        &self,
        address: Pubkey,
        commitment: CommitmentLevel,
    ) -> Result<Option<Account>, BanksClientError> {
        self.get_account_with_commitment_and_context(context::current(), address, commitment)
            .await
    }

    /// Return the account at the given address at the time of the most recent root slot.
    /// If the account is not found, None is returned.
    pub async fn get_account(&self, address: Pubkey) -> Result<Option<Account>, BanksClientError> {
        self.get_account_with_commitment(address, CommitmentLevel::default())
            .await
    }

    /// Return the unpacked account data at the given address
    /// If the account is not found, an error is returned
    pub async fn get_packed_account_data<T: Pack>(
        &self,
        address: Pubkey,
    ) -> Result<T, BanksClientError> {
        let account = self
            .get_account(address)
            .await?
            .ok_or(BanksClientError::ClientError("Account not found"))?;
        T::unpack_from_slice(&account.data)
            .map_err(|_| BanksClientError::ClientError("Failed to deserialize account"))
    }

    /// Return the unpacked account data at the given address
    /// If the account is not found, an error is returned
    pub async fn get_account_data_with_borsh<T: BorshDeserialize>(
        &self,
        address: Pubkey,
    ) -> Result<T, BanksClientError> {
        let account = self
            .get_account(address)
            .await?
            .ok_or(BanksClientError::ClientError("Account not found"))?;
        T::try_from_slice(&account.data).map_err(Into::into)
    }

    /// Return the balance in lamports of an account at the given address at the slot
    /// corresponding to the given commitment level.
    pub async fn get_balance_with_commitment(
        &self,
        address: Pubkey,
        commitment: CommitmentLevel,
    ) -> Result<u64, BanksClientError> {
        Ok(self
            .get_account_with_commitment_and_context(context::current(), address, commitment)
            .await?
            .map(|x| x.lamports)
            .unwrap_or(0))
    }

    /// Return the balance in lamports of an account at the given address at the time
    /// of the most recent root slot.
    pub async fn get_balance(&self, address: Pubkey) -> Result<u64, BanksClientError> {
        self.get_balance_with_commitment(address, CommitmentLevel::default())
            .await
    }

    /// Return the status of a transaction with a signature matching the transaction's first
    /// signature. Return None if the transaction is not found, which may be because the
    /// blockhash was expired or the fee-paying account had insufficient funds to pay the
    /// transaction fee. Note that servers rarely store the full transaction history. This
    /// method may return None if the transaction status has been discarded.
    pub async fn get_transaction_status(
        &self,
        signature: Signature,
    ) -> Result<Option<TransactionStatus>, BanksClientError> {
        self.get_transaction_status_with_context(context::current(), signature)
            .await
    }

    /// Same as get_transaction_status, but for multiple transactions.
    pub async fn get_transaction_statuses(
        &self,
        signatures: Vec<Signature>,
    ) -> Result<Vec<Option<TransactionStatus>>, BanksClientError> {
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

    pub async fn get_latest_blockhash(&self) -> Result<Hash, BanksClientError> {
        self.get_latest_blockhash_with_commitment(CommitmentLevel::default())
            .await?
            .map(|x| x.0)
            .ok_or(BanksClientError::ClientError("valid blockhash not found"))
            .map_err(Into::into)
    }

    pub async fn get_latest_blockhash_with_commitment(
        &self,
        commitment: CommitmentLevel,
    ) -> Result<Option<(Hash, u64)>, BanksClientError> {
        self.get_latest_blockhash_with_commitment_and_context(context::current(), commitment)
            .await
    }

    pub async fn get_latest_blockhash_with_commitment_and_context(
        &self,
        ctx: Context,
        commitment: CommitmentLevel,
    ) -> Result<Option<(Hash, u64)>, BanksClientError> {
        self.inner
            .get_latest_blockhash_with_commitment_and_context(ctx, commitment)
            .await
            .map_err(Into::into)
    }

    pub async fn get_fee_for_message(
        &self,
        message: Message,
    ) -> Result<Option<u64>, BanksClientError> {
        self.get_fee_for_message_with_commitment_and_context(
            context::current(),
            message,
            CommitmentLevel::default(),
        )
        .await
    }

    pub async fn get_fee_for_message_with_commitment(
        &self,
        message: Message,
        commitment: CommitmentLevel,
    ) -> Result<Option<u64>, BanksClientError> {
        self.get_fee_for_message_with_commitment_and_context(
            context::current(),
            message,
            commitment,
        )
        .await
    }

    pub async fn get_fee_for_message_with_commitment_and_context(
        &self,
        ctx: Context,
        message: Message,
        commitment: CommitmentLevel,
    ) -> Result<Option<u64>, BanksClientError> {
        self.inner
            .get_fee_for_message_with_commitment_and_context(ctx, message, commitment)
            .await
            .map_err(Into::into)
    }
}

pub async fn start_client<C>(transport: C) -> Result<BanksClient, BanksClientError>
where
    C: Transport<ClientMessage<BanksRequest>, Response<BanksResponse>> + Send + 'static,
{
    Ok(BanksClient {
        inner: TarpcClient::new(client::Config::default(), transport).spawn(),
    })
}

pub async fn start_tcp_client<T: ToSocketAddrs>(addr: T) -> Result<BanksClient, BanksClientError> {
    let transport = tcp::connect(addr, Bincode::default).await?;
    Ok(BanksClient {
        inner: TarpcClient::new(client::Config::default(), transport).spawn(),
    })
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        solana_banks_server::banks_server::start_local_server,
        solana_runtime::{
            bank::Bank, bank_forks::BankForks, commitment::BlockCommitmentCache,
            genesis_utils::create_genesis_config,
        },
        solana_sdk::{
            message::Message, signature::Signer, system_instruction, transaction::Transaction,
        },
        std::sync::{Arc, RwLock},
        tarpc::transport,
        tokio::{
            runtime::Runtime,
            time::{sleep, Duration},
        },
    };

    #[test]
    fn test_banks_client_new() {
        let (client_transport, _server_transport) = transport::channel::unbounded();
        BanksClient::new(client::Config::default(), client_transport);
    }

    #[test]
    #[allow(clippy::result_large_err)]
    fn test_banks_server_transfer_via_server() -> Result<(), BanksClientError> {
        // This test shows the preferred way to interact with BanksServer.
        // It creates a runtime explicitly (no globals via tokio macros) and calls
        // `runtime.block_on()` just once, to run all the async code.

        let genesis = create_genesis_config(10);
        let bank = Bank::new_for_tests(&genesis.genesis_config);
        let slot = bank.slot();
        let block_commitment_cache = Arc::new(RwLock::new(
            BlockCommitmentCache::new_for_tests_with_slots(slot, slot),
        ));
        let bank_forks = BankForks::new_rw_arc(bank);

        let bob_pubkey = solana_sdk::pubkey::new_rand();
        let mint_pubkey = genesis.mint_keypair.pubkey();
        let instruction = system_instruction::transfer(&mint_pubkey, &bob_pubkey, 1);
        let message = Message::new(&[instruction], Some(&mint_pubkey));

        Runtime::new()?.block_on(async {
            let client_transport =
                start_local_server(bank_forks, block_commitment_cache, Duration::from_millis(1))
                    .await;
            let banks_client = start_client(client_transport).await?;

            let recent_blockhash = banks_client.get_latest_blockhash().await?;
            let transaction = Transaction::new(&[&genesis.mint_keypair], message, recent_blockhash);
            let simulation_result = banks_client
                .simulate_transaction(transaction.clone())
                .await
                .unwrap();
            assert!(simulation_result.result.unwrap().is_ok());
            banks_client.process_transaction(transaction).await.unwrap();
            assert_eq!(banks_client.get_balance(bob_pubkey).await?, 1);
            Ok(())
        })
    }

    #[test]
    #[allow(clippy::result_large_err)]
    fn test_banks_server_transfer_via_client() -> Result<(), BanksClientError> {
        // The caller may not want to hold the connection open until the transaction
        // is processed (or blockhash expires). In this test, we verify the
        // server-side functionality is available to the client.

        let genesis = create_genesis_config(10);
        let bank = Bank::new_for_tests(&genesis.genesis_config);
        let slot = bank.slot();
        let block_commitment_cache = Arc::new(RwLock::new(
            BlockCommitmentCache::new_for_tests_with_slots(slot, slot),
        ));
        let bank_forks = BankForks::new_rw_arc(bank);

        let mint_pubkey = &genesis.mint_keypair.pubkey();
        let bob_pubkey = solana_sdk::pubkey::new_rand();
        let instruction = system_instruction::transfer(mint_pubkey, &bob_pubkey, 1);
        let message = Message::new(&[instruction], Some(mint_pubkey));

        Runtime::new()?.block_on(async {
            let client_transport =
                start_local_server(bank_forks, block_commitment_cache, Duration::from_millis(1))
                    .await;
            let banks_client = start_client(client_transport).await?;
            let (recent_blockhash, last_valid_block_height) = banks_client
                .get_latest_blockhash_with_commitment(CommitmentLevel::default())
                .await?
                .unwrap();
            let transaction = Transaction::new(&[&genesis.mint_keypair], message, recent_blockhash);
            let signature = transaction.signatures[0];
            banks_client.send_transaction(transaction).await?;

            let mut status = banks_client.get_transaction_status(signature).await?;

            while status.is_none() {
                let root_block_height = banks_client.get_root_block_height().await?;
                if root_block_height > last_valid_block_height {
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
