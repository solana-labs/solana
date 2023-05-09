use {
    crate::bench_tps_client::{BenchTpsClient, BenchTpsError, Result},
    solana_runtime::bank_client::BankClient,
    solana_sdk::{
        account::Account,
        client::{AsyncClient, SyncClient},
        commitment_config::CommitmentConfig,
        epoch_info::EpochInfo,
        hash::Hash,
        message::Message,
        pubkey::Pubkey,
        signature::Signature,
        transaction::Transaction,
    },
};

impl BenchTpsClient for BankClient {
    fn send_transaction(&self, transaction: Transaction) -> Result<Signature> {
        AsyncClient::async_send_transaction(self, transaction).map_err(|err| err.into())
    }
    fn send_batch(&self, transactions: Vec<Transaction>) -> Result<()> {
        AsyncClient::async_send_batch(self, transactions).map_err(|err| err.into())
    }
    fn get_latest_blockhash(&self) -> Result<Hash> {
        SyncClient::get_latest_blockhash(self).map_err(|err| err.into())
    }

    fn get_latest_blockhash_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> Result<(Hash, u64)> {
        SyncClient::get_latest_blockhash_with_commitment(self, commitment_config)
            .map_err(|err| err.into())
    }

    fn get_transaction_count(&self) -> Result<u64> {
        SyncClient::get_transaction_count(self).map_err(|err| err.into())
    }

    fn get_transaction_count_with_commitment(
        &self,
        commitment_config: CommitmentConfig,
    ) -> Result<u64> {
        SyncClient::get_transaction_count_with_commitment(self, commitment_config)
            .map_err(|err| err.into())
    }

    fn get_epoch_info(&self) -> Result<EpochInfo> {
        SyncClient::get_epoch_info(self).map_err(|err| err.into())
    }

    fn get_balance(&self, pubkey: &Pubkey) -> Result<u64> {
        SyncClient::get_balance(self, pubkey).map_err(|err| err.into())
    }

    fn get_balance_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> Result<u64> {
        SyncClient::get_balance_with_commitment(self, pubkey, commitment_config)
            .map_err(|err| err.into())
    }

    fn get_fee_for_message(&self, message: &Message) -> Result<u64> {
        SyncClient::get_fee_for_message(self, message).map_err(|err| err.into())
    }

    fn get_minimum_balance_for_rent_exemption(&self, data_len: usize) -> Result<u64> {
        SyncClient::get_minimum_balance_for_rent_exemption(self, data_len).map_err(|err| err.into())
    }

    fn addr(&self) -> String {
        "Local BankClient".to_string()
    }

    fn request_airdrop_with_blockhash(
        &self,
        _pubkey: &Pubkey,
        _lamports: u64,
        _recent_blockhash: &Hash,
    ) -> Result<Signature> {
        // BankClient doesn't support airdrops
        Err(BenchTpsError::AirdropFailure)
    }

    fn get_account(&self, pubkey: &Pubkey) -> Result<Account> {
        SyncClient::get_account(self, pubkey)
            .map_err(|err| err.into())
            .and_then(|account| {
                account.ok_or_else(|| {
                    BenchTpsError::Custom(format!("AccountNotFound: pubkey={pubkey}"))
                })
            })
    }

    fn get_account_with_commitment(
        &self,
        pubkey: &Pubkey,
        commitment_config: CommitmentConfig,
    ) -> Result<Account> {
        SyncClient::get_account_with_commitment(self, pubkey, commitment_config)
            .map_err(|err| err.into())
            .and_then(|account| {
                account.ok_or_else(|| {
                    BenchTpsError::Custom(format!("AccountNotFound: pubkey={pubkey}"))
                })
            })
    }

    fn get_multiple_accounts(&self, _pubkeys: &[Pubkey]) -> Result<Vec<Option<Account>>> {
        unimplemented!("BankClient doesn't support get_multiple_accounts");
    }
}
