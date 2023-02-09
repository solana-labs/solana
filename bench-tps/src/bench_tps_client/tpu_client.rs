use {
    crate::bench_tps_client::{BenchTpsClient, BenchTpsError, Result},
    solana_client::tpu_client::TpuClient,
    solana_connection_cache::connection_cache::{
        ConnectionManager, ConnectionPool, NewConnectionConfig,
    },
    solana_sdk::{
        account::Account, commitment_config::CommitmentConfig, epoch_info::EpochInfo, hash::Hash,
        message::Message, pubkey::Pubkey, signature::Signature, transaction::Transaction,
    },
};

impl<P, M, C> BenchTpsClient for TpuClient<P, M, C>
where
    P: ConnectionPool<NewConnectionConfig = C>,
    M: ConnectionManager<ConnectionPool = P, NewConnectionConfig = C>,
    C: NewConnectionConfig,
{
    fn send_transaction(&self, transaction: Transaction) -> Result<Signature> {
        let signature = transaction.signatures[0];
        self.try_send_transaction(&transaction)?;
        Ok(signature)
    }
    fn send_batch(&self, transactions: Vec<Transaction>) -> Result<()> {
        self.try_send_transaction_batch(&transactions)?;
        Ok(())
    }
    fn get_latest_blockhash(&self) -> Result<Hash> {
        self.rpc_client()
            .get_latest_blockhash()
            .map_err(|err| err.into())
    }

    fn get_latest_blockhash_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> Result<(Hash, u64)> {
        self.rpc_client()
            .get_latest_blockhash_with_commitment(commitment_config)
            .map_err(|err| err.into())
    }

    fn get_transaction_count(&self) -> Result<u64> {
        self.rpc_client()
            .get_transaction_count()
            .map_err(|err| err.into())
    }

    fn get_transaction_count_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> Result<u64> {
        self.rpc_client()
            .get_transaction_count_with_commitment(commitment_config)
            .map_err(|err| err.into())
    }

    fn get_epoch_info(&self) -> Result<EpochInfo> {
        self.rpc_client().get_epoch_info().map_err(|err| err.into())
    }

    fn get_balance(&self, pubkey: &Pubkey) -> Result<u64> {
        self.rpc_client()
            .get_balance(pubkey)
            .map_err(|err| err.into())
    }

    fn get_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> Result<u64> {
        self.rpc_client()
            .get_balance_with_commitment(pubkey, commitment_config)
            .map(|res| res.value)
            .map_err(|err| err.into())
    }

    fn get_fee_for_message(&self, message: &Message) -> Result<u64> {
        self.rpc_client()
            .get_fee_for_message(message)
            .map_err(|err| err.into())
    }

    fn get_minimum_balance_for_rent_exemption(&self, data_len: usize) -> Result<u64> {
        self.rpc_client()
            .get_minimum_balance_for_rent_exemption(data_len)
            .map_err(|err| err.into())
    }

    fn addr(&self) -> String {
        self.rpc_client().url()
    }

    fn request_airdrop_with_blockhash(
        &self,
        pubkey: &Pubkey,
        lamports: u64,
        recent_blockhash: &Hash,
    ) -> Result<Signature> {
        self.rpc_client()
            .request_airdrop_with_blockhash(pubkey, lamports, recent_blockhash)
            .map_err(|err| err.into())
    }

    fn get_account(&self, pubkey: &Pubkey) -> Result<Account> {
        self.rpc_client()
            .get_account(pubkey)
            .map_err(|err| err.into())
    }

    fn get_account_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> Result<Account> {
        self.rpc_client()
            .get_account_with_commitment(pubkey, commitment_config)
            .map(|res| res.value)
            .map_err(|err| err.into())
            .and_then(|account| {
                account.ok_or_else(|| {
                    BenchTpsError::Custom(format!("AccountNotFound: pubkey={pubkey}"))
                })
            })
    }

    fn get_multiple_accounts(&self, pubkeys: &[Pubkey]) -> Result<Vec<Option<Account>>> {
        self.rpc_client()
            .get_multiple_accounts(pubkeys)
            .map_err(|err| err.into())
    }
}
