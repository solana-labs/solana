//! PayTube's "account loader" component, which provides the SVM API with the
//! ability to load accounts for PayTube channels.
//!
//! The account loader is a simple example of an RPC client that can first load
//! an account from the base chain, then cache it locally within the protocol
//! for the duration of the channel.

use {
    solana_client::rpc_client::RpcClient,
    solana_sdk::{
        account::{AccountSharedData, ReadableAccount},
        pubkey::Pubkey,
    },
    solana_svm::transaction_processing_callback::TransactionProcessingCallback,
    std::{collections::HashMap, sync::RwLock},
};

/// An account loading mechanism to hoist accounts from the base chain up to
/// an active PayTube channel.
///
/// Employs a simple cache mechanism to ensure accounts are only loaded once.
pub struct PayTubeAccountLoader<'a> {
    cache: RwLock<HashMap<Pubkey, AccountSharedData>>,
    rpc_client: &'a RpcClient,
}

impl<'a> PayTubeAccountLoader<'a> {
    pub fn new(rpc_client: &'a RpcClient) -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
            rpc_client,
        }
    }
}

/// Implementation of the SVM API's `TransactionProcessingCallback` interface.
///
/// The SVM API requires this plugin be provided to provide the SVM with the
/// ability to load accounts.
///
/// In the Agave validator, this implementation is Bank, powered by AccountsDB.
impl TransactionProcessingCallback for PayTubeAccountLoader<'_> {
    fn get_account_shared_data(&self, pubkey: &Pubkey) -> Option<AccountSharedData> {
        if let Some(account) = self.cache.read().unwrap().get(pubkey) {
            return Some(account.clone());
        }

        let account: AccountSharedData = self.rpc_client.get_account(pubkey).ok()?.into();
        self.cache.write().unwrap().insert(*pubkey, account.clone());

        Some(account)
    }

    fn account_matches_owners(&self, account: &Pubkey, owners: &[Pubkey]) -> Option<usize> {
        self.get_account_shared_data(account)
            .and_then(|account| owners.iter().position(|key| account.owner().eq(key)))
    }
}
